#![forbid(unsafe_code)]
#![deny(
    clippy::all,
    clippy::pedantic,
    clippy::cargo,
    clippy::cognitive_complexity
)]

use std::collections::HashMap;
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufWriter, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{ArgMatches, CommandFactory, FromArgMatches, Parser, ValueEnum};
use ignore::WalkBuilder;
use rayon::prelude::*;
use ryl::cli_support::{
    github_escape, lexical_abspath, report_display_path, resolve_ctx, sanitize_control,
};
use ryl::config::{
    ConfigContext, Overrides, SourceKind, SystemEnv, YamlLintConfig, discover_config,
    user_config_migration_paths,
};
use ryl::config_schema::{
    OutputDestination, OutputTable, schema_string_pretty, yaml_schema_string_pretty,
};
use ryl::decoder;
use ryl::fix::{
    DiffStats, apply_safe_fixes_to_files, diff_outcome, diff_safe_fixes_for_files,
};
use ryl::migrate::{
    MigrateOptions, OutputMode as MigrateOutputMode, SourceCleanup,
    UserConfigMigration, WriteMode, migrate_configs,
};
use ryl::report::{ReportEntry, render_gitlab, render_junit};
use ryl::{
    LintProblem, Severity, lint_file, lint_markdown_file, lint_markdown_str, lint_str,
};
use same_file::Handle;

const STDIN_LABEL: &str = "<stdin>";

// Formatted output is built in an owned `Vec<u8>`, whose `io::Write` never errors, so the
// per-diagnostic writes are `expect`s rather than `?`s that would leave dead error arms.
// Only the final write to the destination (file/stdout/stderr) is fallible.
const OUTPUT_INFALLIBLE: &str =
    "writing diagnostics to an in-memory buffer cannot fail";

// A resolved config that enables no rules would lint nothing while exiting 0 — a
// silent no-op that almost always means a misconfiguration. The lint commands fail
// loudly on it (config resolution itself does not, so `--migrate-configs` can still
// convert a rule-less config, and `--list-files` still answers its file query). ryl
// is deliberately stricter than yamllint here, which accepts a rule-less config.
const NO_RULES_ENABLED_ERROR: &str = "error: configuration enables no rules, so nothing would be linted; enable at \
     least one rule, or use 'extends: default' for the standard rule set";

const NO_CONFIG_ERROR: &str = "error: no configuration found and ryl enables no rules by default; create a \
     config that enables rules, or use 'extends: default' for the standard rule set";

/// Pick the error for a config that would lint nothing: distinguish "no
/// configuration was found anywhere" from "a configuration was provided/discovered
/// but enables no rules". Both name `extends: default` as the escape hatch.
fn no_rules_error(config_found: bool) -> String {
    if config_found {
        NO_RULES_ENABLED_ERROR.to_string()
    } else {
        NO_CONFIG_ERROR.to_string()
    }
}

fn gather_inputs(inputs: &[PathBuf]) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut explicit_files = Vec::new();
    let mut candidates = Vec::new();
    for p in inputs {
        if p.is_dir() {
            let walker = WalkBuilder::new(p)
                .hidden(false)
                .ignore(true)
                .git_ignore(true)
                .git_global(true)
                .git_exclude(true)
                .follow_links(false)
                .build();
            for e in walker.flatten() {
                let fp = e.path().to_path_buf();
                if fp.is_file() {
                    candidates.push(fp);
                }
            }
        } else {
            explicit_files.push(p.clone());
        }
    }
    (candidates, explicit_files)
}

fn cli_overrides(cli: &Cli) -> Overrides {
    Overrides {
        config_file: cli.config_file.clone(),
        config_data: cli.config_data.as_ref().map(|raw| {
            if !raw.is_empty() && !raw.contains(':') {
                format!("extends: {raw}")
            } else {
                raw.clone()
            }
        }),
    }
}

fn build_global_cfg(
    inputs: &[PathBuf],
    cli: &Cli,
) -> Result<Option<ConfigContext>, String> {
    if cli.config_data.is_some()
        || cli.config_file.is_some()
        || std::env::var("YAMLLINT_CONFIG_FILE").is_ok()
    {
        discover_config(inputs, &cli_overrides(cli)).map(Some)
    } else {
        Ok(None)
    }
}

/// The TOML `[output]` table governing the run, or `None` if no config declares one.
/// Read from the global config when one was provided (`-c`/`-d`/env), otherwise from the
/// project config discovered for the inputs (so `ryl .` picks up the project's
/// `.ryl.toml [output]`). This is a run-level setting, read once rather than per file; a
/// CLI `--format` overrides whatever it returns.
///
/// `[output]` is a single, run-level artifact set, so it is sourced from one config. For a
/// single root (the usual case) that is unambiguous, and inputs that are subdirectories of
/// one project share that project's config. A run spanning *separate* projects with their
/// own differing `[output]` tables takes the first project config discovered along the
/// inputs (deterministic for a given argument list, but argument-order sensitive); pass
/// `-c`/`-d` to make the output config explicit when that matters.
///
/// # Errors
///
/// Propagates a config discovery/parse/validation error from the inputs-anchored project
/// config. With lintable files present this is the same config the per-file path also
/// reports on; the value here is the empty-input case, where no per-file discovery runs,
/// so a malformed run config (e.g. an invalid `[output]`) is surfaced rather than silently
/// ignored — and a CI step relying on a configured report is not left without one.
fn run_output_config(
    global_cfg: Option<&ConfigContext>,
    inputs: &[PathBuf],
    cli: &Cli,
) -> Result<Option<OutputTable>, String> {
    if let Some(ctx) = global_cfg {
        return Ok(ctx.config.output().cloned());
    }
    Ok(discover_config(inputs, &cli_overrides(cli))?
        .config
        .output()
        .cloned())
}

fn run_migration(cli: &Cli) -> Result<ExitCode, String> {
    let cleanup = if let Some(suffix) = &cli.migrate.rename_old {
        SourceCleanup::RenameSuffix(suffix.clone())
    } else if cli.migrate.delete_old {
        SourceCleanup::Delete
    } else {
        SourceCleanup::Keep
    };
    let project_root = cli.migrate_configs.then(|| {
        cli.migrate
            .root
            .clone()
            .unwrap_or_else(|| PathBuf::from("."))
    });
    let user_config = if cli.migrate_user_config {
        user_config_migration_paths(&SystemEnv)
            .map(|(source, target)| UserConfigMigration { source, target })
    } else {
        None
    };
    let options = MigrateOptions {
        project_root: project_root.clone(),
        user_config: user_config.clone(),
        write_mode: if cli.migrate.write {
            WriteMode::Write
        } else {
            WriteMode::Preview
        },
        output_mode: if cli.migrate.stdout {
            MigrateOutputMode::IncludeToml
        } else {
            MigrateOutputMode::SummaryOnly
        },
        cleanup,
    };
    let result = migrate_configs(&options)?;
    for warning in &result.warnings {
        eprintln!("{}", sanitize_control(warning));
    }
    for entry in &result.entries {
        println!(
            "{} -> {}",
            sanitize_control(&entry.source.display().to_string()),
            sanitize_control(&entry.target.display().to_string())
        );
    }
    // Per-trigger "nothing migrated" feedback, reported independently so a combined run
    // still surfaces an empty trigger even when the other produced entries. The single
    // user-global entry (if any) is identified by its source path; the rest are project.
    let user_source = user_config.as_ref().map(|user| user.source.as_path());
    if let Some(root) = &project_root {
        let project_migrated = result
            .entries
            .iter()
            .any(|entry| Some(entry.source.as_path()) != user_source);
        if !project_migrated {
            println!(
                "No legacy YAML config files migrated under {}",
                sanitize_control(&root.display().to_string())
            );
        }
    }
    if cli.migrate_user_config {
        let user_migrated = user_source
            .is_some_and(|source| result.entries.iter().any(|e| e.source == source));
        if !user_migrated {
            println!("No yamllint user-global config migrated.");
        }
    }
    if options.output_mode == MigrateOutputMode::IncludeToml {
        for entry in &result.entries {
            println!(
                "# {}",
                sanitize_control(&entry.target.display().to_string())
            );
            println!("{}", entry.toml);
        }
    }
    Ok(ExitCode::SUCCESS)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum CliFormat {
    Auto,
    Standard,
    Colored,
    Github,
    Parsable,
    Junit,
    Gitlab,
}

#[derive(Parser, Debug)]
#[command(
    name = "ryl",
    version,
    about = "Fast YAML linter written in Rust",
    // Shared `--migrate-*` sub-flags require at least one migration trigger; either
    // `--migrate-configs` (project tree) or `--migrate-user-config` (user-global) works.
    group(clap::ArgGroup::new("migrate_mode")
        .args(["migrate_configs", "migrate_user_config"])
        .multiple(true))
)]
// `ryl server` (the LSP) is the only subcommand; bare `ryl <paths>` still lints, so the
// lint args and the subcommand are mutually exclusive.
#[cfg_attr(feature = "lsp", command(args_conflicts_with_subcommands = true))]
// CLI flags are independent user-facing toggles, not state better modeled as an enum.
#[allow(clippy::struct_excessive_bools)]
struct Cli {
    /// One or more paths: files and/or directories, or `-` to read from stdin
    #[arg(value_name = "PATH_OR_FILE")]
    inputs: Vec<PathBuf>,

