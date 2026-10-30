#![forbid(unsafe_code)]
#![deny(
    clippy::all,
    clippy::pedantic,
    clippy::cargo,
    clippy::cognitive_complexity
)]

mod cache;

use std::collections::HashSet;
use std::fs::File;
use std::io::{BufWriter, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use clap::{ArgMatches, CommandFactory, FromArgMatches, Parser, ValueEnum};
use ignore::WalkBuilder;

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
use ryl::directives::starts_with_jinja_header;
use ryl::fix::{
    DiffOutcome, DiffStats, Rewrite, SAFE_FIX_RULE_IDS, decoded_diff_outcome,
    diff_files, rewrite_files, rewrite_str, skip_notice,
};
use ryl::lint::lint_str_with_repair_help;
use ryl::migrate::{
    MigrateOptions, OutputMode as MigrateOutputMode, SourceCleanup,
    UserConfigMigration, WriteMode, migrate_configs,
};
use ryl::report::{GitlabState, JunitReport, ReportEntry};
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

/// Run-level output and strict settings use the global config or the first project
/// config discovered along the inputs (argument-order sensitive).
///
/// # Errors
///
/// Propagates config discovery/parse/validation errors, including for empty inputs.
fn run_config_settings(
    global_cfg: Option<&ConfigContext>,
    inputs: &[PathBuf],
    args: &LintArgs,
) -> Result<(Option<OutputTable>, bool), String> {
    if let Some(ctx) = global_cfg {
        return Ok((ctx.config.output().cloned(), ctx.config.strict()));
    }
    let cfg = discover_config(inputs, &cli_overrides(args))?.config;
    Ok((cfg.output().cloned(), cfg.strict()))
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
    if !cli.migrate.write && !cli.migrate.stdout && !result.entries.is_empty() {
        println!(
            "Preview only: add `--migrate-stdout` to see the migrated content, or \
             `--migrate-write` to apply it."
        );
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
    about = "Fast YAML linter and formatter",
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

    /// Convert legacy user-global config into ryl.toml or move deprecated TOML keys
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

    /// Disable lint rules (comma-separated IDs, or ALL); for format, only suppress
    /// conflict warnings
    #[arg(
        long = "disable",
        value_name = "RULES",
        value_delimiter = ',',
        value_parser = parse_rule_selector,
        action = clap::ArgAction::Set,
        num_args = 1
    )]
    disable: Vec<&'static str>,

    /// Replace config exclusions with comma-separated gitignore-style globs, relative to cwd
    #[arg(
        long = "exclude",
        value_name = "GLOBS",
        value_delimiter = ',',
        action = clap::ArgAction::Set,
        num_args = 1
    )]
    exclude: Option<Vec<String>>,

    /// Disable the on-disk result cache
    #[arg(long = "no-cache", default_value_t = false)]
    no_cache: bool,

    /// Directory for the on-disk result cache
    #[arg(long = "cache-dir", value_name = "PATH")]
    cache_dir: Option<PathBuf>,
}

impl SourceArgs {
    fn cache_settings(&self) -> cache::Settings {
        cache::Settings::new(self.no_cache, self.cache_dir.clone())
    }
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
        disable: if args.source.disable.contains(&"ALL") {
            ALL_RULE_IDS.to_vec()
        } else {
            args.source.disable.clone()
        },
        exclude: args.source.exclude.clone(),
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
#[expect(clippy::struct_excessive_bools, reason = "one field per CLI flag")]
struct CompatibilityLintFlags {
    /// List the files that would be linted (one per line), then exit
    #[arg(long = "list-files", default_value_t = false)]
    list_files: bool,

    /// Return exit code 2 when only warnings (no errors) are found
    #[arg(short = 's', long = "strict", conflicts_with = "no_strict")]
    strict: bool,

    /// Allow warning-only runs to succeed, overriding `lint.strict`
    #[arg(long = "no-strict")]
    no_strict: bool,

    /// Suppress warning-level problems (report only errors)
    #[arg(long = "no-warnings", default_value_t = false)]
    no_warnings: bool,
}

impl CompatibilityLintFlags {
    fn strict(&self, config_strict: bool) -> bool {
        self.strict || (!self.no_strict && config_strict)
    }
}

#[derive(clap::Args, Debug, Default)]
struct MigrateFlags {
    /// Directory to search for legacy config files, or one config file (default: .)
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

