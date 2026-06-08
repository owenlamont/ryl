#![forbid(unsafe_code)]
#![deny(
    clippy::all,
    clippy::pedantic,
    clippy::cargo,
    clippy::cognitive_complexity
)]

use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt::Write as _;
use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, ValueEnum};
use ignore::WalkBuilder;
use rayon::prelude::*;
use ryl::cli_support::{resolve_ctx, sanitize_control};
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
use ryl::{
    LintProblem, Severity, lint_file, lint_markdown_file, lint_markdown_str, lint_str,
};

const STDIN_LABEL: &str = "<stdin>";

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

    /// Output format (auto, standard, colored, github, parsable)
    #[arg(short = 'f', long = "format", default_value_t = CliFormat::Auto, value_enum)]
    format: CliFormat,

    /// Print the JSON Schema for ryl TOML config and exit
    #[arg(long = "print-toml-config-schema", default_value_t = false)]
    print_toml_config_schema: bool,

    /// Print the JSON Schema for yamllint-compatible YAML config and exit
    #[arg(long = "print-yaml-config-schema", default_value_t = false)]
    print_yaml_config_schema: bool,

    /// Convert discovered legacy YAML config files into .ryl.toml files
    #[arg(long = "migrate-configs", default_value_t = false)]
    migrate_configs: bool,

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
}