    /// Filename used for diagnostics, config discovery, and yaml-files matching when reading stdin
    #[arg(long = "stdin-filename", value_name = "FILE")]
    stdin_filename: Option<PathBuf>,

    /// Path to configuration file (YAML or TOML)
    #[arg(short = 'c', long = "config-file", value_name = "FILE")]
    config_file: Option<PathBuf>,

    /// Inline configuration data (yaml)
    #[arg(short = 'd', long = "config-data", value_name = "YAML")]
    config_data: Option<String>,

    /// Output format (auto, standard, colored, github, parsable, junit, gitlab). Repeatable:
    /// each `--format` may be followed by an `--output-file` to send that format to a file,
    /// so console and report artifacts can be produced together.
    #[arg(short = 'f', long = "format", value_enum)]
    format: Vec<CliFormat>,

    /// Destination for the preceding `--format` (a path, or `-` for stdout). Repeatable;
    /// each binds to the most recent `--format`. Default stream otherwise: stderr for the
    /// console formats, stdout for junit/gitlab.
    #[arg(
        short = 'o',
        long = "output-file",
        value_name = "FILE",
        conflicts_with = "diff"
    )]
    output_file: Vec<PathBuf>,

    // These print-and-exit meta-actions ignore every other input/flag; mark them
    // `exclusive` so combining them with a lint/fix/format request is a usage error
    // rather than a silent no-op. `--migrate-configs` is deliberately not exclusive
    // (it combines with its `requires`-bound `--migrate-*` sub-flags and a root path).
    /// Print the JSON Schema for ryl TOML config and exit
    #[arg(
        long = "print-toml-config-schema",
        default_value_t = false,
        exclusive = true
    )]
    print_toml_config_schema: bool,

    /// Print the JSON Schema for yamllint-compatible YAML config and exit
    #[arg(
        long = "print-yaml-config-schema",
        default_value_t = false,
        exclusive = true
    )]
    print_yaml_config_schema: bool,

    /// Convert discovered legacy YAML config files into .ryl.toml files
    #[arg(long = "migrate-configs", default_value_t = false)]
    migrate_configs: bool,

    /// Convert the yamllint user-global config into ryl's own user-global ryl.toml
    #[arg(long = "migrate-user-config", default_value_t = false)]
    migrate_user_config: bool,

    /// Print a shell completion script for SHELL and exit
    #[arg(
        long = "generate-completions",
        value_name = "SHELL",
        value_enum,
        exclusive = true
    )]
    generate_completions: Option<clap_complete::Shell>,

    #[command(flatten)]
    lint: LintFlags,

    #[command(flatten)]
    migrate: MigrateFlags,

    #[cfg(feature = "lsp")]
    #[command(subcommand)]
    command: Option<Commands>,
}

/// Subcommands. None (bare `ryl <paths>`) lints, preserving the historical flat CLI.
/// As with any subcommand-based CLI (cargo, ruff), the bare token `server` resolves to
/// this subcommand rather than a path of that name; lint such a path as `ryl ./server`
/// or `ryl server/` (only the exact bare `server` collides, and it matches no yaml glob).
#[cfg(feature = "lsp")]
#[derive(clap::Subcommand, Debug)]
enum Commands {
    /// Run the language server (LSP) over stdio for editor integration
    Server,
}

#[derive(clap::Args, Debug, Default)]
struct LintFlags {
    #[command(flatten)]
    fix: FixFlags,

    #[command(flatten)]
    compatibility: CompatibilityLintFlags,

    /// Lint inputs as Markdown (embedded YAML front matter and fenced yaml/yml
    /// blocks) using default globs, without configuring `[files].markdown`
    #[arg(long = "markdown", default_value_t = false)]
    markdown: bool,
}

#[derive(clap::Args, Debug, Default)]
struct FixFlags {
    /// Apply safe fixes in place before reporting remaining diagnostics
    #[arg(long = "fix", default_value_t = false)]
    fix: bool,

    /// Print a unified diff of the safe fixes to stdout instead of writing them;
    /// never modifies files and exits 1 if any file would change
    #[arg(long = "diff", default_value_t = false, conflicts_with = "fix")]
    diff: bool,
}

#[derive(clap::Args, Debug, Default)]
struct CompatibilityLintFlags {
    /// List the files that would be linted (one per line), then exit
    #[arg(long = "list-files", default_value_t = false)]
    list_files: bool,

    /// Return exit code 2 when only warnings (no errors) are found
    #[arg(short = 's', long = "strict", default_value_t = false)]
    strict: bool,

    /// Suppress warning-level problems (report only errors)
    #[arg(long = "no-warnings", default_value_t = false)]
    no_warnings: bool,
}

#[derive(clap::Args, Debug, Default)]
struct MigrateFlags {
    /// Root path to search for legacy YAML config files (default: .)
    #[arg(
        long = "migrate-root",
        value_name = "DIR",
        requires = "migrate_configs"
    )]
    root: Option<PathBuf>,

    /// Write migrated TOML files (otherwise preview only)
    #[arg(
        long = "migrate-write",
        default_value_t = false,
        requires = "migrate_mode"
    )]
    write: bool,

    /// Print generated TOML to stdout during migration
    #[arg(
        long = "migrate-stdout",
        default_value_t = false,
        requires = "migrate_mode"
    )]
    stdout: bool,

    /// Rename source YAML configs by appending this suffix after migration
    #[arg(
        long = "migrate-rename-old",
        value_name = "SUFFIX",
        conflicts_with = "delete_old",
        requires_all = ["write", "migrate_mode"]
    )]
    rename_old: Option<String>,

    /// Delete source YAML configs after migration
    #[arg(
        long = "migrate-delete-old",
        default_value_t = false,
        conflicts_with = "rename_old",
        requires_all = ["write", "migrate_mode"]
    )]
    delete_old: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OutputFormat {
    Standard,
    Colored,
    Github,
    Parsable,
    Junit,
    Gitlab,
}