    /// Rename source YAML configs or back up rewritten TOML by appending this suffix
    #[arg(
        long = "migrate-rename-old",
        value_name = "SUFFIX",
        conflicts_with = "delete_old",
        requires_all = ["write", "migrate_mode"]
    )]
    rename_old: Option<String>,

    /// Delete source YAML configs after migration; rewritten TOML is kept
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
/// contents survive until [`OutputSink::start`] truncates it: a later target failing to
/// open then cannot destroy an existing artifact (a freshly created one may be left empty
/// if a later target aborts; see [`open_destination`]).
enum OutputSink {
    /// Buffered stdout or stderr.
    Stream(Box<dyn Write>),
    /// A `--output-file` target, truncated when the output starts.
    File(File),
}

impl OutputSink {
    /// The writer for this sink's complete contents, a file truncated first to clear any
    /// prior artifact.
    fn start(self) -> std::io::Result<Box<dyn Write>> {
        match self {
            Self::Stream(writer) => Ok(writer),
            Self::File(file) => {
                file.set_len(0)?;
                Ok(Box::new(BufWriter::new(file)))
            }
        }
    }
}

/// Open one destination for writing. A file is opened create+write but **not** truncate, so
/// an unopenable `--output-file` fails fast (before `--fix` mutates anything) yet an existing
/// destination survives until [`OutputSink::start`] truncates it. A freshly-created
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
        Destination::Stderr => Ok(OutputSink::Stream(Box::new(BufWriter::new(
            std::io::stderr(),
        )))),
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

/// Renders each file's [`FileRecord`] to every target as it arrives, so output never waits
/// for the whole run. `JUnit`'s root counts precede its body, so its body is held until
/// [`RecordStream::finish`].
struct RecordStream<'a> {
    targets: &'a [OutputTarget],
    writers: Vec<Box<dyn Write>>,
    junit: Option<JunitReport>,
    gitlab: Option<GitlabState>,
    errors_to_stderr: bool,
    project_root: PathBuf,
    /// The first write failure, after which writes are skipped; `finish` reports it.
    status: std::io::Result<()>,
}

impl<'a> RecordStream<'a> {
    /// Truncate every sink and open each `GitLab` array, for records of `paths`.
    ///
    /// # Errors
    ///
    /// Propagates the first truncation failure.
    fn start<'p>(
        targets: &'a [OutputTarget],
        sinks: Vec<OutputSink>,
        paths: impl IntoIterator<Item = &'p Path>,
    ) -> Result<Self, String> {
        let project_root = report_project_root();
        let mut writers: Vec<_> = sinks
            .into_iter()
            .map(OutputSink::start)
            .collect::<std::io::Result<_>>()
            .map_err(|err| write_output_error(&err))?;
        let mut status = Ok(());
        for (target, writer) in targets.iter().zip(&mut writers) {
            if target.format == OutputFormat::Gitlab {
                then(&mut status, || writer.write_all(b"["));
            }
        }
        Ok(Self {
            targets,
            writers,
            junit: targets
                .iter()
                .any(|target| target.format == OutputFormat::Junit)
                .then(JunitReport::default),
            gitlab: targets
                .iter()
                .any(|target| target.format == OutputFormat::Gitlab)
                .then(|| {
                    let paths: Vec<_> = paths
                        .into_iter()
                        .map(|path| report_display_path(path, &project_root))
                        .collect();
                    GitlabState::new(paths.iter().map(String::as_str))
                }),
            errors_to_stderr: !targets.iter().any(|target| {
                target.format.is_streaming()
                    && matches!(&target.destination, Destination::Stderr)
            }),
            project_root,
            status,
        })
    }

    /// Open then start every target. For paths with no `--fix` ordering constraint (empty
    /// input and the format previews); `check` opens early via [`open_targets`].
    ///
    /// # Errors
    ///
    /// Propagates an open or truncation failure.
    fn open<'p>(
        targets: &'a [OutputTarget],
        paths: impl IntoIterator<Item = &'p Path>,
    ) -> Result<Self, String> {
        Self::start(targets, open_targets(targets)?, paths)
    }

    /// Replay a file's captured stderr, through the console target when it owns stderr so
    /// the two stay in order.
    fn replay_stderr(&mut self, bytes: &[u8]) {
        match self
            .targets
            .iter()
            .position(|target| target.destination == Destination::Stderr)
        {
            Some(index) => {
                then(&mut self.status, || self.writers[index].write_all(bytes));
            }
            None => {
                let _ = std::io::stderr().write_all(bytes);
            }
        }
    }

    fn push(&mut self, record: &FileRecord) {
        if self.errors_to_stderr
            && let Some(message) = &record.error
        {
            eprintln!("error: {message}");
        }
        let entry = report_entry(record, &self.project_root);
        let mut gitlab = Vec::new();
        if let Some(state) = &mut self.gitlab {
            state.append(&mut gitlab, &entry);
        }
        if let Some(junit) = &mut self.junit {
            junit.append(&entry);
        }
        for (target, writer) in self.targets.iter().zip(&mut self.writers) {
            let block = match target.format {
                OutputFormat::Junit => continue,
                OutputFormat::Gitlab => std::mem::take(&mut gitlab),
                format => console_block(format, record),
            };
            then(&mut self.status, || writer.write_all(&block));
        }
    }

    /// Close each report and flush every target.
    ///
    /// # Errors
    ///
    /// Reports the first destination write failure.
    fn finish(mut self) -> Result<(), String> {
        let junit = self.junit.take().map(JunitReport::finish);
        for (target, writer) in self.targets.iter().zip(&mut self.writers) {
            let tail: [&[u8]; 2] = match target.format {
                OutputFormat::Junit => {
                    let (root, body) = junit.as_ref().expect(JUNIT_REPORT_KEPT);
                    [root, body]
                }
                OutputFormat::Gitlab => [b"]\n", b""],
                _ => [b"", b""],
            };
            then(&mut self.status, || {
                tail.iter()
                    .try_for_each(|bytes| writer.write_all(bytes))
                    .and_then(|()| writer.flush())
            });
        }
        self.status.map_err(|err| write_output_error(&err))
    }
}

