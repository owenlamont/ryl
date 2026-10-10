#![forbid(unsafe_code)]
#![deny(
    clippy::all,
    clippy::pedantic,
    clippy::cargo,
    clippy::cognitive_complexity
)]

use std::collections::HashSet;
use std::fs::File;
use std::io::{BufWriter, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use clap::{ArgMatches, CommandFactory, FromArgMatches, Parser, ValueEnum};
use ignore::WalkBuilder;
use rayon::prelude::*;
use ryl::cli_support::{
    CliConfigFlags, ConfigCache, LintFile, ResolvedConfig, github_escape,
    lexical_abspath, report_display_path, resolve_ctx, sanitize_control,
};
use ryl::config::{
    ConfigContext, Overrides, SourceKind, SystemEnv, YamlLintConfig, discover_config,
    is_inline_toml, user_config_migration_paths,
};
use ryl::config_schema::{
    OutputDestination, OutputTable, schema_string_pretty, yaml_schema_string_pretty,
};
use ryl::decoder;
use ryl::fix::{
    DiffStats, Rewrite, SAFE_FIX_RULE_IDS, diff_files, diff_outcome, rewrite_files,
    rewrite_str,
};
use ryl::migrate::{
    MigrateOptions, OutputMode as MigrateOutputMode, SourceCleanup,
    UserConfigMigration, WriteMode, migrate_configs,
};
use ryl::report::{ReportEntry, render_gitlab, render_junit};
use ryl::rules::ALL_RULE_IDS;
use ryl::{
    LintProblem, Severity, lint_file, lint_markdown_file, lint_markdown_str, lint_str,
};
use same_file::Handle;

const STDIN_LABEL: &str = "<stdin>";

// Writes into an owned `Vec<u8>` cannot fail, so they `expect` rather than leave dead `?`
// error arms; only the final write to the destination is fallible.
const OUTPUT_INFALLIBLE: &str =
    "writing diagnostics to an in-memory buffer cannot fail";

const NO_RULES_ENABLED_ERROR: &str = "error: configuration enables no rules, so nothing would be linted; enable at \
     least one rule, copy a preset from https://ryl-docs.pages.dev/config-presets/, or pass '--enable ALL'";

const NO_CONFIG_ERROR: &str = "error: no configuration found and ryl enables no rules by default; create a \
     config that enables rules, copy a preset from https://ryl-docs.pages.dev/config-presets/, or pass '--enable ALL'";

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

fn cli_overrides(args: &LintArgs) -> Overrides {
    Overrides {
        config_file: args.source.config_file.clone(),
        config_data: args.source.config_data.as_ref().map(|raw| {
            if !raw.is_empty() && !raw.contains(':') && !is_inline_toml(raw) {
                format!("extends: {raw}")
            } else {
                raw.clone()
            }
        }),
    }
}

fn build_global_cfg(
    inputs: &[PathBuf],
    args: &LintArgs,
) -> Result<Option<ConfigContext>, String> {
    if args.source.config_data.is_some()
        || args.source.config_file.is_some()
        || std::env::var("YAMLLINT_CONFIG_FILE").is_ok()
    {
        discover_config(inputs, &cli_overrides(args)).map(Some)
    } else {
        Ok(None)
    }
}

/// The run-level TOML `[output]` table: from the global config when one was provided
/// (`-c`/`-d`/env), else from the inputs-anchored project config (so `ryl .` picks up the
/// project's `.ryl.toml [output]`). A run spanning separate projects takes the first project
/// config discovered along the inputs (argument-order sensitive); pass `-c`/`-d` to fix it.
///
/// # Errors
///
/// Propagates a config discovery/parse/validation error. The empty-input case has no per-file
/// discovery, so a malformed `[output]` is surfaced here rather than silently ignored.
fn run_output_config(
    global_cfg: Option<&ConfigContext>,
    inputs: &[PathBuf],
    args: &LintArgs,
) -> Result<Option<OutputTable>, String> {
    if let Some(ctx) = global_cfg {
        return Ok(ctx.config.output().cloned());
    }
    Ok(discover_config(inputs, &cli_overrides(args))?
        .config
        .output()
        .cloned())
}

fn run_migration(cli: &Cli) -> Result<ExitCode, String> {
    if !cli.lint_args.source.inputs.is_empty() {
        return Err(
            "error: migration takes no input paths; use --migrate-root <PATH> to \
             choose where --migrate-configs searches"
                .to_string(),
        );
    }
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
    // surfaces an empty trigger even when the other produced entries. User-global entries
    // are identified by their source paths; the rest are project.
    let user_sources: Vec<PathBuf> =
        user_config.as_ref().map_or_else(Vec::new, |user| {
            let mut sources = user.ryl_config_paths();
            sources.push(user.source.clone());
            sources
        });
    if let Some(root) = &project_root {
        let project_migrated = result
            .entries
            .iter()
            .any(|entry| !is_user_source(&entry.source, &user_sources));
        if !project_migrated {
            println!(
                "No legacy config files migrated under {}",
                sanitize_control(&root.display().to_string())
            );
        }
    }
    if cli.migrate_user_config {
        let user_migrated = result
            .entries
            .iter()
            .any(|entry| is_user_source(&entry.source, &user_sources));
        if !user_migrated {
            println!("No user-global config migrated.");
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
    // The `--migrate-*` sub-flags require at least one migration trigger.
    group(clap::ArgGroup::new("migrate_mode")
        .args(["migrate_configs", "migrate_user_config"])
        .multiple(true))
)]
// `check` and (with `lsp`) `server` are subcommands; bare `ryl <paths>` still lints via the
// flattened top-level lint args, so those are mutually exclusive with any subcommand.
#[command(args_conflicts_with_subcommands = true)]
// These are independent toggles, not state better modeled as an enum.
#[allow(clippy::struct_excessive_bools)]
struct Cli {
    #[command(flatten)]
    lint_args: LintArgs,

    // These print-and-exit meta-actions are `exclusive` so combining them with a lint/fix
    // request is a usage error. `--migrate-configs` is not exclusive: it combines with its
    // `requires`-bound `--migrate-*` sub-flags.
    /// Print the JSON Schema for ryl TOML config and exit
    #[arg(
        long = "print-toml-config-schema",
        default_value_t = false,
        exclusive = true
    )]
    print_toml_config_schema: bool,

    /// Print the JSON Schema for yamllint-compatible YAML config and exit (deprecated)
    #[arg(
        long = "print-yaml-config-schema",
        default_value_t = false,
        exclusive = true
    )]
    print_yaml_config_schema: bool,

    /// Convert legacy YAML configs into .ryl.toml files and move deprecated keys in ryl TOML configs
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
    migrate: MigrateFlags,

    #[command(subcommand)]
    command: Option<Commands>,
}