impl OutputFormat {
    /// Streaming formats emit one line per diagnostic as files are visited; the
    /// whole-document formats (junit/gitlab) buffer every diagnostic and serialize once.
    const fn is_streaming(self) -> bool {
        matches!(
            self,
            Self::Standard | Self::Colored | Self::Github | Self::Parsable
        )
    }
}

/// Where one output is written.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Destination {
    Stdout,
    Stderr,
    File(PathBuf),
}

/// One resolved output: a format and where it goes. The unit both the CLI
/// (`--format` + paired `--output-file`) and the TOML `[output]` table resolve to.
struct OutputTarget {
    format: OutputFormat,
    destination: Destination,
}

/// A format's destination when none is given: console formats go to stderr (unchanged),
/// the whole-document report formats to stdout so they can be piped/redirected.
fn default_destination(format: OutputFormat) -> Destination {
    if format.is_streaming() {
        Destination::Stderr
    } else {
        Destination::Stdout
    }
}

/// Resolve the `--format`/`--output-file` occurrences into output targets, pairing each
/// `--output-file` with the most recent `--format` (RuboCop/Biome style). `-` means stdout.
/// Returns an empty vec when no `--format` was given (the caller then falls back to config
/// `[output]` or the default). Relies on clap arg indices to recover CLI order.
///
/// # Errors
///
/// Returns a usage error for an `--output-file` with no preceding `--format`, or a second
/// `--output-file` bound to the same `--format`.
fn resolve_cli_targets(
    matches: &ArgMatches,
    cli: &Cli,
) -> Result<Vec<OutputTarget>, String> {
    enum Occurrence {
        Format(OutputFormat),
        Output(PathBuf),
    }
    let mut occurrences: Vec<(usize, Occurrence)> = Vec::new();
    if let Some(indices) = matches.indices_of("format") {
        for (index, format) in indices.zip(&cli.format) {
            occurrences
                .push((index, Occurrence::Format(detect_output_format(*format))));
        }
    }
    if let Some(indices) = matches.indices_of("output_file") {
        for (index, path) in indices.zip(&cli.output_file) {
            occurrences.push((index, Occurrence::Output(path.clone())));
        }
    }
    occurrences.sort_by_key(|(index, _)| *index);

    // Build (format, explicit destination?) pairs in CLI order; fill defaults at the end.
    let mut pending: Vec<(OutputFormat, Option<Destination>)> = Vec::new();
    for (_, occurrence) in occurrences {
        match occurrence {
            Occurrence::Format(format) => pending.push((format, None)),
            Occurrence::Output(path) => {
                let Some((_, destination)) = pending.last_mut() else {
                    return Err(
                        "error: --output-file must follow a --format".to_string()
                    );
                };
                if destination.is_some() {
                    return Err(
                        "error: a --format takes at most one --output-file".to_string()
                    );
                }
                *destination = Some(if path.as_os_str() == "-" {
                    Destination::Stdout
                } else {
                    Destination::File(path)
                });
            }
        }
    }
    Ok(pending
        .into_iter()
        .map(|(format, destination)| OutputTarget {
            format,
            destination: destination.unwrap_or_else(|| default_destination(format)),
        })
        .collect())
}

/// The output targets for a run, in precedence order CLI > config > default: the CLI
/// `--format`/`--output-file` pairs when any `--format` was given, otherwise the TOML
/// `[output]` table from the run's config when it declares any target, otherwise the
/// single default target (the auto-detected console format on its default stream).
///
/// # Errors
///
/// Propagates a `--format`/`--output-file` pairing error from [`resolve_cli_targets`].
fn resolve_targets(
    matches: &ArgMatches,
    cli: &Cli,
    config_output: Option<&OutputTable>,
) -> Result<Vec<OutputTarget>, String> {
    let cli_targets = resolve_cli_targets(matches, cli)?;
    if !cli_targets.is_empty() {
        return Ok(cli_targets);
    }
    if let Some(config_targets) = config_output.map(config_targets_from_table)
        && !config_targets.is_empty()
    {
        return Ok(config_targets);
    }
    let format = detect_output_format(CliFormat::Auto);
    Ok(vec![OutputTarget {
        destination: default_destination(format),
        format,
    }])
}

/// Resolve a TOML `[output]` table into targets, one per declared format, in
/// `OutputTable::entries` order (deterministic output and stream-conflict reporting). Each
/// entry's name maps to its `CliFormat` (the table field names are exactly the `--format`
/// value names), so `auto` is env-resolved like the CLI's `--format auto`.
fn config_targets_from_table(table: &OutputTable) -> Vec<OutputTarget> {
    table
        .entries()
        .into_iter()
        .filter_map(|(name, destination)| {
            let destination = destination?;
            let choice = CliFormat::from_str(name, false)
                .expect("OutputTable field names match CliFormat value names");
            Some(config_target(choice, destination))
        })
        .collect()
}

/// One config output entry to a target: `path` absent uses the format's default stream,
/// `"-"` is stdout, anything else is a file path.
fn config_target(choice: CliFormat, destination: &OutputDestination) -> OutputTarget {
    let format = detect_output_format(choice);
    let destination = match destination.path.as_deref() {
        None => default_destination(format),
        Some("-") => Destination::Stdout,
        Some(path) => Destination::File(PathBuf::from(path)),
    };
    OutputTarget {
        format,
        destination,
    }
}

fn detect_output_format(choice: CliFormat) -> OutputFormat {
    match choice {
        CliFormat::Standard => OutputFormat::Standard,
        CliFormat::Colored => OutputFormat::Colored,
        CliFormat::Github => OutputFormat::Github,
        CliFormat::Parsable => OutputFormat::Parsable,
        CliFormat::Junit => OutputFormat::Junit,
        CliFormat::Gitlab => OutputFormat::Gitlab,
        CliFormat::Auto => {
            if github_env_active() {
                OutputFormat::Github
            } else if supports_color() {
                OutputFormat::Colored
            } else {
                OutputFormat::Standard
            }
        }
    }
}

/// An opened output destination. A file is opened with create+write but **not** truncate,
/// so its current contents survive until [`OutputSink::commit`] truncates and rewrites it —
/// a later target failing to open then cannot destroy an *existing* artifact (a freshly
/// created one may be left empty if a later target aborts the run; see [`open_destination`]).
enum OutputSink {
    /// stdout or stderr (a console stream; written and flushed as-is).
    Stream(Box<dyn Write>),
    /// A `--output-file` target, truncated then written at commit time.
    File(File),
}

impl OutputSink {
    /// Write `bytes` as this sink's complete contents: a stream is written then flushed; a
    /// file is truncated (clearing any prior artifact) then written from the start and
    /// flushed. The single fallible step of the output pipeline (rendering is infallible,
    /// see [`OUTPUT_INFALLIBLE`]); the write/flush are chained into one coverable error.
    fn commit(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        match self {
            Self::Stream(writer) => {
                writer.write_all(bytes).and_then(|()| writer.flush())
            }
            Self::File(file) => {
                file.set_len(0).and_then(|()| file.write_all(bytes))?;
                file.flush()
            }
        }
    }
}