/// Run `write` unless an earlier write in `status` failed, keeping the first failure.
fn then(status: &mut std::io::Result<()>, write: impl FnOnce() -> std::io::Result<()>) {
    *status = std::mem::replace(status, Ok(())).and_then(|()| write());
}

// A JUnit target means its report is kept, so `finish` only sees `Some`; the `expect` pins
// that invariant rather than leaving an uncovered `None` arm.
const JUNIT_REPORT_KEPT: &str = "the report is kept when a JUnit target is present";

/// One record's block in a console `format`: nothing for a clean file, its
/// (already-sanitized) message line for a processing error.
fn console_block(format: OutputFormat, record: &FileRecord) -> Vec<u8> {
    let append: fn(&mut Vec<u8>, &Path, &[LintProblem]) = match format {
        OutputFormat::Colored => append_colored,
        OutputFormat::Github => append_github,
        OutputFormat::Parsable => append_parsable,
        _ => append_standard,
    };
    let mut block = Vec::new();
    if let Some(message) = &record.error {
        writeln!(block, "error: {message}").expect(OUTPUT_INFALLIBLE);
    } else if !record.kept.is_empty() {
        append(&mut block, record.path, &record.kept);
        if matches!(format, OutputFormat::Standard | OutputFormat::Colored)
            && let Some(diff) = &record.diff
        {
            block.pop();
            append_diff(&mut block, diff, format == OutputFormat::Colored);
            writeln!(block).expect(OUTPUT_INFALLIBLE);
        }
    }
    block
}

/// `diff`'s hunks, indented under a file's finding as ruff shows what `--check` would
/// change; tabs survive the control-character escaping.
fn append_diff(out: &mut Vec<u8>, diff: &str, colored: bool) {
    for line in diff.lines().skip_while(|line| !line.starts_with("@@")) {
        let text = line
            .split('\t')
            .map(sanitize_control)
            .collect::<Vec<_>>()
            .join("\t");
        let color = match line.as_bytes().first() {
            Some(b'-') => "31",
            Some(b'+') => "32",
            Some(b'@') => "36",
            _ => "0",
        };
        if colored {
            writeln!(out, "  \u{001b}[{color}m{text}\u{001b}[0m")
        } else {
            writeln!(out, "  {text}")
        }
        .expect(OUTPUT_INFALLIBLE);
    }
}

/// A record (clean files included) as a [`ReportEntry`] with a project-root relative
/// display path; the report emitters decide how to render a clean file.
fn report_entry<'a>(record: &'a FileRecord, project_root: &Path) -> ReportEntry<'a> {
    ReportEntry {
        path: report_display_path(record.path, project_root),
        problems: &record.kept,
        error: record.error.as_deref(),
    }
}