/// Subcommands; none (bare `ryl <paths>`) lints, like `check`. A bare subcommand token
/// (`check`/`format`/`server`) resolves to the subcommand, so lint a path of that name with
/// `ryl check <name>`.
#[derive(clap::Subcommand, Debug)]
enum Commands {
    /// Lint YAML inputs (the explicit form of bare `ryl <paths>`)
    Check(LintArgs),
    /// Format YAML inputs in place
    Format(FormatArgs),
    /// Run the language server (LSP) over stdio for editor integration
    #[cfg(feature = "lsp")]
    Server,
}

/// The lint pass arguments, flattened both at the top level (bare `ryl <paths>`) and under
/// `ryl check`, so the two forms are byte-for-byte equivalent.
#[derive(clap::Args, Debug, Default)]
struct LintArgs {
    #[command(flatten)]
    source: SourceArgs,

    /// Output format (auto, standard, colored, github, parsable, junit, gitlab). Repeatable:
    /// each `--output-format` may be followed by an `--output-file` to send that format to a
    /// file, so console and report artifacts can be produced together.
    #[arg(
        short = 'f',
        long = "output-format",
        visible_alias = "format",
        value_enum
    )]
    format: Vec<CliFormat>,

    /// Destination for the preceding `--output-format` (a path, or `-` for stdout).
    /// Repeatable; each binds to the most recent `--output-format`. Default stream
    /// otherwise: stderr for the console formats, stdout for junit/gitlab.
    #[arg(
        short = 'o',
        long = "output-file",
        value_name = "FILE",
        conflicts_with = "diff"
    )]
    output_file: Vec<PathBuf>,

    #[command(flatten)]
    lint: LintFlags,
}

/// The inputs and config selection shared by `check` and `format`.
#[derive(clap::Args, Debug, Default, Clone)]
struct SourceArgs {
    /// One or more paths: files and/or directories, or `-` to read from stdin
    #[arg(value_name = "PATH_OR_FILE")]
    inputs: Vec<PathBuf>,

    /// Filename used for diagnostics, config discovery, and yaml-files matching when reading stdin
    #[arg(long = "stdin-filename", value_name = "FILE")]
    stdin_filename: Option<PathBuf>,

    /// Path to configuration file (TOML, or deprecated yamllint YAML)
    #[arg(short = 'c', long = "config-file", value_name = "FILE")]
    config_file: Option<PathBuf>,

    /// Inline configuration data (TOML, or deprecated yamllint YAML)
    #[arg(short = 'd', long = "config-data", value_name = "DATA")]
    config_data: Option<String>,
}

#[derive(clap::Args, Debug)]
#[expect(clippy::struct_excessive_bools, reason = "one field per CLI flag")]
struct FormatArgs {
    #[command(flatten)]
    source: SourceArgs,

    /// Write nothing; exit 1 if any file would be reformatted
    #[arg(long = "check", default_value_t = false, conflicts_with = "diff")]
    check: bool,

    /// Print a unified diff of the formatting changes instead of writing them; exits 1 if any
    /// file would change
    #[arg(long = "diff", default_value_t = false)]
    diff: bool,

    /// Output format for the files `--check` would reformat; overrides `[output]`
    #[arg(long = "output-format", value_enum)]
    output_format: Option<CliFormat>,

    /// Format inputs as Markdown (embedded YAML front matter and fenced yaml/yml
    /// blocks) using default globs, without configuring `[files].markdown`
    #[arg(long = "markdown", default_value_t = false)]
    markdown: bool,

    /// Suppress config warnings, the only warnings `ryl format` emits
    #[arg(long = "no-warnings", default_value_t = false)]
    no_warnings: bool,
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

    /// Lint with exactly these rules (comma-separated ids, or ALL), replacing the
    /// config's selection; works without a config file
    #[arg(
        long = "enable",
        value_name = "RULES",
        value_delimiter = ',',
        value_parser = parse_rule_selector,
        conflicts_with = "list_files"
    )]
    enable: Vec<&'static str>,
}

fn parse_rule_selector(value: &str) -> Result<&'static str, String> {
    std::iter::once("ALL")
        .chain(ALL_RULE_IDS)
        .find(|id| *id == value)
        .ok_or_else(|| format!("no such rule: \"{value}\""))
}