/// Open one destination for writing: stdout, stderr, or a file. A file is opened with
/// create+write but **not** truncate, so an unopenable `--output-file` fails fast (before
/// `--fix` mutates anything) yet an existing destination's contents survive until
/// [`OutputSink::commit`] truncates and rewrites it — a later target failing to open then
/// cannot destroy an existing artifact. (A *freshly*-created destination is left in place,
/// possibly empty, if a later target/collision aborts the run: cleaning it up by path would
/// race a concurrent writer at that path, so the empty artifact is left for the failed run
/// — gate CI artifact use on the exit code.) Console diagnostics are the only thing routed
/// here; notices, `--fix` summaries, and skip messages always stay on stderr.
///
/// # Errors
///
/// Returns an error if a [`Destination::File`] path cannot be opened/created.
fn open_destination(destination: &Destination) -> Result<OutputSink, String> {
    match destination {
        Destination::Stdout => Ok(OutputSink::Stream(Box::new(BufWriter::new(
            std::io::stdout(),
        )))),
        Destination::Stderr => Ok(OutputSink::Stream(Box::new(std::io::stderr()))),
        Destination::File(path) => {
            let file = File::options()
                .create(true)
                .write(true)
                .truncate(false)
                .open(path)
                .map_err(|err| {
                    format!(
                        "error: cannot open --output-file {}: {err}",
                        sanitize_control(&path.display().to_string())
                    )
                })?;
            Ok(OutputSink::File(file))
        }
    }
}

/// Open every target's destination up front (so an unopenable `--output-file` fails before
/// `--fix` mutates any source), then reject two outputs that resolve to the same file. The
/// collision check runs after opening, so each destination exists and `same_file::Handle`
/// resolves it through an existing symlink/hard link / aliased parent. Returns the sinks in
/// target order.
///
/// # Errors
///
/// Propagates the first [`open_destination`] failure, or a same-file collision among the
/// `--output-file` targets.
fn open_targets(targets: &[OutputTarget]) -> Result<Vec<OutputSink>, String> {
    let sinks = targets
        .iter()
        .map(|target| open_destination(&target.destination))
        .collect::<Result<Vec<_>, _>>()?;
    reject_colliding_output_files(targets)?;
    Ok(sinks)
}

/// Render `records` for each target and write the bytes to that target's (already-opened)
/// sink. Report entries are built once and shared across any report targets.
///
/// # Errors
///
/// Propagates the first destination write failure.
fn write_targets(
    targets: &[OutputTarget],
    sinks: &mut [OutputSink],
    records: &[FileRecord],
) -> Result<(), String> {
    let project_root = report_project_root();
    let entries = targets
        .iter()
        .any(|target| !target.format.is_streaming())
        .then(|| build_entries(records, &project_root));
    for (target, sink) in targets.iter().zip(sinks.iter_mut()) {
        let bytes = render_target(target.format, records, entries.as_deref());
        sink.commit(&bytes)
            .map_err(|err| write_output_error(&err))?;
    }
    Ok(())
}

/// Open every target's destination, then render and write `records` to each. The combined
/// open+write step for paths without a `--fix` ordering constraint (the empty-input and
/// stdin cases); the `--fix` path opens early via [`open_targets`] instead.
///
/// # Errors
///
/// Propagates an open or write failure.
fn emit_targets(
    targets: &[OutputTarget],
    records: &[FileRecord],
) -> Result<(), String> {
    let mut sinks = open_targets(targets)?;
    write_targets(targets, &mut sinks, records)
}

/// Render `records` to bytes in `format`. The streaming formats append per-file blocks;
/// the report formats serialize the pre-built `entries` (always `Some` when a report
/// target is present, see [`write_targets`]).
fn render_target(
    format: OutputFormat,
    records: &[FileRecord],
    entries: Option<&[ReportEntry]>,
) -> Vec<u8> {
    match format {
        OutputFormat::Standard => render_streaming(records, append_standard),
        OutputFormat::Colored => render_streaming(records, append_colored),
        OutputFormat::Github => render_streaming(records, append_github),
        OutputFormat::Parsable => render_streaming(records, append_parsable),
        OutputFormat::Junit => render_junit(entries.expect(REPORT_ENTRIES_BUILT)),
        OutputFormat::Gitlab => render_gitlab(entries.expect(REPORT_ENTRIES_BUILT)),
    }
}

// Report entries are built whenever any target uses a report format, so the Junit/Gitlab
// arms of `render_target` only ever see `Some`; the `expect` documents that invariant
// rather than leaving an uncovered `None` arm.
const REPORT_ENTRIES_BUILT: &str =
    "report entries are built when a report target is present";

/// Append every record's per-file block to an owned buffer using `append`, skipping clean
/// files (no error, no kept diagnostics) — the streaming formats print nothing for those.
/// A processing-error record contributes its (already-sanitized) message line.
fn render_streaming(
    records: &[FileRecord],
    append: fn(&mut Vec<u8>, &Path, &[LintProblem]),
) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    for record in records {
        if let Some(message) = &record.error {
            writeln!(out, "{message}").expect(OUTPUT_INFALLIBLE);
        } else if !record.kept.is_empty() {
            append(&mut out, record.path, &record.kept);
        }
    }
    out
}

/// Convert every record (clean files included) into a [`ReportEntry`] with a project-root
/// relative display path. Clean files become passing `JUnit` testcases / are omitted by
/// `GitLab`; the report emitters decide.
fn build_entries(records: &[FileRecord], project_root: &Path) -> Vec<ReportEntry> {
    records
        .iter()
        .map(|record| ReportEntry {
            path: report_display_path(record.path, project_root),
            problems: record.kept.clone(),
            error: record.error.clone(),
        })
        .collect()
}

/// The `--output-file` paths among `targets` (a stdout/stderr destination has none).
fn output_file_paths(targets: &[OutputTarget]) -> impl Iterator<Item = &Path> {
    targets
        .iter()
        .filter_map(|target| match &target.destination {
            Destination::File(path) => Some(path.as_path()),
            Destination::Stdout | Destination::Stderr => None,
        })
}

/// File-independent guards on the resolved targets: `--diff` cannot pair with a report
/// format, and at most one target may write to each console stream (two would interleave).
/// The same-file collision guard needs the files opened (so symlinked parents resolve), so
/// it lives in [`open_targets`] rather than here.
///
/// # Errors
///
/// Returns a usage error for any of the conflicts above.
fn validate_targets(targets: &[OutputTarget], diff: bool) -> Result<(), String> {
    if diff {
        // `--diff` emits only its own unified diff and ignores output formatting, so no
        // target is ever rendered on the diff path: the stream-uniqueness and file-collision
        // checks do not apply, and the sole conflict is an explicit report format.
        return reject_diff_report_conflict(targets);
    }
    reject_duplicate_streams(targets)
}

/// `--diff` with a whole-document report format is a usage error: the report would never be
/// produced (the diff path emits only its patch), so the combination signals confusion.
fn reject_diff_report_conflict(targets: &[OutputTarget]) -> Result<(), String> {
    if targets.iter().any(|target| !target.format.is_streaming()) {
        return Err(
            "error: `--diff` cannot be combined with `--format junit` or `--format gitlab`"
                .to_string(),
        );
    }
    Ok(())
}

/// At most one target may write to stdout (`-`) and one to stderr; two whole-document
/// reports interleaved on one stream produce an unparsable artifact.
fn reject_duplicate_streams(targets: &[OutputTarget]) -> Result<(), String> {
    let (mut stdout, mut stderr) = (false, false);
    for target in targets {
        match target.destination {
            Destination::Stdout if stdout => {
                return Err(
                    "error: at most one output may go to stdout (`-`); give the others a \
                     file destination"
                        .to_string(),
                );
            }
            Destination::Stderr if stderr => {
                return Err(
                    "error: at most one output may go to the console (stderr); give the \
                     others a file destination"
                        .to_string(),
                );
            }
            Destination::Stdout => stdout = true,
            Destination::Stderr => stderr = true,
            Destination::File(_) => {}
        }
    }
    Ok(())
}

/// A path's identity for collision checks: its lexical absolute form (which also covers a
/// not-yet-created destination) plus, for a path that already exists, its underlying file
/// identity via `same_file::Handle`. The shared comparison behind both the output-output
/// and the output-input collision guards.
struct PathIdentity<'a> {
    display: &'a Path,
    abs: PathBuf,
    handle: Option<Handle>,
}

