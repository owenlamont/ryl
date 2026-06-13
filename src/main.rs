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

use clap::{CommandFactory, Parser, ValueEnum};
use ignore::WalkBuilder;
use rayon::prelude::*;
use ryl::cli_support::{
    github_escape, lexical_abspath, report_display_path, resolve_ctx, sanitize_control,
};
use ryl::config::{
    ConfigContext, Overrides, SourceKind, YamlLintConfig, discover_config,
};
use ryl::config_schema::{schema_string_pretty, yaml_schema_string_pretty};
use ryl::decoder;
use ryl::fix::{
    DiffStats, apply_safe_fixes_to_files, diff_outcome, diff_safe_fixes_for_files,
};
use ryl::migrate::{
    MigrateOptions, OutputMode as MigrateOutputMode, SourceCleanup, WriteMode,
    migrate_configs,
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

fn run_migration(cli: &Cli) -> Result<ExitCode, String> {
    let cleanup = if let Some(suffix) = &cli.migrate.rename_old {
        SourceCleanup::RenameSuffix(suffix.clone())
    } else if cli.migrate.delete_old {
        SourceCleanup::Delete
    } else {
        SourceCleanup::Keep
    };
    let options = MigrateOptions {
        root: cli
            .migrate
            .root
            .clone()
            .unwrap_or_else(|| PathBuf::from(".")),
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
    for warning in result.warnings {
        eprintln!("{}", sanitize_control(&warning));
    }
    if result.entries.is_empty() {
        println!(
            "No legacy YAML config files found under {}",
            sanitize_control(&options.root.display().to_string())
        );
        return Ok(ExitCode::SUCCESS);
    }

    for entry in &result.entries {
        println!(
            "{} -> {}",
            sanitize_control(&entry.source.display().to_string()),
            sanitize_control(&entry.target.display().to_string())
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
#[command(name = "ryl", version, about = "Fast YAML linter written in Rust")]
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
    /// List files that would be linted (reserved)
    #[arg(long = "list-files", default_value_t = false)]
    list_files: bool,

    /// Strict mode (reserved)
    #[arg(short = 's', long = "strict", default_value_t = false)]
    strict: bool,

    /// Suppress warnings (reserved)
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

    /// Write migrated .ryl.toml files (otherwise preview only)
    #[arg(
        long = "migrate-write",
        default_value_t = false,
        requires = "migrate_configs"
    )]
    write: bool,

    /// Print generated TOML to stdout during migration
    #[arg(
        long = "migrate-stdout",
        default_value_t = false,
        requires = "migrate_configs"
    )]
    stdout: bool,

    /// Rename source YAML configs by appending this suffix after migration
    #[arg(
        long = "migrate-rename-old",
        value_name = "SUFFIX",
        conflicts_with = "delete_old",
        requires_all = ["write", "migrate_configs"]
    )]
    rename_old: Option<String>,

    /// Delete source YAML configs after migration
    #[arg(
        long = "migrate-delete-old",
        default_value_t = false,
        conflicts_with = "rename_old",
        requires_all = ["write", "migrate_configs"]
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

// TODO(#285): temporary bridge while `--format`/`-o` are repeatable but the multi-target
// resolution is not wired yet — collapse to the last occurrence (current single-output
// behavior). Replaced by the (format -> destination) target resolution.
fn primary_format(formats: &[CliFormat]) -> CliFormat {
    formats.last().copied().unwrap_or(CliFormat::Auto)
}