fn cli_config_flags(args: &LintArgs) -> CliConfigFlags {
    let enable = &args.lint.enable;
    CliConfigFlags {
        markdown: args.lint.markdown,
        enable: (!enable.is_empty()).then(|| {
            if enable.contains(&"ALL") {
                ALL_RULE_IDS.to_vec()
            } else {
                enable.clone()
            }
        }),
    }
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
    /// Directory to search for legacy YAML config files, or one config file (default: .)
    #[arg(
        long = "migrate-root",
        value_name = "PATH",
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
    /// Streaming formats emit per diagnostic; junit/gitlab buffer all and serialize once.
    const fn is_streaming(self) -> bool {
        matches!(
            self,
            Self::Standard | Self::Colored | Self::Github | Self::Parsable
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Destination {
    Stdout,
    Stderr,
    File(PathBuf),
}

struct OutputTarget {
    format: OutputFormat,
    destination: Destination,
}

fn default_destination(format: OutputFormat) -> Destination {
    if format.is_streaming() {
        Destination::Stderr
    } else {
        Destination::Stdout
    }
}

/// Resolve `--format`/`--output-file` into targets, pairing each `--output-file` with the
/// most recent `--format` (`-` means stdout), recovering CLI order via clap arg indices.
/// Empty when no `--format` was given, so the caller falls back to config `[output]`.
///
/// # Errors
///
/// Usage error for an `--output-file` with no preceding `--format`, or a second one bound to
/// the same `--format`.
fn resolve_cli_targets(
    matches: &ArgMatches,
    args: &LintArgs,
) -> Result<Vec<OutputTarget>, String> {
    enum Occurrence {
        Format(OutputFormat),
        Output(PathBuf),
    }
    let mut occurrences: Vec<(usize, Occurrence)> = Vec::new();
    if let Some(indices) = matches.indices_of("format") {
        for (index, format) in indices.zip(&args.format) {
            occurrences
                .push((index, Occurrence::Format(detect_output_format(*format))));
        }
    }
    if let Some(indices) = matches.indices_of("output_file") {
        for (index, path) in indices.zip(&args.output_file) {
            occurrences.push((index, Occurrence::Output(path.clone())));
        }
    }
    occurrences.sort_by_key(|(index, _)| *index);

    let mut pending: Vec<(OutputFormat, Option<Destination>)> = Vec::new();
    for (_, occurrence) in occurrences {
        match occurrence {
            Occurrence::Format(format) => pending.push((format, None)),
            Occurrence::Output(path) => {
                let Some((_, destination)) = pending.last_mut() else {
                    return Err("error: --output-file must follow an --output-format"
                        .to_string());
                };
                if destination.is_some() {
                    return Err(
                        "error: an --output-format takes at most one --output-file"
                            .to_string(),
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

/// The run's output targets, precedence CLI > config > default: the CLI `--format` pairs,
/// else the config `[output]` table, else the default auto-console target.
///
/// # Errors
///
/// Propagates a `--format`/`--output-file` pairing error from [`resolve_cli_targets`].
fn resolve_targets(
    matches: &ArgMatches,
    args: &LintArgs,
    config_output: Option<&OutputTable>,
) -> Result<Vec<OutputTarget>, String> {
    let cli_targets = resolve_cli_targets(matches, args)?;
    if !cli_targets.is_empty() {
        return Ok(cli_targets);
    }
    Ok(config_or_default_targets(config_output))
}

fn config_or_default_targets(config_output: Option<&OutputTable>) -> Vec<OutputTarget> {
    if let Some(config_targets) = config_output.map(config_targets_from_table)
        && !config_targets.is_empty()
    {
        return config_targets;
    }
    vec![default_target(CliFormat::Auto)]
}

fn default_target(choice: CliFormat) -> OutputTarget {
    let format = detect_output_format(choice);
    OutputTarget {
        destination: default_destination(format),
        format,
    }
}

/// One target per declared format, in `OutputTable::entries` order (deterministic). Table
/// field names are exactly the `--format` value names (so the `from_str` cannot fail), and
/// `auto` is env-resolved like `--format auto`.
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

/// An opened output destination. A file is opened create+write but **not** truncate, so its
/// contents survive until [`OutputSink::commit`] truncates and rewrites it: a later target
/// failing to open then cannot destroy an existing artifact (a freshly created one may be
/// left empty if a later target aborts; see [`open_destination`]).
enum OutputSink {
    /// stdout or stderr, written and flushed as-is.
    Stream(Box<dyn Write>),
    /// A `--output-file` target, truncated then written at commit time.
    File(File),
}

impl OutputSink {
    /// Write `bytes` as this sink's complete contents (a file is truncated first, clearing
    /// any prior artifact). The only fallible output step (rendering is infallible, see
    /// [`OUTPUT_INFALLIBLE`]); write and flush are chained into one coverable error.
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

/// Open one destination for writing. A file is opened create+write but **not** truncate, so
/// an unopenable `--output-file` fails fast (before `--fix` mutates anything) yet an existing
/// destination survives until [`OutputSink::commit`] rewrites it. A freshly-created
/// destination may be left empty if a later target/collision aborts the run (cleaning it by
/// path would race a concurrent writer); gate CI artifact use on the exit code.
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

/// Open every destination up front (so an unopenable `--output-file` fails before `--fix`
/// mutates any source), then reject two outputs resolving to the same file. The collision
/// check runs after opening so `same_file::Handle` can resolve symlink/hard link/aliased
/// parents.
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

/// Render `records` for each target and write to its sink. Report entries are built once and
/// shared across any report targets.
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

/// Open then render and write `records` to each target. For paths with no `--fix` ordering
/// constraint (empty-input and stdin); the `--fix` path opens early via [`open_targets`].
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

/// Render `records` to bytes in `format`. The report arms serialize the pre-built `entries`,
/// always `Some` when a report target is present (see [`write_targets`]).
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

// A report target means entries are built, so the junit/gitlab arms only see `Some`; the
// `expect` pins that invariant rather than leaving an uncovered `None` arm.
const REPORT_ENTRIES_BUILT: &str =
    "report entries are built when a report target is present";

/// Append each record's per-file block via `append`, skipping clean files; a processing-error
/// record contributes its (already-sanitized) message line.
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
/// relative display path; the report emitters decide how to render a clean file.
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

fn output_file_paths(targets: &[OutputTarget]) -> impl Iterator<Item = &Path> {
    targets
        .iter()
        .filter_map(|target| match &target.destination {
            Destination::File(path) => Some(path.as_path()),
            Destination::Stdout | Destination::Stderr => None,
        })
}

/// File-independent target guards: `--diff` cannot pair with a report format, and at most one
/// target per console stream. The same-file collision guard needs the files opened (so
/// symlinked parents resolve), so it lives in [`open_targets`] instead.
///
/// # Errors
///
/// Returns a usage error for any of the conflicts above.
fn validate_targets(targets: &[OutputTarget], diff: bool) -> Result<(), String> {
    if diff {
        // `--diff` renders no target (it emits only its own diff), so stream-uniqueness and
        // file-collision do not apply: the sole conflict is an explicit report format.
        return reject_diff_report_conflict(targets);
    }
    reject_duplicate_streams(targets)
}

/// `--diff` with a report format is a usage error: the diff path emits only its patch, so the
/// report would never be produced.
fn reject_diff_report_conflict(targets: &[OutputTarget]) -> Result<(), String> {
    if targets.iter().any(|target| !target.format.is_streaming()) {
        return Err(
            "error: `--diff` cannot be combined with `--output-format junit` or `--output-format gitlab`"
                .to_string(),
        );
    }
    Ok(())
}

/// At most one target per stream; two reports interleaved on one stream are unparsable.
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

/// A path's identity for collision checks: its lexical absolute form (covering a
/// not-yet-created destination) plus, when it exists, its file identity via
/// `same_file::Handle`. Shared by the output-output and output-input collision guards.
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

    /// Whether `self` and `other` are lexically equal or (when `self` exists) the same
    /// underlying file, so a symlink or hard link is caught.
    fn same_file(&self, other: &PathIdentity) -> bool {
        self.abs == other.abs || (self.handle.is_some() && self.handle == other.handle)
    }
}

/// No two `--output-file` targets may resolve to the same file (the second would clobber the
/// first). Called from [`open_targets`] after opening, so `same_file::Handle` resolves a
/// shared symlink/hard link/aliased parent that a lexical comparison alone would miss. An
/// existing file without read permission cannot be identity-checked and is matched lexically
/// only: an adversarial, non-real-world case.
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

/// Refuse an `--output-file` matching a linted input, so a report can never truncate the
/// source it just linted (or, with `--fix`, the freshly-fixed file). Uses the same lexical +
/// file-identity match as [`reject_colliding_output_files`], catching a symlink or hard link
/// onto an input.
///
/// # Errors
///
/// Returns a usage error when an output path resolves to one of the `inputs`.
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

/// The project root report paths are made relative to: `CI_PROJECT_DIR` when set (matching
/// ruff), otherwise `.`.
fn report_project_root() -> PathBuf {
    // A blank `CI_PROJECT_DIR` is treated as unset: an empty path would panic
    // `lexical_abspath`, and `.` resolves to the cwd.
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
    // Keep the `ArgMatches` so `resolve_cli_targets` can recover the CLI order of the
    // repeatable `--format`/`--output-file` pairs via `indices_of`; `from_arg_matches` then
    // builds the typed `Cli` from the same matches.
    let matches = Cli::command().get_matches();
    let cli =
        Cli::from_arg_matches(&matches).expect("Cli parses from its own ArgMatches");

    match run_cli(&cli, &matches) {
        Ok(code) => code,
        Err(err) => {
            // Sanitize: errors embed user-controlled paths/values that could otherwise
            // inject control sequences or a CI workflow command.
            eprintln!("{}", sanitize_control(&err));
            ExitCode::from(2)
        }
    }
}

fn run_cli(cli: &Cli, matches: &ArgMatches) -> Result<ExitCode, String> {
    match &cli.command {
        #[cfg(feature = "lsp")]
        Some(Commands::Server) => return Ok(ryl::lsp::run()),
        // `ryl check`: lint via the subcommand's own args/matches (the `--format`/`--output-file`
        // indices `resolve_cli_targets` reads live in this nested `ArgMatches`, not the root's).
        Some(Commands::Check(args)) => {
            let sub = matches
                .subcommand_matches("check")
                .expect("check subcommand matches present");
            return run_lint(args, sub);
        }
        Some(Commands::Format(args)) => return run_format(args),
        None => {}
    }

    // A meta-action that ignores every other input/flag, like the schema-print flags below.
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

    let args = &cli.lint_args;
    if !args.source.inputs.is_empty() && !args.lint.compatibility.no_warnings {
        eprintln!(
            "warning: bare `ryl <paths>` is deprecated; use `ryl check <paths>` instead"
        );
    }
    // Output targets are resolved inside `run_lint`/`run_stdin_lint`, where the run's config
    // (and thus a TOML `[output]` fallback) is known; `matches` carries the arg indices.
    run_lint(args, matches)
}

/// Whether the inputs name stdin (`-`), which must then be the only input.
fn stdin_requested(source: &SourceArgs) -> Result<bool, String> {
    let has_stdin = source.inputs.iter().any(|p| p.as_path() == Path::new("-"));
    if has_stdin && source.inputs.len() > 1 {
        return Err(
            "error: `-` (stdin) cannot be combined with other inputs".to_string()
        );
    }
    Ok(has_stdin)
}

fn run_lint(args: &LintArgs, matches: &ArgMatches) -> Result<ExitCode, String> {
    if stdin_requested(&args.source)? {
        if args.lint.fix.fix {
            return Err(
                "error: `--fix` is not supported when reading from stdin".to_string()
            );
        }
        return run_stdin_lint(args, matches);
    }

    let (global_cfg, files, ruleless_config_found) = collect_files(args)?;

    if args.lint.compatibility.list_files {
        for (path, ..) in &files {
            println!("{}", sanitize_control(&path.display().to_string()));
        }
        return Ok(ExitCode::SUCCESS);
    }

    // `--diff` has its own unified-diff output, so skip config `[output]`: a config report
    // target must not block it. An explicit CLI `--format junit|gitlab` still conflicts via
    // the CLI-derived targets `validate_targets` reads regardless.
    let output_config = if args.lint.fix.diff {
        None
    } else {
        run_output_config(global_cfg.as_ref(), &args.source.inputs, args)?
    };
    let targets = resolve_targets(matches, args, output_config.as_ref())?;
    validate_targets(&targets, args.lint.fix.diff)?;
    let targets = &targets;

    reject_input_collisions(targets, files.iter().map(|(path, ..)| path.as_path()))?;

    if files.is_empty() {
        // Still emit a valid empty report per target, so CI artifact ingestion sees
        // `[]` / `<testsuites .../>` rather than a missing file.
        emit_targets(targets, &[])?;
        return Ok(ExitCode::SUCCESS);
    }

    if let Some(config_found) = ruleless_config_found {
        return Err(no_rules_error(config_found));
    }

    if args.lint.fix.diff {
        return Ok(emit_diff(
            &diff_files(&files, Rewrite::Fix, Preview::Diff.flag())?,
            Preview::Diff,
        ));
    }

    lint_and_exit(&files, args, targets)
}

type CollectedFiles = (Option<ConfigContext>, Vec<LintFile>, Option<bool>);

/// The deduplicated files the (non-stdin) inputs select, with the global config they were
/// resolved under and [`gather_lint_files`]' no-rules marker; config notices are emitted.
///
/// # Errors
///
/// A usage error for `--stdin-filename` without `-` or for no inputs; otherwise propagates
/// a config or source-kind error.
fn collect_files(args: &LintArgs) -> Result<CollectedFiles, String> {
    if args.source.stdin_filename.is_some() {
        return Err(
            "error: `--stdin-filename` only applies when reading from stdin (`-`)"
                .to_string(),
        );
    }

    if args.source.inputs.is_empty() {
        return Err(
            "error: expected one or more paths (files and/or directories), or `-` for stdin"
                .to_string(),
        );
    }

    let flags = cli_config_flags(args);
    let mut global_cfg = build_global_cfg(&args.source.inputs, args)?;
    if let Some(ctx) = global_cfg.as_mut() {
        flags.apply(&mut ctx.config, &ctx.base_dir);
    }
    let mut notices = global_cfg
        .as_ref()
        .map_or_else(Vec::new, |ctx| ctx.notices.clone());
    let inputs = &args.source.inputs;

    let (candidates, explicit_files) = gather_inputs(inputs);

    let global_resolved = global_cfg.as_ref().map(|ctx| {
        (
            ctx.base_dir.clone(),
            Arc::new(ctx.config.clone()),
            ctx.config_found,
        )
    });
    let mut cache = ConfigCache::default();
    let mut files: Vec<LintFile> = Vec::new();
    let ruleless_config_found = gather_lint_files(
        &candidates,
        &explicit_files,
        global_resolved.as_ref(),
        &flags,
        &mut cache,
        &mut notices,
        &mut files,
    )?;
    emit_notices(&notices, args.lint.compatibility.no_warnings);
    Ok((global_cfg, files, ruleless_config_found))
}

/// Open destinations before `--fix` mutates anything (so an unopenable `--output-file` fails
/// fast), then fix/lint/write and map the tally to an exit code.
///
/// # Errors
///
/// Returns an error if a file cannot be read/written or an output destination fails.
fn lint_and_exit(
    files: &[LintFile],
    args: &LintArgs,
    targets: &[OutputTarget],
) -> Result<ExitCode, String> {
    let mut sinks = open_targets(targets)?;

    let no_warnings = args.lint.compatibility.no_warnings;
    let before_fix = if args.lint.fix.fix {
        Some(apply_fixes_reporting_skips(files, no_warnings)?)
    } else {
        None
    };

    let results = lint_files(files);
    let fixed = before_fix.map(|before| {
        let after = safe_fix_rule_counts(&results, no_warnings);
        before
            .iter()
            .zip(&after)
            .flat_map(|(before, after)| before.iter().zip(after))
            .map(|(before, after)| before.saturating_sub(*after))
            .sum::<usize>()
    });
    let (summary, records) = collect_records(files, results, no_warnings);
    write_targets(targets, &mut sinks, &records)?;

    if let Some(fixed) = fixed
        && let found = fixed + summary.problem_count
        && found > 0
    {
        eprintln!(
            "Found {} {} ({} fixed, {} remaining).",
            found,
            pluralize("problem", found),
            fixed,
            summary.problem_count
        );
    }

    Ok(summary_to_exit(&summary, args.lint.compatibility.strict))
}

/// Apply safe fixes in place and report any files skipped (they do not parse), returning the
/// pre-fix [`safe_fix_rule_counts`] for the summary.
///
/// # Errors
///
/// Returns an error if any file cannot be read or written.
fn apply_fixes_reporting_skips(
    files: &[LintFile],
    no_warnings: bool,
) -> Result<Vec<SafeFixRuleCounts>, String> {
    let before = safe_fix_rule_counts(&lint_files(files), no_warnings);
    let fix_stats = rewrite_files(files, Rewrite::Fix)?;
    for (path, problem) in &fix_stats.skipped {
        eprint_skip_notice(path, problem, Rewrite::Fix.flag());
    }
    Ok(before)
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

/// Stderr notice for a file `--fix`/`--diff` left untouched, or for a rule's finding it
/// could not fix. Path and message are user-controlled, so both are sanitized; `action` is
/// the literal flag name.
fn eprint_skip_notice(path: &Path, problem: &LintProblem, action: &str) {
    let what = problem.rule.map_or_else(
        || format!("skipped by {action}"),
        |rule| format!("{rule} not fixed"),
    );
    eprintln!(
        "{}:{}:{} {what}: {}",
        sanitize_control(&path.display().to_string()),
        problem.line,
        problem.column,
        sanitize_control(&problem.message),
    );
}

fn run_stdin_lint(args: &LintArgs, matches: &ArgMatches) -> Result<ExitCode, String> {
    let (path, base_dir, cfg, apply_yaml_files, config_found) =
        resolve_stdin_ctx(args)?;

    // As in `run_lint`, `--diff` skips config `[output]` so a config report can't block it.
    let config_output = if args.lint.fix.diff {
        None
    } else {
        cfg.output()
    };
    let targets = resolve_targets(matches, args, config_output)?;
    validate_targets(&targets, args.lint.fix.diff)?;
    let targets = &targets;

    reject_input_collisions(targets, std::iter::once(path.as_path()))?;

    let Some(kind) =
        resolve_stdin_kind(args, &cfg, &path, &base_dir, apply_yaml_files)?
    else {
        // An ignored stdin filename is an empty input set: still emit a valid empty
        // report per target so CI artifact ingestion does not see a missing file.
        emit_targets(targets, &[])?;
        return Ok(ExitCode::SUCCESS);
    };

    if args.lint.compatibility.list_files {
        println!("{}", sanitize_control(&path.display().to_string()));
        return Ok(ExitCode::SUCCESS);
    }

    if !cfg.enables_any_rule() {
        return Err(no_rules_error(config_found));
    }

    if args.lint.fix.diff {
        let stats = stdin_diff_stats(&path, &base_dir, &cfg, kind, Rewrite::Fix)?;
        return Ok(emit_diff(&stats, Preview::Diff));
    }

    let outcome = read_and_lint_stdin(&path, &base_dir, &cfg, kind);

    let files = vec![(path, base_dir, Arc::new(cfg), kind)];
    let results = vec![(0usize, outcome)];

    let mut sinks = open_targets(targets)?;
    let (summary, records) =
        collect_records(&files, results, args.lint.compatibility.no_warnings);
    write_targets(targets, &mut sinks, &records)?;
    Ok(summary_to_exit(&summary, args.lint.compatibility.strict))
}

/// Resolve the source kind for stdin, or `None` to skip an ignored `--stdin-filename`.
/// `--markdown` forces Markdown; with `--stdin-filename` the kind comes from `[files]` globs
/// (no match is an error); without one the input is YAML.
fn resolve_stdin_kind(
    args: &LintArgs,
    cfg: &YamlLintConfig,
    path: &Path,
    base_dir: &Path,
    apply_yaml_files: bool,
) -> Result<Option<SourceKind>, String> {
    if apply_yaml_files && cfg.is_file_ignored(path, base_dir) {
        return Ok(None);
    }
    if args.lint.markdown {
        return Ok(Some(SourceKind::Markdown));
    }
    if !apply_yaml_files {
        return Ok(Some(SourceKind::Yaml));
    }
    match cfg.source_kind(path, base_dir)? {
        Some(kind) => Ok(Some(kind)),
        // No-kind match is an error, like an explicitly-passed file (see `gather_lint_files`).
        None => Err(format!(
            "{}: no source kind matches; add a matching glob under \
             [files].yaml or [files].markdown",
            path.display()
        )),
    }
}

/// Read stdin, returning its decoded text and raw bytes; they are equal exactly when the
/// input was plain UTF-8 (no BOM, no transcode), i.e. when a textual `--diff` applies back.
fn read_stdin_decoded(path: &Path) -> Result<(String, Vec<u8>), String> {
    let mut buf = Vec::new();
    std::io::stdin()
        .read_to_end(&mut buf)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    let content = decoder::decode_bytes(&buf)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    Ok((content, buf))
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

fn stdin_diff_stats(
    path: &Path,
    base_dir: &Path,
    cfg: &YamlLintConfig,
    kind: SourceKind,
    rewrite: Rewrite,
) -> Result<DiffStats, String> {
    let (content, raw) = read_stdin_decoded(path)?;
    let mut stats = DiffStats::default();
    if content.as_bytes() == raw {
        stats.record(
            path,
            diff_outcome(&content, cfg, path, base_dir, kind, rewrite),
        );
    } else {
        // The decoded-UTF-8 diff would not apply to the BOM'd/transcoded source, so skip
        // rather than emit a patch that won't apply (same as the file path).
        stats
            .skipped
            .push((path.to_path_buf(), ryl::fix::non_utf8_diff_skip()));
    }
    Ok(stats)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Preview {
    Diff,
    Check,
}

impl Preview {
    fn flag(self) -> &'static str {
        match self {
            Self::Diff => "--diff",
            Self::Check => "--check",
        }
    }
}

/// Write parse-skip notices to stderr and, for `--diff`, the per-file unified diffs to
/// stdout, returning `1` if any file would change, else `0`. Only the diff drives the exit
/// code; remaining unfixable diagnostics are neither printed nor counted.
fn emit_diff(stats: &DiffStats, preview: Preview) -> ExitCode {
    for (path, problem) in &stats.skipped {
        eprint_skip_notice(path, problem, preview.flag());
    }
    if preview == Preview::Diff {
        // Each diff already carries its trailing newline, so concatenate and print once.
        print!("{}", stats.diffs.concat());
    }
    if stats.diffs.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

/// `ryl format`: rewrite each input in place (stdin to stdout), or preview with
/// `--check`/`--diff`. Unlike `check`, a config enabling no rules is not an error.
///
/// # Errors
///
/// A usage or config error, or a file that cannot be read or written.
fn run_format(format: &FormatArgs) -> Result<ExitCode, String> {
    let args = LintArgs {
        source: format.source.clone(),
        lint: LintFlags {
            compatibility: CompatibilityLintFlags {
                no_warnings: format.no_warnings,
                ..CompatibilityLintFlags::default()
            },
            markdown: format.markdown,
            ..LintFlags::default()
        },
        ..LintArgs::default()
    };
    let preview = if format.diff {
        Some(Preview::Diff)
    } else {
        format.check.then_some(Preview::Check)
    };
    if stdin_requested(&args.source)? {
        return run_stdin_format(&args, preview, format.output_format);
    }
    let (_, files, _) = collect_files(&args)?;
    warn_format_conflicts(files.iter().map(|(.., cfg, _)| cfg.as_ref()), &args);
    if let Some(preview) = preview {
        let output_config = files.first().and_then(|(.., cfg, _)| cfg.output());
        let targets = format_targets(preview, format.output_format, output_config);
        reject_input_collisions(
            &targets,
            files.iter().map(|(path, ..)| path.as_path()),
        )?;
        let stats = diff_files(&files, Rewrite::Format, preview.flag())?;
        return emit_format_preview(&stats, preview, &targets);
    }
    for (path, problem) in &rewrite_files(&files, Rewrite::Format)?.skipped {
        eprint_skip_notice(path, problem, Rewrite::Format.flag());
    }
    Ok(ExitCode::SUCCESS)
}

/// Under `--check`, `--output-format` beats `[output]`; `--diff` keeps the default target,
/// as in ruff.
fn format_targets(
    preview: Preview,
    output_format: Option<CliFormat>,
    config_output: Option<&OutputTable>,
) -> Vec<OutputTarget> {
    match (preview, output_format) {
        (Preview::Diff, _) => config_or_default_targets(None),
        (Preview::Check, Some(choice)) => vec![default_target(choice)],
        (Preview::Check, None) => config_or_default_targets(config_output),
    }
}

fn warn_format_conflicts<'a>(
    configs: impl IntoIterator<Item = &'a YamlLintConfig>,
    args: &LintArgs,
) {
    if args.lint.compatibility.no_warnings {
        return;
    }
    let mut distinct: Vec<&YamlLintConfig> = Vec::new();
    for cfg in configs {
        if !distinct.iter().any(|seen| seen.source() == cfg.source()) {
            distinct.push(cfg);
        }
    }
    for cfg in &distinct {
        let prefix = match (distinct.len() > 1, cfg.source()) {
            (true, Some(path)) => {
                format!("{}: ", sanitize_control(&path.display().to_string()))
            }
            _ => String::new(),
        };
        for warning in ryl::format::conflicts(cfg) {
            eprintln!("warning: {prefix}{warning}");
        }
    }
}

fn emit_format_preview(
    stats: &DiffStats,
    preview: Preview,
    targets: &[OutputTarget],
) -> Result<ExitCode, String> {
    let records: Vec<FileRecord> = stats
        .problems
        .iter()
        .map(|(path, problems)| FileRecord {
            path,
            kept: problems.clone(),
            error: None,
        })
        .collect();
    emit_targets(targets, &records)?;
    Ok(emit_diff(stats, preview))
}

/// `ryl format -`: the formatted text goes to stdout, an ignored `--stdin-filename` passing
/// through unchanged.
fn run_stdin_format(
    args: &LintArgs,
    preview: Option<Preview>,
    output_format: Option<CliFormat>,
) -> Result<ExitCode, String> {
    let (path, base_dir, cfg, apply_yaml_files, _) = resolve_stdin_ctx(args)?;
    let kind = resolve_stdin_kind(args, &cfg, &path, &base_dir, apply_yaml_files)?;
    warn_format_conflicts([&cfg], args);
    if let (Some(preview), Some(kind)) = (preview, kind) {
        let targets = format_targets(preview, output_format, cfg.output());
        reject_input_collisions(&targets, std::iter::once(path.as_path()))?;
        let stats = stdin_diff_stats(&path, &base_dir, &cfg, kind, Rewrite::Format)?;
        return emit_format_preview(&stats, preview, &targets);
    }
    if preview.is_some() {
        return Ok(ExitCode::SUCCESS);
    }
    let (content, raw) = read_stdin_decoded(&path)?;
    let (formatted, skipped) = kind.map_or((None, Vec::new()), |kind| {
        rewrite_str(&content, &cfg, &path, &base_dir, kind, Rewrite::Format)
    });
    for problem in &skipped {
        eprint_skip_notice(&path, problem, Rewrite::Format.flag());
    }
    // Unchanged input goes back byte-for-byte, keeping a BOM or non-UTF-8 encoding.
    std::io::stdout()
        .write_all(&formatted.map_or(raw, String::into_bytes))
        .expect("stdout accepts the output, as `print!` assumes");
    Ok(ExitCode::SUCCESS)
}

fn resolve_stdin_ctx(
    args: &LintArgs,
) -> Result<(PathBuf, PathBuf, YamlLintConfig, bool, bool), String> {
    let (path, apply_yaml_files) = match args.source.stdin_filename.clone() {
        Some(name) => (name, true),
        None => (PathBuf::from(STDIN_LABEL), false),
    };
    let anchor = if apply_yaml_files {
        path.clone()
    } else {
        PathBuf::from(".")
    };
    let ctx = discover_config(std::slice::from_ref(&anchor), &cli_overrides(args))?;
    emit_notices(&ctx.notices, args.lint.compatibility.no_warnings);
    let mut cfg = ctx.config;
    cli_config_flags(args).apply(&mut cfg, &ctx.base_dir);
    if !apply_yaml_files {
        cfg.disable_path_based_rule_ignores();
    }
    Ok((path, ctx.base_dir, cfg, apply_yaml_files, ctx.config_found))
}

/// Whether a migrated `source` is a user-global config, by file identity so a relative
/// `--migrate-root` spelling still matches; a deleted source only matches literally.
fn is_user_source(source: &Path, user_sources: &[PathBuf]) -> bool {
    user_sources.iter().any(|user| {
        user == source || same_file::is_same_file(user, source).unwrap_or(false)
    })
}

/// Config notices are all warnings, so `--no-warnings` silences them.
fn emit_notices(notices: &[String], no_warnings: bool) {
    if no_warnings {
        return;
    }
    for notice in notices {
        eprintln!("{}", sanitize_control(notice));
    }
}

fn lint_files(files: &[LintFile]) -> Vec<(usize, Result<Vec<LintProblem>, String>)> {
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
    global_cfg: Option<&ResolvedConfig>,
    flags: &CliConfigFlags,
    cache: &mut ConfigCache,
    notices: &mut Vec<String>,
    files: &mut Vec<LintFile>,
) -> Result<Option<bool>, String> {
    // `config_found` of the first selected file that enables no rules, so a no-rules run
    // reports the right message for that file ("no config found" vs "config enables no
    // rules"). `None` means every selected file enables at least one rule.
    let mut ruleless_config_found = None;
    // De-duplicate across the walk and explicit args, so a file listed twice is linted once
    // (and `--diff` does not emit a duplicate patch block that fails to apply).
    let mut seen: HashSet<PathBuf> = HashSet::new();
    // Directory candidates first (a no-kind file is silently skipped), then explicit args (a
    // no-kind file is a hard error, since the user named it); `explicit` selects which.
    // `seen` spans both, so a file reached both ways is linted once, in walk order.
    let tagged = candidates
        .iter()
        .map(|path| (path, false))
        .chain(explicit_files.iter().map(|path| (path, true)));
    for (path, explicit) in tagged {
        let (base_dir, cfg, file_notices, found) =
            resolve_ctx(path, global_cfg, flags, cache)?;
        for notice in file_notices {
            if !notices.contains(&notice) {
                notices.push(notice);
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

/// One linted file's format-agnostic outcome: kept diagnostics or a processing-error message
/// (mutually exclusive; a clean file has neither). Every target renders the same records, so
/// the filter+tally pass runs once.
struct FileRecord<'a> {
    path: &'a Path,
    kept: Vec<LintProblem>,
    error: Option<String>,
}

/// Filter and tally every lint result into [`FileRecord`]s in file order. The returned
/// [`LintSummary`] (and exit code) is independent of which formats render the records.
/// `no_warnings` drops warning-level diagnostics before they are kept or counted.
fn collect_records<'a>(
    files: &'a [LintFile],
    results: Vec<(usize, Result<Vec<LintProblem>, String>)>,
    no_warnings: bool,
) -> (LintSummary, Vec<FileRecord<'a>>) {
    let mut summary = LintSummary::default();
    let mut records: Vec<FileRecord<'a>> = Vec::with_capacity(results.len());

    for (idx, outcome) in results {
        let (path, ..) = &files[idx];
        match outcome {
            Err(message) => {
                // Sanitize: a crafted filename could otherwise inject terminal escapes or
                // (via a newline) a GitHub workflow command. Safe for every format because it
                // neutralises the newline injection needs.
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

type SafeFixRuleCounts = [usize; SAFE_FIX_RULE_IDS.len()];

/// Each file's reported problems per safe-fix rule, so a fix that adds another rule's problem
/// cannot cancel one it cleared.
fn safe_fix_rule_counts(
    results: &[(usize, Result<Vec<LintProblem>, String>)],
    no_warnings: bool,
) -> Vec<SafeFixRuleCounts> {
    results
        .iter()
        .map(|(_, outcome)| {
            let mut counts = [0; SAFE_FIX_RULE_IDS.len()];
            for problem in outcome.iter().flatten() {
                if !(no_warnings && problem.level == Severity::Warning)
                    && let Some(idx) = SAFE_FIX_RULE_IDS
                        .iter()
                        .position(|id| problem.rule == Some(*id))
                {
                    counts[idx] += 1;
                }
            }
            counts
        })
        .collect()
}

fn pluralize(singular: &str, count: usize) -> &str {
    if count == 1 { singular } else { "problems" }
}

// The streaming `append_*` fns are only reached with a non-empty `problems` slice (the
// caller skips clean files), and every write is infallible (see `OUTPUT_INFALLIBLE`).

/// Shared shape of the standard/colored formats: a `header` line, one `format_line` per
/// diagnostic, then a trailing blank line.
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

/// `escaped_file` is the `file=` property value, escaped once per file by the caller.
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

/// `sanitized_path` is sanitized once per file by the caller.
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