impl<'a> PathIdentity<'a> {
    fn of(path: &'a Path) -> Self {
        Self {
            display: path,
            abs: lexical_abspath(path),
            handle: Handle::from_path(path).ok(),
        }
    }

    /// Whether `self` and `other` resolve to the same file: lexically equal, or — when
    /// `self` exists — the same underlying file (so a symlink or hard link is caught).
    fn same_file(&self, other: &PathIdentity) -> bool {
        self.abs == other.abs || (self.handle.is_some() && self.handle == other.handle)
    }
}

/// No two `--output-file` targets may resolve to the same file: the second's write would
/// clobber the first's report. Called from [`open_targets`] *after* the destinations are
/// opened, so each exists and `same_file::Handle` resolves it through an existing
/// symlink/hard link or an aliased parent directory (a lexical comparison alone would miss
/// two distinct paths pointing at one file). A destination whose existing file is not
/// readable (mode without read permission) cannot be identity-checked and is matched only
/// lexically — an adversarial, non-real-world case for a report path.
fn reject_colliding_output_files(targets: &[OutputTarget]) -> Result<(), String> {
    let outputs: Vec<PathIdentity> =
        output_file_paths(targets).map(PathIdentity::of).collect();
    for (index, output) in outputs.iter().enumerate() {
        if outputs[index + 1..]
            .iter()
            .any(|other| output.same_file(other))
        {
            return Err(format!(
                "error: output path {} is written by more than one format",
                sanitize_control(&output.display.display().to_string())
            ));
        }
    }
    Ok(())
}

/// Refuse any `--output-file` target whose path matches a linted input, so a report can
/// never truncate the source it just linted (or, with `--fix`, the freshly-fixed file).
/// Uses the same lexical + file-identity match as [`reject_colliding_output_files`], so an
/// `-o` that is a symlink *or* a hard link onto an input is caught. The output identities
/// are computed once and reused across all inputs.
///
/// # Errors
///
/// Returns a usage error when an output path resolves to one of the `inputs` (for stdin,
/// the single `--stdin-filename`/label entry).
fn reject_input_collisions<'a>(
    targets: &[OutputTarget],
    inputs: impl Iterator<Item = &'a Path>,
) -> Result<(), String> {
    let outputs: Vec<PathIdentity> =
        output_file_paths(targets).map(PathIdentity::of).collect();
    if outputs.is_empty() {
        return Ok(());
    }
    for input in inputs {
        let input = PathIdentity::of(input);
        for output in &outputs {
            if output.same_file(&input) {
                return Err(format!(
                    "error: output file {} is also a linted input; refusing to overwrite it",
                    sanitize_control(&output.display.display().to_string())
                ));
            }
        }
    }
    Ok(())
}

/// The project root that report paths are made relative to: `CI_PROJECT_DIR` when set
/// (matching ruff's GitLab integration), otherwise `.` (which `lexical_abspath` resolves
/// to the working directory). Computed once per run, not per file.
fn report_project_root() -> PathBuf {
    // An empty `CI_PROJECT_DIR` (set but blank, e.g. a misconfigured CI) is treated as
    // unset: an empty path would panic `lexical_abspath`, and `.` resolves to the cwd.
    std::env::var_os("CI_PROJECT_DIR")
        .filter(|dir| !dir.is_empty())
        .map_or_else(|| PathBuf::from("."), PathBuf::from)
}

fn write_output_error(err: &std::io::Error) -> String {
    format!(
        "error: failed to write output: {}",
        sanitize_control(&err.to_string())
    )
}

fn github_env_active() -> bool {
    std::env::var_os("GITHUB_ACTIONS").is_some()
        && std::env::var_os("GITHUB_WORKFLOW").is_some()
}

fn supports_color() -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    if std::env::var_os("FORCE_COLOR").is_some() {
        return true;
    }
    std::io::stderr().is_terminal()
}

fn main() -> ExitCode {
    // `get_matches` parses (handling `--help`/`--version`/usage errors by exiting) and
    // keeps the `ArgMatches` so `resolve_cli_targets` can recover the CLI order of the
    // repeatable `--format`/`--output-file` pairs via `indices_of`; `from_arg_matches`
    // then builds the typed `Cli` from those same matches (infallible here).
    let matches = Cli::command().get_matches();
    let cli =
        Cli::from_arg_matches(&matches).expect("Cli parses from its own ArgMatches");

    match run_cli(&cli, &matches) {
        Ok(code) => code,
        Err(err) => {
            // Usage/config errors embed user-controlled paths and config values;
            // sanitize so a crafted filename or value cannot inject control
            // sequences or a CI workflow command.
            eprintln!("{}", sanitize_control(&err));
            ExitCode::from(2)
        }
    }
}

fn run_cli(cli: &Cli, matches: &ArgMatches) -> Result<ExitCode, String> {
    #[cfg(feature = "lsp")]
    if matches!(cli.command, Some(Commands::Server)) {
        ryl::lsp::run();
        return Ok(ExitCode::SUCCESS);
    }

    // A pure meta-action that ignores every other input/flag, like the
    // schema-print flags below; clap_complete derives the script from `Cli`.
    if let Some(shell) = cli.generate_completions {
        clap_complete::generate(
            shell,
            &mut Cli::command(),
            "ryl",
            &mut std::io::stdout(),
        );
        return Ok(ExitCode::SUCCESS);
    }

    if cli.print_toml_config_schema {
        println!("{}", schema_string_pretty());
        return Ok(ExitCode::SUCCESS);
    }

    if cli.print_yaml_config_schema {
        println!("{}", yaml_schema_string_pretty());
        return Ok(ExitCode::SUCCESS);
    }

    if cli.migrate_configs || cli.migrate_user_config {
        return run_migration(cli);
    }

    // Output targets are resolved on the lint path (the meta-actions above ignore
    // formatting), inside `run_lint`/`run_stdin_lint` where the run's config — and thus a
    // TOML `[output]` fallback — is known. `matches` carries the CLI arg indices needed to
    // order-pair `--format` with `--output-file`.
    run_lint(cli, matches)
}

