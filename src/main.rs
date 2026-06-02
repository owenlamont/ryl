#![forbid(unsafe_code)]
#![deny(
    clippy::all,
    clippy::pedantic,
    clippy::cargo,
    clippy::cognitive_complexity
)]

use std::borrow::Cow;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt::Write as _;
use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, ValueEnum};
use ignore::WalkBuilder;
use rayon::prelude::*;
use ryl::cli_support::resolve_ctx;
use ryl::config::{
    ConfigContext, Overrides, SourceKind, YamlLintConfig, discover_config,
};
use ryl::config_schema::{schema_string_pretty, yaml_schema_string_pretty};
use ryl::decoder;
use ryl::fix::apply_safe_fixes_to_files;
use ryl::migrate::{
    MigrateOptions, OutputMode as MigrateOutputMode, SourceCleanup, WriteMode,
    migrate_configs,
};
use ryl::{
    LintProblem, Severity, lint_file, lint_markdown_file, lint_markdown_str, lint_str,
};

const STDIN_LABEL: &str = "<stdin>";

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
        eprintln!("{warning}");
    }
    if result.entries.is_empty() {
        println!(
            "No legacy YAML config files found under {}",
            options.root.display()
        );
        return Ok(ExitCode::SUCCESS);
    }

    for entry in &result.entries {
        println!("{} -> {}", entry.source.display(), entry.target.display());
    }
    if options.output_mode == MigrateOutputMode::IncludeToml {
        for entry in &result.entries {
            println!("# {}", entry.target.display());
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
            eprintln!("{err}");
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
            eprintln!("{notice}");
        }
    }
    let inputs = cli.inputs;

    // Determine files to parse from mixed inputs.
    // - Directories: recursively gather only .yml/.yaml
    // - Files: include as-is (even if extension isn't yaml)
    let (candidates, explicit_files) = gather_inputs(&inputs);

    // Filter directory candidates via ignores, respecting global vs per-file behavior.
    let mut cache: HashMap<PathBuf, (PathBuf, YamlLintConfig)> = HashMap::new();
    let mut emitted_notices: HashSet<String> = HashSet::new();
    let mut files: Vec<(PathBuf, PathBuf, YamlLintConfig, SourceKind)> = Vec::new();
    gather_lint_files(
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
            println!("{}", path.display());
        }
        return Ok(ExitCode::SUCCESS);
    }

    if files.is_empty() {
        return Ok(ExitCode::SUCCESS);
    }

    let mut initial_problem_count = 0usize;
    if cli.lint.fix.fix {
        let initial_results = lint_files(&files);
        initial_problem_count = count_reported_problems(
            &initial_results,
            cli.lint.compatibility.no_warnings,
        );
        apply_safe_fixes_to_files(&files)?;
    }

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
    let (path, base_dir, cfg, apply_yaml_files) = resolve_stdin_ctx(cli)?;

    let Some(kind) = resolve_stdin_kind(cli, &cfg, &path, &base_dir, apply_yaml_files)?
    else {
        return Ok(ExitCode::SUCCESS);
    };

    if cli.lint.compatibility.list_files {
        println!("{}", path.display());
        return Ok(ExitCode::SUCCESS);
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

fn read_and_lint_stdin(
    path: &Path,
    base_dir: &Path,
    cfg: &YamlLintConfig,
    kind: SourceKind,
) -> Result<Vec<LintProblem>, String> {
    let mut buf = Vec::new();
    std::io::stdin()
        .read_to_end(&mut buf)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    let content = decoder::decode_bytes(&buf)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    Ok(match kind {
        SourceKind::Markdown => lint_markdown_str(&content, path, cfg, base_dir),
        SourceKind::Yaml => lint_str(&content, path, cfg, base_dir),
    })
}

fn resolve_stdin_ctx(
    cli: &Cli,
) -> Result<(PathBuf, PathBuf, YamlLintConfig, bool), String> {
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
        eprintln!("{notice}");
    }
    let mut cfg = ctx.config;
    if cli.lint.markdown {
        cfg.enable_default_markdown(&ctx.base_dir);
    }
    if !apply_yaml_files {
        cfg.disable_path_based_rule_ignores();
    }
    Ok((path, ctx.base_dir, cfg, apply_yaml_files))
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
    cache: &mut HashMap<PathBuf, (PathBuf, YamlLintConfig)>,
    emitted_notices: &mut HashSet<String>,
    files: &mut Vec<(PathBuf, PathBuf, YamlLintConfig, SourceKind)>,
) -> Result<(), String> {
    for f in candidates {
        let (base_dir, cfg, notices) = resolve_ctx(f, global_cfg, markdown, cache)?;
        for notice in notices {
            if emitted_notices.insert(notice.clone()) {
                eprintln!("{notice}");
            }
        }
        if cfg.is_file_ignored(f, &base_dir) {
            continue;
        }
        if let Some(kind) = cfg.source_kind(f, &base_dir)? {
            files.push((f.clone(), base_dir, cfg, kind));
        }
    }

    for ef in explicit_files {
        let (base_dir, cfg, notices) = resolve_ctx(ef, global_cfg, markdown, cache)?;
        for notice in notices {
            if emitted_notices.insert(notice.clone()) {
                eprintln!("{notice}");
            }
        }
        if cfg.is_file_ignored(ef, &base_dir) {
            continue;
        }
        match cfg.source_kind(ef, &base_dir)? {
            Some(kind) => files.push((ef.clone(), base_dir, cfg, kind)),
            None => {
                return Err(format!(
                    "{}: no source kind matches; add a matching glob under \
                     [files].yaml or [files].markdown",
                    ef.display()
                ));
            }
        }
    }

    Ok(())
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
                eprintln!("{message}");
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
                        eprintln!(
                            "::group::{}",
                            github_escape_data(&path.display().to_string())
                        );
                        for problem in problems {
                            eprintln!("{}", format_github(problem, path));
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
                        for problem in problems {
                            eprintln!("{}", format_parsable(problem, path));
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

/// Replace control characters — which a crafted key, value, anchor name, or
/// filename can carry into a diagnostic — with a visible `\u{..}` escape, so they
/// cannot inject terminal escape sequences or split a single-line diagnostic.
/// Printable text (including multibyte Unicode) is untouched, and the common
/// control-free case borrows without allocating.
fn sanitize_control(text: &str) -> Cow<'_, str> {
    if !text.contains(char::is_control) {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch.is_control() {
            write!(out, "\\u{{{:x}}}", ch as u32)
                .expect("writing to a String is infallible");
        } else {
            out.push(ch);
        }
    }
    Cow::Owned(out)
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
// `@actions/core` does: data (the message) escapes `%`/CR/LF; properties (the
// `file=` path) additionally escape `:` and `,`. `%` must be escaped first so the
// `%XX` sequences are not themselves re-encoded.
fn github_escape_data(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

fn github_escape_property(value: &str) -> String {
    github_escape_data(value)
        .replace(':', "%3A")
        .replace(',', "%2C")
}

fn format_github(problem: &LintProblem, path: &Path) -> String {
    let mut line = format!(
        "::{} file={},line={},col={}::{}:{} ",
        problem.level.as_str(),
        github_escape_property(&path.display().to_string()),
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
    line.push_str(&github_escape_data(&problem.message));
    line
}

fn format_parsable(problem: &LintProblem, path: &Path) -> String {
    let mut line = format!(
        "{}:{}:{}: [{}] {}",
        sanitize_control(&path.display().to_string()),
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