fn file_paths(files: &[LintFile]) -> impl Iterator<Item = &Path> {
    files.iter().map(|(path, ..)| path.as_path())
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
        for (path, .., kind) in &files {
            if *kind == SourceKind::Yaml && has_jinja_header(path) {
                continue;
            }
            println!("{}", sanitize_control(&path.display().to_string()));
        }
        return Ok(ExitCode::SUCCESS);
    }

    // `--diff` has its own unified-diff output, so skip config `[output]`: a config report
    // target must not block it. An explicit CLI `--format junit|gitlab` still conflicts via
    // the CLI-derived targets `validate_targets` reads regardless.
    let (output_config, config_strict) = if args.lint.fix.diff {
        (None, false)
    } else {
        run_config_settings(global_cfg.as_ref(), &args.source.inputs, args)?
    };
    let targets = resolve_targets(matches, args, output_config.as_ref())?;
    validate_targets(&targets, args.lint.fix.diff)?;
    let targets = &targets;

    reject_input_collisions(targets, file_paths(&files))?;

    if files.is_empty() {
        // Still emit a valid empty report per target, so CI artifact ingestion sees
        // `[]` / `<testsuites .../>` rather than a missing file.
        return RecordStream::open(targets, [])?
            .finish()
            .map(|()| ExitCode::SUCCESS);
    }

    if let Some(config_found) = ruleless_config_found {
        return Err(no_rules_error(config_found));
    }

    if args.lint.fix.diff {
        let mut out = PreviewOutput::new(Preview::Diff, None);
        let stats = diff_files(
            &files,
            (Rewrite::Fix, "--diff"),
            &[],
            &mut |path, outcome| {
                out.push(path, outcome);
            },
        )?;
        return out.finish(&stats, 0);
    }

    lint_and_exit(&files, args, targets, config_strict)
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
        flags.apply(&mut ctx.config, &ctx.base_dir)?;
        ctx.config_found |= flags.enable.is_some();
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
/// Returns an error if an output destination fails.
fn lint_and_exit(
    files: &[LintFile],
    args: &LintArgs,
    targets: &[OutputTarget],
    config_strict: bool,
) -> Result<ExitCode, String> {
    let sinks = open_targets(targets)?;

    let no_warnings = args.lint.compatibility.no_warnings;
    let before_fix = args
        .lint
        .fix
        .fix
        .then(|| apply_fixes_reporting_skips(files, no_warnings));

    let mut stream = RecordStream::start(targets, sinks, file_paths(files))?;
    let mut summary = LintSummary::default();
    let mut fixed = 0;
    let emit = |idx: usize, (outcome, stderr): (LintOutcome, Vec<u8>)| {
        let (path, ..) = &files[idx];
        let outcome = match &before_fix {
            Some((before, errors)) => {
                fixed += before[idx]
                    .iter()
                    .zip(safe_fix_rule_counts(&outcome, no_warnings))
                    .map(|(before, after)| before.saturating_sub(after))
                    .sum::<usize>();
                match errors.iter().find(|(index, _)| *index == idx) {
                    Some((_, error)) => Err(error.clone()),
                    None => outcome,
                }
            }
            None => outcome,
        };
        stream.replay_stderr(&stderr);
        stream.push(&summary.record(path, outcome, no_warnings));
    };
    if before_fix.is_some() {
        lint_files(files, emit);
    } else {
        lint_files_cached(files, &args.source.cache_settings(), emit)?;
    }
    stream.finish().map(|()| {
        if before_fix.is_some()
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
        if before_fix
            .as_ref()
            .is_none_or(|(_, errors)| errors.is_empty())
        {
            summary_to_exit(&summary, args.lint.compatibility.strict(config_strict))
        } else {
            ExitCode::from(2)
        }
    })
}