fn run_lint(cli: &Cli, matches: &ArgMatches) -> Result<ExitCode, String> {
    let stdin_input = Path::new("-");
    let has_stdin = cli.inputs.iter().any(|p| p.as_path() == stdin_input);
    if has_stdin {
        if cli.inputs.len() > 1 {
            return Err(
                "error: `-` (stdin) cannot be combined with other inputs".to_string()
            );
        }
        if cli.lint.fix.fix {
            return Err(
                "error: `--fix` is not supported when reading from stdin".to_string()
            );
        }
        return run_stdin_lint(cli, matches);
    }

    if cli.stdin_filename.is_some() {
        return Err(
            "error: `--stdin-filename` only applies when reading from stdin (`-`)"
                .to_string(),
        );
    }

    if cli.inputs.is_empty() {
        return Err(
            "error: expected one or more paths (files and/or directories), or `-` for stdin"
                .to_string(),
        );
    }

    // Build a global config if -d/-c provided or env var set; else None for per-file discovery.
    let mut global_cfg = build_global_cfg(&cli.inputs, cli)?;
    if cli.lint.markdown
        && let Some(ctx) = global_cfg.as_mut()
    {
        // Enable markdown once here so per-file clones inherit the built matcher.
        ctx.config.enable_default_markdown(&ctx.base_dir);
    }
    if let Some(cfg) = &global_cfg {
        for notice in &cfg.notices {
            eprintln!("{}", sanitize_control(notice));
        }
    }
    let inputs = &cli.inputs;

    // Determine files to parse from mixed inputs.
    // - Directories: recursively gather only .yml/.yaml
    // - Files: include as-is (even if extension isn't yaml)
    let (candidates, explicit_files) = gather_inputs(inputs);

    // Filter directory candidates via ignores, respecting global vs per-file behavior.
    let mut cache: HashMap<PathBuf, (PathBuf, YamlLintConfig, bool)> = HashMap::new();
    let mut emitted_notices: HashSet<String> = HashSet::new();
    let mut files: Vec<(PathBuf, PathBuf, YamlLintConfig, SourceKind)> = Vec::new();
    let ruleless_config_found = gather_lint_files(
        &candidates,
        &explicit_files,
        global_cfg.as_ref(),
        cli.lint.markdown,
        &mut cache,
        &mut emitted_notices,
        &mut files,
    )?;

    if cli.lint.compatibility.list_files {
        for (path, ..) in &files {
            println!("{}", sanitize_control(&path.display().to_string()));
        }
        return Ok(ExitCode::SUCCESS);
    }

    // Resolve output targets now that the run's config is known: a TOML `[output]` table
    // is read from the global config (`-c`/`-d`/env) or the project config governing the
    // inputs, so `ryl .` honors a project's `.ryl.toml [output]`. A CLI `--format`
    // overrides it. (`list_files`/`migrate` above are exempt, like the no-rules check.)
    // `--diff` previews fixes and has its own unified-diff output; it ignores output
    // formatting, so a config `[output]` report target must not block it. Only an
    // explicit CLI `--format junit|gitlab` conflicts with `--diff` (caught by
    // `validate_targets` via the CLI-derived targets, which are read regardless).
    let output_config = if cli.lint.fix.diff {
        None
    } else {
        run_output_config(global_cfg.as_ref(), &cli.inputs, cli)?
    };
    let targets = resolve_targets(matches, cli, output_config.as_ref())?;
    validate_targets(&targets, cli.lint.fix.diff)?;
    let targets = &targets;

    // Refuse an --output-file that resolves to a linted input: writing the report there
    // would truncate the source we just linted (and, with --fix, the freshly-fixed file).
    reject_input_collisions(targets, files.iter().map(|(path, ..)| path.as_path()))?;

    if files.is_empty() {
        // A clean or fully-ignored project still gets a valid empty report per target, so
        // CI artifact ingestion sees `[]` / `<testsuites .../>` rather than a missing file.
        emit_targets(targets, &[])?;
        return Ok(ExitCode::SUCCESS);
    }

    if let Some(config_found) = ruleless_config_found {
        return Err(no_rules_error(config_found));
    }

    if cli.lint.fix.diff {
        return Ok(emit_diff(&diff_safe_fixes_for_files(&files)?));
    }

    lint_and_exit(&files, cli, targets)
}

/// Open every output destination (before `--fix` mutates anything, so an unopenable
/// `--output-file` fails fast), apply fixes if requested, lint, render each target's
/// format, write it, print the `--fix` summary, and map the tally to an exit code.
///
/// # Errors
///
/// Returns an error if a file cannot be read/written or an output destination fails.
fn lint_and_exit(
    files: &[(PathBuf, PathBuf, YamlLintConfig, SourceKind)],
    cli: &Cli,
    targets: &[OutputTarget],
) -> Result<ExitCode, String> {
    let mut sinks = open_targets(targets)?;

    let initial_problem_count = if cli.lint.fix.fix {
        apply_fixes_reporting_skips(files, cli.lint.compatibility.no_warnings)?
    } else {
        0
    };

    let results = lint_files(files);
    let (summary, records) =
        collect_records(files, results, cli.lint.compatibility.no_warnings);
    write_targets(targets, &mut sinks, &records)?;

    if cli.lint.fix.fix && initial_problem_count > 0 {
        eprintln!(
            "Found {} {} ({} fixed, {} remaining).",
            initial_problem_count,
            pluralize("problem", initial_problem_count),
            initial_problem_count.saturating_sub(summary.problem_count),
            summary.problem_count
        );
    }

    Ok(summary_to_exit(&summary, cli.lint.compatibility.strict))
}

/// Run the initial lint, apply safe fixes in place, and report any files skipped
/// because they do not parse. Returns the pre-fix problem count for the fix summary.
///
/// # Errors
///
/// Returns an error if any file cannot be read or written.
fn apply_fixes_reporting_skips(
    files: &[(PathBuf, PathBuf, YamlLintConfig, SourceKind)],
    no_warnings: bool,
) -> Result<usize, String> {
    let initial_problem_count =
        count_reported_problems(&lint_files(files), no_warnings);
    let fix_stats = apply_safe_fixes_to_files(files)?;
    for (path, problem) in &fix_stats.skipped {
        eprint_skip_notice(path, problem, "--fix");
    }
    Ok(initial_problem_count)
}

fn summary_to_exit(summary: &LintSummary, strict: bool) -> ExitCode {
    if summary.has_error {
        ExitCode::from(1)
    } else if summary.has_warning && strict {
        ExitCode::from(2)
    } else {
        ExitCode::SUCCESS
    }
}

/// Stderr notice for a file `--fix`/`--diff` left untouched (it does not parse, is a
/// symlink, or is non-UTF-8): `<path>:L:C skipped by <action>: <message>`. Both path and
/// message are user-controlled, so both are sanitized; `action` is the literal flag name.
fn eprint_skip_notice(path: &Path, problem: &LintProblem, action: &str) {
    eprintln!(
        "{}:{}:{} skipped by {action}: {}",
        sanitize_control(&path.display().to_string()),
        problem.line,
        problem.column,
        sanitize_control(&problem.message),
    );
}

fn run_stdin_lint(cli: &Cli, matches: &ArgMatches) -> Result<ExitCode, String> {
    let (path, base_dir, cfg, apply_yaml_files, config_found) = resolve_stdin_ctx(cli)?;

    // The stdin config's `[output]` defines the run targets (a CLI `--format` overrides).
    // As in `run_lint`, `--diff` ignores config `[output]` so a config report can't block
    // it; an explicit CLI `--format junit|gitlab` still conflicts via the CLI targets.
    let config_output = if cli.lint.fix.diff {
        None
    } else {
        cfg.output()
    };
    let targets = resolve_targets(matches, cli, config_output)?;
    validate_targets(&targets, cli.lint.fix.diff)?;
    let targets = &targets;

    // The stdin content is labelled by `--stdin-filename`; refuse writing the report to
    // that same path so it cannot truncate the file the label names.
    reject_input_collisions(targets, std::iter::once(path.as_path()))?;

    let Some(kind) = resolve_stdin_kind(cli, &cfg, &path, &base_dir, apply_yaml_files)?
    else {
        // An ignored stdin filename is an empty input set: still emit a valid empty
        // report per target so CI artifact ingestion does not see a missing file.
        emit_targets(targets, &[])?;
        return Ok(ExitCode::SUCCESS);
    };

    if cli.lint.compatibility.list_files {
        println!("{}", sanitize_control(&path.display().to_string()));
        return Ok(ExitCode::SUCCESS);
    }

    if !cfg.enables_any_rule() {
        return Err(no_rules_error(config_found));
    }

    if cli.lint.fix.diff {
        return run_stdin_diff(&path, &base_dir, &cfg, kind);
    }

    let outcome = read_and_lint_stdin(&path, &base_dir, &cfg, kind);

    let files = vec![(path, base_dir, cfg, kind)];
    let results = vec![(0usize, outcome)];

    let mut sinks = open_targets(targets)?;
    let (summary, records) =
        collect_records(&files, results, cli.lint.compatibility.no_warnings);
    write_targets(targets, &mut sinks, &records)?;
    Ok(summary_to_exit(&summary, cli.lint.compatibility.strict))
}