fn detect_output_format(choice: CliFormat) -> OutputFormat {
    match choice {
        CliFormat::Standard => OutputFormat::Standard,
        CliFormat::Colored => OutputFormat::Colored,
        CliFormat::Github => OutputFormat::Github,
        CliFormat::Parsable => OutputFormat::Parsable,
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

    match run_cli(cli) {
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

fn run_cli(cli: Cli) -> Result<ExitCode, String> {
    if cli.print_toml_config_schema {
        println!("{}", schema_string_pretty());
        return Ok(ExitCode::SUCCESS);
    }

    if cli.print_yaml_config_schema {
        println!("{}", yaml_schema_string_pretty());
        return Ok(ExitCode::SUCCESS);
    }

    if cli.migrate_configs {
        return run_migration(&cli);
    }

    run_lint(cli)
}

fn run_lint(cli: Cli) -> Result<ExitCode, String> {
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
        return run_stdin_lint(&cli);
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
    let mut global_cfg = build_global_cfg(&cli.inputs, &cli)?;
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
    let inputs = cli.inputs;

    // Determine files to parse from mixed inputs.
    // - Directories: recursively gather only .yml/.yaml
    // - Files: include as-is (even if extension isn't yaml)
    let (candidates, explicit_files) = gather_inputs(&inputs);

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

    if files.is_empty() {
        return Ok(ExitCode::SUCCESS);
    }

    if let Some(config_found) = ruleless_config_found {
        return Err(no_rules_error(config_found));
    }

    if cli.lint.fix.diff {
        return Ok(emit_diff(&diff_safe_fixes_for_files(&files)?));
    }

    let initial_problem_count = if cli.lint.fix.fix {
        apply_fixes_reporting_skips(&files, cli.lint.compatibility.no_warnings)?
    } else {
        0
    };

    let results = lint_files(&files);

    let output_format = detect_output_format(cli.format);
    let summary = process_results(
        &files,
        results,
        output_format,
        cli.lint.compatibility.no_warnings,
    );

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

    let Some(kind) = resolve_stdin_kind(cli, &cfg, &path, &base_dir, apply_yaml_files)?
    else {
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

    let output_format = detect_output_format(cli.format);
    let summary = process_results(
        &files,
        results,
        output_format,
        cli.lint.compatibility.no_warnings,
    );
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

/// Identity used to de-duplicate inputs so a file reached by two spellings is linted
/// once — e.g. `ryl . f.yaml` lists `f.yaml` via both the directory walk (`./f.yaml`)
/// and the explicit arg (`f.yaml`), and `ryl f.yaml f.yaml` lists it twice. `absolute`
/// normalizes `.`/relative segments lexically *without* resolving symlinks (so a
/// symlink and its target stay distinct, preserving the `--fix`/`--diff` symlink skip)
/// and without requiring the path to exist. It is only called on paths that already
/// matched a source kind, which are non-empty, so `absolute` cannot fail here.
fn canonical_input(path: &Path) -> PathBuf {
    std::path::absolute(path)
        .expect("a source-kind-matched input path is absolutizable")
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
            if !seen.insert(canonical_input(f)) {
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
                if !seen.insert(canonical_input(ef)) {
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

fn process_results(
    files: &[(PathBuf, PathBuf, YamlLintConfig, SourceKind)],
    results: Vec<(usize, Result<Vec<LintProblem>, String>)>,
    output_format: OutputFormat,
    no_warnings: bool,
) -> LintSummary {
    let mut summary = LintSummary::default();

    for (idx, outcome) in results {
        let (path, ..) = &files[idx];
        match outcome {
            Err(message) => {
                // Error messages embed user-controlled paths/values; sanitize so a
                // crafted filename cannot inject terminal escapes or (via a newline)
                // a GitHub workflow command. `sanitize_control` is safe for every
                // format because it neutralises the newline that injection needs.
                eprintln!("{}", sanitize_control(&message));
                summary.has_error = true;
                summary.problem_count += 1;
            }
            Ok(diagnostics) => {
                let mut problems = diagnostics
                    .iter()
                    .filter(|problem| {
                        !(no_warnings && problem.level == Severity::Warning)
                    })
                    .peekable();

                if problems.peek().is_none() {
                    continue;
                }

                match output_format {
                    OutputFormat::Standard => {
                        eprintln!("{}", sanitize_control(&path.display().to_string()));
                        for problem in problems {
                            eprintln!("{}", format_standard(problem));
                            match problem.level {
                                Severity::Error => summary.has_error = true,
                                Severity::Warning => summary.has_warning = true,
                            }
                            summary.problem_count += 1;
                        }
                        eprintln!();
                    }
                    OutputFormat::Colored => {
                        eprintln!(
                            "\u{001b}[4m{}\u{001b}[0m",
                            sanitize_control(&path.display().to_string())
                        );
                        for problem in problems {
                            eprintln!("{}", format_colored(problem));
                            match problem.level {
                                Severity::Error => summary.has_error = true,
                                Severity::Warning => summary.has_warning = true,
                            }
                            summary.problem_count += 1;
                        }
                        eprintln!();
                    }
                    OutputFormat::Github => {
                        let path_str = path.display().to_string();
                        eprintln!("::group::{}", github_escape(&path_str, false));
                        let escaped_file = github_escape(&path_str, true);
                        for problem in problems {
                            eprintln!("{}", format_github(problem, &escaped_file));
                            match problem.level {
                                Severity::Error => summary.has_error = true,
                                Severity::Warning => summary.has_warning = true,
                            }
                            summary.problem_count += 1;
                        }
                        eprintln!("::endgroup::");
                        eprintln!();
                    }
                    OutputFormat::Parsable => {
                        let sanitized_path =
                            sanitize_control(&path.display().to_string()).into_owned();
                        for problem in problems {
                            eprintln!("{}", format_parsable(problem, &sanitized_path));
                            match problem.level {
                                Severity::Error => summary.has_error = true,
                                Severity::Warning => summary.has_warning = true,
                            }
                            summary.problem_count += 1;
                        }
                    }
                }
            }
        }
    }

    summary
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

// User-controlled text (a quoted key, an anchor name, a filename) reaches GitHub
// Actions workflow-command output, where a raw newline would start a new
// `::command::` — a command-injection vector in CI. Encode it the way GitHub's
// `@actions/core` does (data escapes `%`/CR/LF; a `property` such as `file=` also
// escapes `:`/`,`), and additionally render any other control character as a
// literal `\u{..}` — never a `%XX`, which the runner would decode back into the raw
// control char and let it drive ANSI sequences in the log viewer.
fn github_escape(value: &str, property: bool) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '%' => out.push_str("%25"),
            '\r' => out.push_str("%0D"),
            '\n' => out.push_str("%0A"),
            ':' if property => out.push_str("%3A"),
            ',' if property => out.push_str("%2C"),
            c if c.is_control() => {
                write!(out, "\\u{{{:x}}}", c as u32)
                    .expect("writing to a String is infallible");
            }
            c => out.push(c),
        }
    }
    out
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