fn primary_output_file(files: &[PathBuf]) -> Option<&Path> {
    files.last().map(PathBuf::as_path)
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

/// Open the destination for formatted output: the `--output-file` path when given,
/// otherwise stdout for the whole-document report formats (so they can be redirected or
/// piped as an artifact) and stderr for the streaming console formats (unchanged
/// behavior). The console diagnostics are the only thing routed here; notices, `--fix`
/// summaries, and skip messages always stay on stderr.
///
/// # Errors
///
/// Returns an error if the `--output-file` path cannot be created.
fn open_output_sink(
    output_file: Option<&Path>,
    format: OutputFormat,
) -> Result<Box<dyn Write>, String> {
    if let Some(path) = output_file {
        let file = File::create(path).map_err(|err| {
            format!(
                "error: cannot open --output-file {}: {err}",
                sanitize_control(&path.display().to_string())
            )
        })?;
        return Ok(Box::new(BufWriter::new(file)));
    }
    if format.is_streaming() {
        Ok(Box::new(std::io::stderr()))
    } else {
        Ok(Box::new(BufWriter::new(std::io::stdout())))
    }
}

/// `--diff` previews safe fixes and ignores `--format`, so combining it with a
/// whole-document report format is a usage error rather than a silent no-op. (`--diff`
/// with `--output-file` is rejected declaratively by clap's `conflicts_with`.)
///
/// # Errors
///
/// Returns a usage error when `--diff` is combined with `--format junit|gitlab`.
fn reject_diff_with_report_format(
    diff: bool,
    format: OutputFormat,
) -> Result<(), String> {
    if diff && !format.is_streaming() {
        return Err(
            "error: `--diff` cannot be combined with `--format junit` or `--format gitlab`"
                .to_string(),
        );
    }
    Ok(())
}

/// Emit a valid empty report for the whole-document formats when there are no files to
/// lint, so CI artifact ingestion sees a present, valid file. Streaming formats emit
/// nothing (unchanged).
///
/// # Errors
///
/// Returns an error if the `--output-file` cannot be created or written.
fn emit_empty_report(
    output_file: Option<&Path>,
    output_format: OutputFormat,
) -> Result<(), String> {
    // A streaming format writes nothing to its default stream for an empty input, but an
    // explicit --output-file is still created (empty) and validated, so `-o` uniformly
    // produces the file and an unopenable destination still errors.
    if output_file.is_none() && output_format.is_streaming() {
        return Ok(());
    }
    let mut sink = open_output_sink(output_file, output_format)?;
    let (_summary, output) = process_results(&[], Vec::new(), output_format, false);
    write_output(sink.as_mut(), &output)
}

/// Refuse an `--output-file` whose lexical path matches a linted input, so a report can
/// never truncate the source it just linted (or, with `--fix`, the freshly-fixed file).
/// Matches on the lexical identity (`lexical_abspath`) used for input de-duplication —
/// which also covers a not-yet-created destination — and, for a destination that already
/// exists, on its underlying file identity via `same_file::Handle`, so an `-o` that is a
/// symlink *or* a hard link onto a linted input is caught too.
///
/// # Errors
///
/// Returns a usage error when `output` resolves to one of the `inputs` (for stdin, the
/// single `--stdin-filename`/label entry). `output` is the clap-parsed `--output-file`,
/// which clap guarantees is non-empty, so `lexical_abspath` never sees an empty path.
fn reject_output_file_collision<'a>(
    output: &Path,
    inputs: impl Iterator<Item = &'a Path>,
) -> Result<(), String> {
    let output_abs = lexical_abspath(output);
    let output_handle = Handle::from_path(output).ok();
    let collides = inputs.into_iter().any(|input| {
        lexical_abspath(input) == output_abs
            || output_handle.as_ref().is_some_and(|handle| {
                Handle::from_path(input).ok().as_ref() == Some(handle)
            })
    });
    if collides {
        return Err(format!(
            "error: --output-file {} is also a linted input; refusing to overwrite it",
            sanitize_control(&output.display().to_string())
        ));
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

/// Write the buffered, formatted output to its destination: the single fallible step of
/// the output pipeline (the formatting itself is infallible, see [`OUTPUT_INFALLIBLE`]).
///
/// # Errors
///
/// Returns an error if the destination cannot be written or flushed (e.g. a full disk).
fn write_output(sink: &mut dyn Write, output: &[u8]) -> Result<(), String> {
    sink.write_all(output)
        .and_then(|()| sink.flush())
        .map_err(|err| write_output_error(&err))
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
    let cli = Cli::parse();

    match run_cli(&cli) {
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

fn run_cli(cli: &Cli) -> Result<ExitCode, String> {
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

    if cli.migrate_configs {
        return run_migration(cli);
    }

    run_lint(cli)
}

fn run_lint(cli: &Cli) -> Result<ExitCode, String> {
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
        return run_stdin_lint(cli);
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

    let output_format = detect_output_format(primary_format(&cli.format));
    reject_diff_with_report_format(cli.lint.fix.diff, output_format)?;

    // Refuse an --output-file that resolves to a linted input: writing the report there
    // would truncate the source we just linted (and, with --fix, the freshly-fixed file).
    if let Some(output) = primary_output_file(&cli.output_file) {
        reject_output_file_collision(
            output,
            files.iter().map(|(path, ..)| path.as_path()),
        )?;
    }

    if files.is_empty() {
        // A clean or fully-ignored project still gets a valid empty report, so CI artifact
        // ingestion sees `[]` / `<testsuites .../>` rather than a missing or invalid file.
        emit_empty_report(primary_output_file(&cli.output_file), output_format)?;
        return Ok(ExitCode::SUCCESS);
    }

    if let Some(config_found) = ruleless_config_found {
        return Err(no_rules_error(config_found));
    }

    if cli.lint.fix.diff {
        return Ok(emit_diff(&diff_safe_fixes_for_files(&files)?));
    }

    lint_and_exit(&files, cli, output_format)
}

/// Open the output destination (before `--fix` mutates anything, so an unopenable
/// `--output-file` fails fast), apply fixes if requested, lint, render the chosen format,
/// write it, print the `--fix` summary, and map the tally to an exit code.
///
/// # Errors
///
/// Returns an error if a file cannot be read/written or the output destination fails.
fn lint_and_exit(
    files: &[(PathBuf, PathBuf, YamlLintConfig, SourceKind)],
    cli: &Cli,
    output_format: OutputFormat,
) -> Result<ExitCode, String> {
    let mut sink =
        open_output_sink(primary_output_file(&cli.output_file), output_format)?;

    let initial_problem_count = if cli.lint.fix.fix {
        apply_fixes_reporting_skips(files, cli.lint.compatibility.no_warnings)?
    } else {
        0
    };

    let results = lint_files(files);
    let (summary, output) = process_results(
        files,
        results,
        output_format,
        cli.lint.compatibility.no_warnings,
    );
    write_output(sink.as_mut(), &output)?;

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
        eprintln!(
            "{}:{}:{} skipped by --fix: {}",
            sanitize_control(&path.display().to_string()),
            problem.line,
            problem.column,
            sanitize_control(&problem.message),
        );
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

fn run_stdin_lint(cli: &Cli) -> Result<ExitCode, String> {
    let (path, base_dir, cfg, apply_yaml_files, config_found) = resolve_stdin_ctx(cli)?;
    let output_format = detect_output_format(primary_format(&cli.format));
    reject_diff_with_report_format(cli.lint.fix.diff, output_format)?;

    // The stdin content is labelled by `--stdin-filename`; refuse writing the report to
    // that same path so it cannot truncate the file the label names.
    if let Some(output) = primary_output_file(&cli.output_file) {
        reject_output_file_collision(output, std::iter::once(path.as_path()))?;
    }

    let Some(kind) = resolve_stdin_kind(cli, &cfg, &path, &base_dir, apply_yaml_files)?
    else {
        // An ignored stdin filename is an empty input set: still emit a valid empty
        // report so CI artifact ingestion does not see a missing file.
        emit_empty_report(primary_output_file(&cli.output_file), output_format)?;
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

    let mut sink =
        open_output_sink(primary_output_file(&cli.output_file), output_format)?;
    let (summary, output) = process_results(
        &files,
        results,
        output_format,
        cli.lint.compatibility.no_warnings,
    );
    write_output(sink.as_mut(), &output)?;
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
        eprintln!(
            "{}:{}:{} skipped by --diff: {}",
            sanitize_control(&path.display().to_string()),
            problem.line,
            problem.column,
            sanitize_control(&problem.message),
        );
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
    for f in candidates {
        let (base_dir, cfg, notices, found) =
            resolve_ctx(f, global_cfg, markdown, cache)?;
        for notice in notices {
            if emitted_notices.insert(notice.clone()) {
                eprintln!("{}", sanitize_control(notice.as_str()));
            }
        }
        if cfg.is_file_ignored(f, &base_dir) {
            continue;
        }
        if let Some(kind) = cfg.source_kind(f, &base_dir)? {
            if !seen.insert(lexical_abspath(f)) {
                continue;
            }
            if !cfg.enables_any_rule() && ruleless_config_found.is_none() {
                ruleless_config_found = Some(found);
            }
            files.push((f.clone(), base_dir, cfg, kind));
        }
    }

    for ef in explicit_files {
        let (base_dir, cfg, notices, found) =
            resolve_ctx(ef, global_cfg, markdown, cache)?;
        for notice in notices {
            if emitted_notices.insert(notice.clone()) {
                eprintln!("{}", sanitize_control(notice.as_str()));
            }
        }
        if cfg.is_file_ignored(ef, &base_dir) {
            continue;
        }
        match cfg.source_kind(ef, &base_dir)? {
            Some(kind) => {
                if !seen.insert(lexical_abspath(ef)) {
                    continue;
                }
                if !cfg.enables_any_rule() && ruleless_config_found.is_none() {
                    ruleless_config_found = Some(found);
                }
                files.push((ef.clone(), base_dir, cfg, kind));
            }
            None => {
                return Err(format!(
                    "{}: no source kind matches; add a matching glob under \
                     [files].yaml or [files].markdown",
                    ef.display()
                ));
            }
        }
    }

    Ok(ruleless_config_found)
}

/// Filter, tally, and format diagnostics into an owned buffer. Streaming formats append
/// one line per diagnostic as files are visited; the whole-document formats (junit/gitlab)
/// buffer every file into a [`ReportEntry`] and serialize once after the loop. The buffer
/// is written to its destination by the caller (a single fallible step); the returned
/// [`LintSummary`] (and thus the exit code) is identical across all formats. Output is
/// built in memory so the per-diagnostic writes cannot fail (see [`OUTPUT_INFALLIBLE`]).
///
/// Buffering rather than streaming the console formats is not a regression: `lint_files`
/// has already collected every result (in parallel) before this runs, so output only ever
/// appears after linting completes — the buffer just emits the same bytes in one write.
fn process_results(
    files: &[(PathBuf, PathBuf, YamlLintConfig, SourceKind)],
    results: Vec<(usize, Result<Vec<LintProblem>, String>)>,
    output_format: OutputFormat,
    no_warnings: bool,
) -> (LintSummary, Vec<u8>) {
    let mut summary = LintSummary::default();
    let mut entries: Vec<ReportEntry> = Vec::new();
    let mut out: Vec<u8> = Vec::new();
    let project_root = report_project_root();

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
                if output_format.is_streaming() {
                    writeln!(out, "{message}").expect(OUTPUT_INFALLIBLE);
                } else {
                    entries.push(ReportEntry {
                        path: report_display_path(path, &project_root),
                        problems: Vec::new(),
                        error: Some(message),
                    });
                }
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

                // Streaming formats print nothing for a file with no reportable
                // diagnostics; junit still records it as a passing test, so the
                // whole-document formats keep the (possibly empty) entry.
                if output_format.is_streaming() && kept.is_empty() {
                    continue;
                }

                match output_format {
                    OutputFormat::Standard => append_standard(&mut out, path, &kept),
                    OutputFormat::Colored => append_colored(&mut out, path, &kept),
                    OutputFormat::Github => append_github(&mut out, path, &kept),
                    OutputFormat::Parsable => append_parsable(&mut out, path, &kept),
                    OutputFormat::Junit | OutputFormat::Gitlab => {
                        entries.push(ReportEntry {
                            path: report_display_path(path, &project_root),
                            problems: kept,
                            error: None,
                        });
                    }
                }
            }
        }
    }

    match output_format {
        OutputFormat::Junit => out = render_junit(&entries),
        OutputFormat::Gitlab => out = render_gitlab(&entries),
        _ => {}
    }

    (summary, out)
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