/// Resolve the source kind for stdin, or `None` to skip an ignored `--stdin-filename`.
/// `--markdown` forces Markdown; otherwise, with `--stdin-filename` the kind comes
/// from `[files]` globs (a named file matching no kind is an error, like an
/// explicitly-passed file), and without one the input is linted as YAML.
fn resolve_stdin_kind(
    cli: &Cli,
    cfg: &YamlLintConfig,
    path: &Path,
    base_dir: &Path,
    apply_yaml_files: bool,
) -> Result<Option<SourceKind>, String> {
    if apply_yaml_files && cfg.is_file_ignored(path, base_dir) {
        return Ok(None);
    }
    if cli.lint.markdown {
        return Ok(Some(SourceKind::Markdown));
    }
    if !apply_yaml_files {
        return Ok(Some(SourceKind::Yaml));
    }
    match cfg.source_kind(path, base_dir)? {
        Some(kind) => Ok(Some(kind)),
        // A named stdin file that matches no kind is an error, the same as an
        // explicitly-passed file (see `gather_lint_files`).
        None => Err(format!(
            "{}: no source kind matches; add a matching glob under \
             [files].yaml or [files].markdown",
            path.display()
        )),
    }
}

/// Read and decode stdin. The bool is whether the bytes were plain UTF-8 (no BOM, no
/// transcode), i.e. whether a textual `--diff` of the decoded content would apply back
/// to the original bytes; `read_and_lint_stdin` ignores it.
fn read_stdin_decoded(path: &Path) -> Result<(String, bool), String> {
    let mut buf = Vec::new();
    std::io::stdin()
        .read_to_end(&mut buf)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    let content = decoder::decode_bytes(&buf)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    let plain_utf8 = content.as_bytes() == buf.as_slice();
    Ok((content, plain_utf8))
}

fn read_and_lint_stdin(
    path: &Path,
    base_dir: &Path,
    cfg: &YamlLintConfig,
    kind: SourceKind,
) -> Result<Vec<LintProblem>, String> {
    let (content, _) = read_stdin_decoded(path)?;
    Ok(match kind {
        SourceKind::Markdown => lint_markdown_str(&content, path, cfg, base_dir),
        SourceKind::Yaml => lint_str(&content, path, cfg, base_dir),
    })
}

fn run_stdin_diff(
    path: &Path,
    base_dir: &Path,
    cfg: &YamlLintConfig,
    kind: SourceKind,
) -> Result<ExitCode, String> {
    let (content, plain_utf8) = read_stdin_decoded(path)?;
    let mut stats = DiffStats::default();
    if plain_utf8 {
        stats.record(path, diff_outcome(&content, cfg, path, base_dir, kind));
    } else {
        // Same reason as the file path: the decoded-UTF-8 diff would not apply to the
        // BOM'd/transcoded source, so skip rather than emit a patch that won't apply.
        stats
            .skipped
            .push((path.to_path_buf(), ryl::fix::non_utf8_diff_skip()));
    }
    Ok(emit_diff(&stats))
}

/// Write per-file unified diffs to stdout and parse-skip notices to stderr, returning
/// the `--diff` exit code: `1` if any file would change, else `0`. Mirrors
/// `ruff check --diff` — only the diff drives the exit code; remaining (unfixable)
/// diagnostics are neither printed nor counted here.
fn emit_diff(stats: &DiffStats) -> ExitCode {
    for (path, problem) in &stats.skipped {
        eprint_skip_notice(path, problem, "--diff");
    }
    // Each diff already carries the trailing newline similar emits, so concatenating
    // and printing once keeps stdout a clean sequence of `--- / +++ / @@` blocks.
    let mut rendered = String::new();
    for diff in &stats.diffs {
        rendered.push_str(diff);
    }
    print!("{rendered}");
    if stats.diffs.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn resolve_stdin_ctx(
    cli: &Cli,
) -> Result<(PathBuf, PathBuf, YamlLintConfig, bool, bool), String> {
    let (path, apply_yaml_files) = match cli.stdin_filename.clone() {
        Some(name) => (name, true),
        None => (PathBuf::from(STDIN_LABEL), false),
    };
    let anchor = if apply_yaml_files {
        path.clone()
    } else {
        PathBuf::from(".")
    };
    let ctx = discover_config(std::slice::from_ref(&anchor), &cli_overrides(cli))?;
    for notice in &ctx.notices {
        eprintln!("{}", sanitize_control(notice.as_str()));
    }
    let mut cfg = ctx.config;
    if cli.lint.markdown {
        cfg.enable_default_markdown(&ctx.base_dir);
    }
    if !apply_yaml_files {
        cfg.disable_path_based_rule_ignores();
    }
    Ok((path, ctx.base_dir, cfg, apply_yaml_files, ctx.config_found))
}

fn lint_files(
    files: &[(PathBuf, PathBuf, YamlLintConfig, SourceKind)],
) -> Vec<(usize, Result<Vec<LintProblem>, String>)> {
    let mut results: Vec<(usize, Result<Vec<LintProblem>, String>)> = files
        .par_iter()
        .enumerate()
        .map(|(idx, (path, base_dir, cfg, kind))| {
            let result = match kind {
                SourceKind::Markdown => lint_markdown_file(path, cfg, base_dir),
                SourceKind::Yaml => lint_file(path, cfg, base_dir),
            };
            (idx, result)
        })
        .collect();
    results.sort_by_key(|(idx, _)| *idx);
    results
}

#[allow(clippy::too_many_arguments)]
fn gather_lint_files(
    candidates: &[PathBuf],
    explicit_files: &[PathBuf],
    global_cfg: Option<&ConfigContext>,
    markdown: bool,
    cache: &mut HashMap<PathBuf, (PathBuf, YamlLintConfig, bool)>,
    emitted_notices: &mut HashSet<String>,
    files: &mut Vec<(PathBuf, PathBuf, YamlLintConfig, SourceKind)>,
) -> Result<Option<bool>, String> {
    // `config_found` of the first selected file that enables no rules, so a run that
    // would lint nothing reports the right message *for that file* — "no config
    // found" vs "config enables no rules" — even when other files did find a config.
    // `None` means every selected file enables at least one rule.
    let mut ruleless_config_found = None;
    // De-duplicate selected files across the walk and explicit args; without this a
    // file listed twice is linted twice (and `--diff` would emit a duplicate patch
    // block that fails to apply on the second copy). Unlike yamllint, which keeps
    // duplicates.
    let mut seen: HashSet<PathBuf> = HashSet::new();
    // Directory candidates first (a file matching no source kind is silently skipped),
    // then explicit args (a no-kind file is a hard error, since the user named it);
    // `explicit` selects which. `seen` spans both, so a file reached via the walk and
    // also named on the command line is linted exactly once, in walk order.
    let tagged = candidates
        .iter()
        .map(|path| (path, false))
        .chain(explicit_files.iter().map(|path| (path, true)));
    for (path, explicit) in tagged {
        let (base_dir, cfg, notices, found) =
            resolve_ctx(path, global_cfg, markdown, cache)?;
        for notice in notices {
            if emitted_notices.insert(notice.clone()) {
                eprintln!("{}", sanitize_control(notice.as_str()));
            }
        }
        if cfg.is_file_ignored(path, &base_dir) {
            continue;
        }
        let kind = match cfg.source_kind(path, &base_dir)? {
            Some(kind) => kind,
            None if explicit => {
                return Err(format!(
                    "{}: no source kind matches; add a matching glob under \
                     [files].yaml or [files].markdown",
                    path.display()
                ));
            }
            None => continue,
        };
        if !seen.insert(lexical_abspath(path)) {
            continue;
        }
        if !cfg.enables_any_rule() && ruleless_config_found.is_none() {
            ruleless_config_found = Some(found);
        }
        files.push((path.clone(), base_dir, cfg, kind));
    }

    Ok(ruleless_config_found)
}

/// One linted file's filtered outcome, format-agnostic: a borrowed source path plus
/// either its kept diagnostics or a single processing-error message (mutually exclusive,
/// a clean file has neither). Each output target renders the same records its own way
/// ([`render_target`]), so the filter+tally pass below runs exactly once regardless of how
/// many targets a run has.
struct FileRecord<'a> {
    path: &'a Path,
    kept: Vec<LintProblem>,
    error: Option<String>,
}