/// Apply safe fixes in place and report skips, returning pre-fix rule counts and
/// file errors for the shared report records.
///
fn apply_fixes_reporting_skips(files: &[LintFile], no_warnings: bool) -> FixBaseline {
    let mut before = Vec::with_capacity(files.len());
    lint_files(files, |_, (outcome, stderr)| {
        let _ = std::io::stderr().write_all(&stderr);
        before.push(safe_fix_rule_counts(&outcome, no_warnings));
    });
    let fix_stats = rewrite_files(files, Rewrite::Fix);
    let _ = std::io::stderr().write_all(&fix_stats.notices);
    for (index, _) in &fix_stats.errors {
        before[*index] = [0; SAFE_FIX_RULE_IDS.len()];
    }
    (before, fix_stats.errors)
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
    eprint!("{}", skip_notice(path, problem, action));
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
        return RecordStream::open(targets, [])?
            .finish()
            .map(|()| ExitCode::SUCCESS);
    };

    if args.lint.compatibility.list_files {
        println!("{}", sanitize_control(&path.display().to_string()));
        return Ok(ExitCode::SUCCESS);
    }

    if !cfg.enables_any_rule() {
        return Err(no_rules_error(config_found));
    }

    if args.lint.fix.diff {
        let mut out = PreviewOutput::new(Preview::Diff, None);
        let stats =
            stdin_preview(&path, &base_dir, &cfg, (kind, Rewrite::Fix), &mut out)?;
        return out.finish(&stats, 0);
    }

    let outcome =
        read_stdin_decoded(&path, Some(kind))?.map(|(content, _)| match kind {
            SourceKind::Markdown => {
                lint_markdown_str(content.content(), &path, &cfg, &base_dir)
            }
            SourceKind::Yaml => lint_str(content.content(), &path, &cfg, &base_dir),
        });
    let strict = args.lint.compatibility.strict(cfg.strict());

    let mut summary = LintSummary::default();
    let mut stream = RecordStream::open(targets, [path.as_path()])?;
    stream.push(&summary.record(&path, outcome, args.lint.compatibility.no_warnings));
    stream.finish().map(|()| summary_to_exit(&summary, strict))
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

/// Whether the file opens with a Jinja renderer header; an unreadable file reads as not,
/// so its read error surfaces where the file is linted. The encoding override is read
/// directly so its warning prints once, where the file is read.
fn has_jinja_header(path: &Path) -> bool {
    let encoding = std::env::var("YAMLLINT_FILE_ENCODING").ok();
    std::fs::read(path)
        .ok()
        .and_then(|bytes| {
            decoder::decode_bytes_with_override(&bytes, encoding.as_deref()).ok()
        })
        .is_some_and(|text| starts_with_jinja_header(&text))
}

fn jinja_template_error(path: &Path) -> String {
    format!(
        "{}: a Jinja template (`#!` renderer header); ryl does not lint or format \
         templates",
        path.display()
    )
}

type StdinRead = Result<(decoder::DecodedFile, Vec<u8>), String>;

/// Read and decode stdin; the outer error refuses a YAML Jinja template, the inner one is
/// a read or decode failure.
fn read_stdin_decoded(
    path: &Path,
    kind: Option<SourceKind>,
) -> Result<StdinRead, String> {
    let read_error = |err: String| format!("failed to read {}: {err}", path.display());
    let mut buf = Vec::new();
    if let Err(err) = std::io::stdin().read_to_end(&mut buf) {
        return Ok(Err(read_error(err.to_string())));
    }
    let decoded = decoder::decode_bytes_lossless(&buf);
    if kind == Some(SourceKind::Yaml)
        && decoded
            .as_ref()
            .is_ok_and(|content| starts_with_jinja_header(content.content()))
    {
        return Err(jinja_template_error(path));
    }
    Ok(decoded.map(|content| (content, buf)).map_err(read_error))
}

fn stdin_preview<'a>(
    path: &'a Path,
    base_dir: &Path,
    cfg: &YamlLintConfig,
    (kind, rewrite): (SourceKind, Rewrite),
    out: &mut PreviewOutput<'a>,
) -> Result<DiffStats, String> {
    let (decoded, _) = read_stdin_decoded(path, Some(kind))??;
    let outcome = decoded_diff_outcome(&decoded, cfg, path, base_dir, kind, rewrite);
    let mut stats = DiffStats::default();
    stats.record(path, &outcome, out.preview.flag());
    out.push(path, outcome);
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

/// Streams a rewrite preview: each changed file's diff to stdout under `--diff`, and each
/// `ryl format` finding to its targets. Skip notices print once the run ends.
struct PreviewOutput<'a> {
    preview: Preview,
    diffs: BufWriter<std::io::Stdout>,
    status: std::io::Result<()>,
    findings: Option<RecordStream<'a>>,
}

impl<'a> PreviewOutput<'a> {
    fn new(preview: Preview, findings: Option<RecordStream<'a>>) -> Self {
        Self {
            preview,
            diffs: BufWriter::new(std::io::stdout()),
            status: Ok(()),
            findings,
        }
    }