/// Filter and tally every lint result into format-agnostic [`FileRecord`]s in file order.
/// The returned [`LintSummary`] (and thus the exit code) is independent of which formats
/// the records are later rendered to. `no_warnings` drops warning-level diagnostics before
/// they are kept or counted, exactly as the streaming path used to.
fn collect_records<'a>(
    files: &'a [(PathBuf, PathBuf, YamlLintConfig, SourceKind)],
    results: Vec<(usize, Result<Vec<LintProblem>, String>)>,
    no_warnings: bool,
) -> (LintSummary, Vec<FileRecord<'a>>) {
    let mut summary = LintSummary::default();
    let mut records: Vec<FileRecord<'a>> = Vec::with_capacity(results.len());

    for (idx, outcome) in results {
        let (path, ..) = &files[idx];
        match outcome {
            Err(message) => {
                // Error messages embed user-controlled paths/values; sanitize so a
                // crafted filename cannot inject terminal escapes or (via a newline)
                // a GitHub workflow command. `sanitize_control` is safe for every
                // format because it neutralises the newline that injection needs.
                let message = sanitize_control(&message).into_owned();
                summary.has_error = true;
                summary.problem_count += 1;
                records.push(FileRecord {
                    path,
                    kept: Vec::new(),
                    error: Some(message),
                });
            }
            Ok(diagnostics) => {
                let mut kept: Vec<LintProblem> = Vec::new();
                for problem in diagnostics {
                    if no_warnings && problem.level == Severity::Warning {
                        continue;
                    }
                    match problem.level {
                        Severity::Error => summary.has_error = true,
                        Severity::Warning => summary.has_warning = true,
                    }
                    summary.problem_count += 1;
                    kept.push(problem);
                }
                records.push(FileRecord {
                    path,
                    kept,
                    error: None,
                });
            }
        }
    }

    (summary, records)
}

#[derive(Default)]
struct LintSummary {
    has_error: bool,
    has_warning: bool,
    problem_count: usize,
}

fn count_reported_problems(
    results: &[(usize, Result<Vec<LintProblem>, String>)],
    no_warnings: bool,
) -> usize {
    results
        .iter()
        .map(|(_, outcome)| match outcome {
            Err(_) => 1,
            Ok(diagnostics) => diagnostics
                .iter()
                .filter(|problem| !(no_warnings && problem.level == Severity::Warning))
                .count(),
        })
        .sum()
}

fn pluralize(singular: &str, count: usize) -> &str {
    if count == 1 { singular } else { "problems" }
}

// The streaming formats append a per-file block to the in-memory output buffer. Each is
// only reached with a non-empty `problems` slice (the caller skips clean files), and each
// write is infallible (see `OUTPUT_INFALLIBLE`).

/// Shared shape of the standard/colored formats: a file `header` line, one `format_line`
/// per diagnostic, then a trailing blank line.
fn append_grouped(
    out: &mut Vec<u8>,
    header: &str,
    problems: &[LintProblem],
    format_line: fn(&LintProblem) -> String,
) {
    writeln!(out, "{header}").expect(OUTPUT_INFALLIBLE);
    for problem in problems {
        writeln!(out, "{}", format_line(problem)).expect(OUTPUT_INFALLIBLE);
    }
    writeln!(out).expect(OUTPUT_INFALLIBLE);
}

fn append_standard(out: &mut Vec<u8>, path: &Path, problems: &[LintProblem]) {
    let display = path.display().to_string();
    let header = sanitize_control(&display);
    append_grouped(out, &header, problems, format_standard);
}

fn append_colored(out: &mut Vec<u8>, path: &Path, problems: &[LintProblem]) {
    let header = format!(
        "\u{001b}[4m{}\u{001b}[0m",
        sanitize_control(&path.display().to_string())
    );
    append_grouped(out, &header, problems, format_colored);
}

fn append_github(out: &mut Vec<u8>, path: &Path, problems: &[LintProblem]) {
    let path_str = path.display().to_string();
    writeln!(out, "::group::{}", github_escape(&path_str, false))
        .expect(OUTPUT_INFALLIBLE);
    let escaped_file = github_escape(&path_str, true);
    for problem in problems {
        writeln!(out, "{}", format_github(problem, &escaped_file))
            .expect(OUTPUT_INFALLIBLE);
    }
    writeln!(out, "::endgroup::").expect(OUTPUT_INFALLIBLE);
    writeln!(out).expect(OUTPUT_INFALLIBLE);
}

fn append_parsable(out: &mut Vec<u8>, path: &Path, problems: &[LintProblem]) {
    let sanitized_path = sanitize_control(&path.display().to_string()).into_owned();
    for problem in problems {
        writeln!(out, "{}", format_parsable(problem, &sanitized_path))
            .expect(OUTPUT_INFALLIBLE);
    }
}

fn format_standard(problem: &LintProblem) -> String {
    let mut line = format!("  {}:{}", problem.line, problem.column);
    line.push_str(&" ".repeat(12usize.saturating_sub(line.len())));
    line.push_str(problem.level.as_str());
    line.push_str(&" ".repeat(21usize.saturating_sub(line.len())));
    line.push_str(&sanitize_control(&problem.message));
    if let Some(rule) = problem.rule {
        line.push_str("  (");
        line.push_str(rule);
        line.push(')');
    }
    line
}

fn format_colored(problem: &LintProblem) -> String {
    let mut line = format!(
        "  \u{001b}[2m{}:{}\u{001b}[0m",
        problem.line, problem.column
    );
    line.push_str(&" ".repeat(20usize.saturating_sub(line.len())));
    let level_str = match problem.level {
        Severity::Warning => "\u{001b}[33mwarning\u{001b}[0m",
        Severity::Error => "\u{001b}[31merror\u{001b}[0m",
    };
    line.push_str(level_str);
    line.push_str(&" ".repeat(38usize.saturating_sub(line.len())));
    line.push_str(&sanitize_control(&problem.message));
    if let Some(rule) = problem.rule {
        line.push_str("  \u{001b}[2m(");
        line.push_str(rule);
        line.push_str(")\u{001b}[0m");
    }
    line
}

/// `escaped_file` is the `file=` property value, escaped once per file by the
/// caller (it is identical for every diagnostic in the same file).
fn format_github(problem: &LintProblem, escaped_file: &str) -> String {
    let mut line = format!(
        "::{} file={escaped_file},line={},col={}::{}:{} ",
        problem.level.as_str(),
        problem.line,
        problem.column,
        problem.line,
        problem.column
    );
    if let Some(rule) = problem.rule {
        line.push('[');
        line.push_str(rule);
        line.push_str("] ");
    }
    line.push_str(&github_escape(&problem.message, false));
    line
}

/// `sanitized_path` is sanitized once per file by the caller (identical for every
/// diagnostic in the file).
fn format_parsable(problem: &LintProblem, sanitized_path: &str) -> String {
    let mut line = format!(
        "{sanitized_path}:{}:{}: [{}] {}",
        problem.line,
        problem.column,
        problem.level.as_str(),
        sanitize_control(&problem.message)
    );
    if let Some(rule) = problem.rule {
        line.push_str(" (");
        line.push_str(rule);
        line.push(')');
    }
    line
}