    fn push(&mut self, path: &'a Path, outcome: DiffOutcome) {
        let check = self.preview == Preview::Check;
        if let Some(diff) = outcome.diff.as_ref().filter(|_| !check) {
            then(&mut self.status, || self.diffs.write_all(diff.as_bytes()));
        }
        if let Some(findings) = &mut self.findings
            && !outcome.problems.is_empty()
        {
            findings.push(&FileRecord {
                path,
                // `--diff` shows the change itself, as ruff does, rather than a finding.
                kept: outcome
                    .problems
                    .into_iter()
                    .filter(|problem| check || problem.rule != Some("unformatted"))
                    .collect(),
                error: None,
                diff: outcome.diff.filter(|_| check),
            });
        }
    }

    /// Flush the diffs and findings, then print the skip notices and, for `ryl format`,
    /// ruff's closing line.
    ///
    /// # Errors
    ///
    /// Reports the first write failure.
    fn finish(mut self, stats: &DiffStats, cached: usize) -> Result<ExitCode, String> {
        then(&mut self.status, || self.diffs.flush());
        let format = self.findings.is_some();
        self.status
            .map_err(|err| write_output_error(&err))
            .and_then(|()| self.findings.map_or(Ok(()), RecordStream::finish))
            .map(|()| {
                let _ = std::io::stderr().write_all(&stats.notices);
                if format
                    && let Some(summary) =
                        format_summary(stats.changed_files, stats.unchanged + cached)
                {
                    eprintln!("{summary}");
                }
                if stats.changed_files == 0 {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::from(1)
                }
            })
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
    let (global_cfg, files, _) = collect_files(&args)?;
    warn_format_conflicts(files.iter().map(|(.., cfg, _)| cfg.as_ref()), &args);
    let settings = args.source.cache_settings();
    let mut pending = Vec::new();
    let mut entries = Vec::new();
    let mut cache_hits = Vec::new();
    for (path, base_dir, cfg, kind) in &files {
        let symlink = std::fs::symlink_metadata(path)
            .is_ok_and(|metadata| metadata.file_type().is_symlink());
        let entry = if format.diff || symlink {
            None
        } else {
            settings
                .entry(path, base_dir, cfg, *kind, "format")?
                .map(|(path, _)| path)
        };
        let hit = entry.as_deref().is_some_and(cache::clean_hit);
        cache_hits.push(hit);
        if !hit {
            pending.push((path.clone(), base_dir.clone(), Arc::clone(cfg), *kind));
            entries.push(entry);
        }
    }
    if let Some(preview) = preview {
        let output_config = if preview == Preview::Check {
            run_config_settings(global_cfg.as_ref(), &args.source.inputs, &args)?.0
        } else {
            None
        };
        let targets =
            format_targets(preview, format.output_format, output_config.as_ref())?;
        reject_input_collisions(&targets, file_paths(&files))?;
        let prefixes = cache_notice_prefixes(&cache_hits);
        let findings = RecordStream::open(&targets, file_paths(&pending))?;
        let mut out = PreviewOutput::new(preview, Some(findings));
        let stats = diff_files(
            &pending,
            (Rewrite::Format, preview.flag()),
            &prefixes,
            &mut |path, outcome| out.push(path, outcome),
        )?;
        if preview == Preview::Check
            && stats.changed_files == 0
            && stats.skipped.is_empty()
        {
            for entry in entries.into_iter().flatten() {
                cache::mark_clean(&entry)?;
            }
        }
        return out.finish(&stats, cache_hits.iter().filter(|hit| **hit).count());
    }
    let mut stats = rewrite_files(&pending, Rewrite::Format);
    for (index, entry) in entries.into_iter().enumerate() {
        if stats.errors.iter().any(|(failed, _)| *failed == index)
            || stats.skipped.contains(&index)
        {
            continue;
        }
        if entry.is_some() {
            let (path, base_dir, cfg, kind) = &pending[index];
            settings
                .entry(path, base_dir, cfg, *kind, "format")
                .and_then(|current| {
                    current
                        .as_ref()
                        .map(|(path, _)| cache::mark_clean(path))
                        .transpose()
                })?;
        }
    }
    stats.notices = ordered_cache_notices(&cache_hits, &stats.file_notices);
    let _ = std::io::stderr().write_all(&stats.notices);
    Ok(if stats.errors.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(2)
    })
}

fn ordered_cache_notices(cache_hits: &[bool], file_notices: &[Vec<u8>]) -> Vec<u8> {
    let mut pending = file_notices.iter();
    let mut ordered = Vec::new();
    for hit in cache_hits {
        if *hit {
            ordered.extend_from_slice(cache_hit_notice());
        } else {
            ordered.extend_from_slice(
                pending.next().expect("each cache miss has notices"),
            );
        }
    }
    ordered
}

fn cache_notice_prefixes(cache_hits: &[bool]) -> Vec<Vec<u8>> {
    let mut prefixes = vec![Vec::new()];
    for hit in cache_hits {
        if *hit {
            prefixes
                .last_mut()
                .expect("the first notice bucket exists")
                .extend_from_slice(cache_hit_notice());
        } else {
            prefixes.push(Vec::new());
        }
    }
    prefixes
}

fn cache_hit_notice() -> &'static [u8] {
    if std::env::var("YAMLLINT_FILE_ENCODING").is_ok() {
        decoder::ENCODING_OVERRIDE_WARNING
    } else {
        &[]
    }
}

/// Under `--check`, `--output-format` beats `[output]`; `--diff` keeps the default target,
/// as in ruff.
fn format_targets(
    preview: Preview,
    output_format: Option<CliFormat>,
    config_output: Option<&OutputTable>,
) -> Result<Vec<OutputTarget>, String> {
    let targets = match (preview, output_format) {
        (Preview::Diff, _) => config_or_default_targets(None),
        (Preview::Check, Some(choice)) => vec![default_target(choice)],
        (Preview::Check, None) => config_or_default_targets(config_output),
    };
    validate_targets(&targets, preview == Preview::Diff)?;
    Ok(targets)
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

/// ruff's closing line for a format preview.
fn format_summary(changed: usize, unchanged: usize) -> Option<String> {
    let files = |count: usize| {
        if count == 1 {
            "1 file".to_string()
        } else {
            format!("{count} files")
        }
    };
    match (changed, unchanged) {
        (0, 0) => None,
        (0, _) => Some(format!("{} already formatted", files(unchanged))),
        (_, 0) => Some(format!("{} would be reformatted", files(changed))),
        _ => Some(format!(
            "{} would be reformatted, {} already formatted",
            files(changed),
            files(unchanged)
        )),
    }
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
    if let Some(preview) = preview {
        let targets = format_targets(preview, output_format, cfg.output())?;
        reject_input_collisions(&targets, std::iter::once(path.as_path()))?;
        let findings = RecordStream::open(&targets, [path.as_path()])?;
        let mut out = PreviewOutput::new(preview, Some(findings));
        let stats = match kind {
            Some(kind) => stdin_preview(
                &path,
                &base_dir,
                &cfg,
                (kind, Rewrite::Format),
                &mut out,
            )?,
            None => DiffStats::default(),
        };
        return out.finish(&stats, 0);
    }
    let (content, raw) = read_stdin_decoded(&path, kind)??;
    let (formatted, skipped) = kind.map_or((None, Vec::new()), |kind| {
        rewrite_str(
            content.content(),
            &cfg,
            &path,
            &base_dir,
            kind,
            Rewrite::Format,
        )
    });
    for problem in &skipped {
        eprint_skip_notice(&path, problem, Rewrite::Format.flag());
    }
    std::io::stdout()
        .write_all(&formatted.map_or(raw, |formatted| content.encode(&formatted)))
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
    let flags = cli_config_flags(args);
    flags.apply(&mut cfg, &ctx.base_dir)?;
    if !apply_yaml_files {
        cfg.disable_path_based_rule_ignores();
    }
    Ok((
        path,
        ctx.base_dir,
        cfg,
        apply_yaml_files,
        ctx.config_found || flags.enable.is_some(),
    ))
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

/// Lint `files` in parallel, handing each outcome to `emit` in input order.
fn lint_files(files: &[LintFile], mut emit: impl FnMut(usize, (LintOutcome, Vec<u8>))) {
    let Ok(()) = ryl::cli_support::try_for_each_ordered(
        files,
        |(path, base_dir, cfg, kind)| match kind {
            SourceKind::Markdown => lint_markdown_file(path, cfg, base_dir),
            SourceKind::Yaml => lint_file(path, cfg, base_dir),
        },
        |idx, result| {
            emit(idx, result);
            Ok::<(), std::convert::Infallible>(())
        },
    );
}

/// Cache misses stored per batch file, so the pending entries stay bounded.
const CACHE_BATCH: usize = 256;

/// As [`lint_files`], serving hits from the lint cache and storing misses.
///
/// # Errors
///
/// Propagates the first cache error.
fn lint_files_cached(
    files: &[LintFile],
    settings: &cache::Settings,
    mut emit: impl FnMut(usize, (LintOutcome, Vec<u8>)),
) -> Result<(), String> {
    let compute = |(path, base_dir, cfg, kind): &LintFile| -> Result<_, String> {
        let entry = settings.entry(path, base_dir, cfg, *kind, "check")?;
        if let Some(problems) =
            entry.as_ref().and_then(|(path, _)| settings.lint_hit(path))
        {
            warn_cached_encoding_override();
            return Ok((Ok(problems), None));
        }
        let outcome = match entry.as_ref() {
            Some((_, bytes)) => ryl::decoder::decode_bytes(bytes)
                .map_err(|err| format!("failed to read {}: {err}", path.display()))
                .map(|content| match kind {
                    SourceKind::Markdown => {
                        lint_markdown_str(&content, path, cfg, base_dir)
                    }
                    SourceKind::Yaml => {
                        lint_str_with_repair_help(&content, path, cfg, base_dir)
                    }
                }),
            None => match kind {
                SourceKind::Markdown => lint_markdown_file(path, cfg, base_dir),
                SourceKind::Yaml => lint_file(path, cfg, base_dir),
            },
        };
        Ok((outcome, entry.map(|(path, _)| path)))
    };
    let mut misses: Vec<(PathBuf, Vec<LintProblem>)> = Vec::new();
    let store = |misses: &mut Vec<(PathBuf, Vec<LintProblem>)>| {
        cache::store_lint_batch(
            std::mem::take(misses)
                .iter()
                .map(|(path, problems)| (path.as_path(), problems.as_slice())),
        )
    };
    let mut stored = Ok(());
    ryl::cli_support::try_for_each_ordered(files, compute, |idx, (result, stderr)| {
        let (outcome, entry) = result?;
        if let (Some(entry), Ok(problems)) = (entry, &outcome) {
            misses.push((entry, problems.clone()));
            if misses.len() == CACHE_BATCH {
                stored = std::mem::replace(&mut stored, Ok(()))
                    .and_then(|()| store(&mut misses));
            }
        }
        emit(idx, (outcome, stderr));
        Ok::<(), String>(())
    })?;
    stored.and_then(|()| store(&mut misses))
}

fn warn_cached_encoding_override() {
    if std::env::var("YAMLLINT_FILE_ENCODING").is_ok() {
        ryl::decoder::warn_encoding_override();
    }
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
        if explicit && kind == SourceKind::Yaml && has_jinja_header(path) {
            return Err(jinja_template_error(path));
        }
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
/// (mutually exclusive; a clean file has neither). Every target renders the same record, so
/// the filter+tally pass runs once.
struct FileRecord<'a> {
    path: &'a Path,
    kept: Vec<LintProblem>,
    error: Option<String>,
    /// The diff `ryl format --check` shows under the file's finding.
    diff: Option<String>,
}

type LintOutcome = Result<Vec<LintProblem>, String>;

/// The tally behind the exit code, independent of which formats render the records.
#[derive(Default)]
struct LintSummary {
    has_error: bool,
    has_warning: bool,
    problem_count: usize,
}

impl LintSummary {
    /// Filter and tally one lint outcome into its [`FileRecord`]. `no_warnings` drops
    /// warning-level diagnostics before they are kept or counted.
    fn record<'a>(
        &mut self,
        path: &'a Path,
        outcome: LintOutcome,
        no_warnings: bool,
    ) -> FileRecord<'a> {
        let (kept, error) = match outcome {
            // A crafted filename could otherwise inject terminal escapes or, via a newline, a
            // GitHub workflow command.
            Err(message) => {
                self.has_error = true;
                self.problem_count += 1;
                (Vec::new(), Some(sanitize_control(&message).into_owned()))
            }
            Ok(mut kept) => {
                kept.retain(|problem| {
                    !(no_warnings && problem.level == Severity::Warning)
                });
                for problem in &kept {
                    match problem.level {
                        Severity::Error => self.has_error = true,
                        Severity::Warning => self.has_warning = true,
                    }
                }
                self.problem_count += kept.len();
                (kept, None)
            }
        };
        FileRecord {
            path,
            kept,
            error,
            diff: None,
        }
    }
}

type SafeFixRuleCounts = [usize; SAFE_FIX_RULE_IDS.len()];

/// Each file's pre-fix safe-fix counts, and the files `--fix` failed to write.
type FixBaseline = (Vec<SafeFixRuleCounts>, Vec<(usize, String)>);

/// A file's reported problems per safe-fix rule, so a fix that adds another rule's problem
/// cannot cancel one it cleared.
fn safe_fix_rule_counts(outcome: &LintOutcome, no_warnings: bool) -> SafeFixRuleCounts {
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
