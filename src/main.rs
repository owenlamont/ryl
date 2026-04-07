#![forbid(unsafe_code)]
#![deny(
    clippy::all,
    clippy::pedantic,
    clippy::cargo,
    clippy::cognitive_complexity
)]

use std::collections::HashMap;
use std::collections::HashSet;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, ValueEnum};
use ignore::WalkBuilder;
use rayon::prelude::*;
use ryl::cli_support::resolve_ctx;
use ryl::config::{ConfigContext, Overrides, YamlLintConfig, discover_config};
use ryl::fix::apply_safe_fixes_to_files;
use ryl::migrate::{
    MigrateOptions, OutputMode as MigrateOutputMode, SourceCleanup, WriteMode,
    migrate_configs,
};
use ryl::{LintProblem, Severity, lint_file};

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

fn build_global_cfg(
    inputs: &[PathBuf],
    cli: &Cli,
) -> Result<Option<ConfigContext>, String> {
    if cli.config_data.is_some()
        || cli.config_file.is_some()
        || std::env::var("YAMLLINT_CONFIG_FILE").is_ok()
    {
        let config_data = cli.config_data.as_ref().map(|raw| {
            if !raw.is_empty() && !raw.contains(':') {
                format!("extends: {raw}")
            } else {
                raw.clone()
            }
        });
        discover_config(
            inputs,
            &Overrides {
                config_file: cli.config_file.clone(),
                config_data,
            },
        )
        .map(Some)
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
    /// One or more paths: files and/or directories
    #[arg(value_name = "PATH_OR_FILE")]
    inputs: Vec<PathBuf>,

    /// Path to configuration file (YAML or TOML)
    #[arg(short = 'c', long = "config-file", value_name = "FILE")]
    config_file: Option<PathBuf>,

    /// Inline configuration data (yaml)
    #[arg(short = 'd', long = "config-data", value_name = "YAML")]
    config_data: Option<String>,

    /// Output format (auto, standard, colored, github, parsable)
    #[arg(short = 'f', long = "format", default_value_t = CliFormat::Auto, value_enum)]
    format: CliFormat,

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
    if cli.migrate_configs {
        return run_migration(&cli);
    }

    run_lint(cli)
}

fn run_lint(cli: Cli) -> Result<ExitCode, String> {
    if cli.inputs.is_empty() {
        return Err(
            "error: expected one or more paths (files and/or directories)".to_string(),
        );
    }

    // Build a global config if -d/-c provided or env var set; else None for per-file discovery.
    let global_cfg = build_global_cfg(&cli.inputs, &cli)?;
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
    let mut files: Vec<(PathBuf, PathBuf, YamlLintConfig)> = Vec::new();
    gather_lint_files(
        &candidates,
        &explicit_files,
        global_cfg.as_ref(),
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

    if cli.lint.fix.fix {
        if let Err(err) = apply_safe_fixes_to_files(&files) {
            return Err(err);
        }
    }

    let mut results: Vec<(usize, Result<Vec<LintProblem>, String>)> = files
        .par_iter()
        .enumerate()
        .map(|(idx, (path, base_dir, cfg))| (idx, lint_file(path, cfg, base_dir)))
        .collect();

    results.sort_by_key(|(idx, _)| *idx);

    let output_format = detect_output_format(cli.format);
    let (has_error, has_warning) = process_results(
        &files,
        results,
        output_format,
        cli.lint.compatibility.no_warnings,
    );

    if has_error {
        Ok(ExitCode::from(1))
    } else if has_warning && cli.lint.compatibility.strict {
        Ok(ExitCode::from(2))
    } else {
        Ok(ExitCode::SUCCESS)
    }
}

#[allow(clippy::too_many_arguments)]
fn gather_lint_files(
    candidates: &[PathBuf],
    explicit_files: &[PathBuf],
    global_cfg: Option<&ConfigContext>,
    cache: &mut HashMap<PathBuf, (PathBuf, YamlLintConfig)>,
    emitted_notices: &mut HashSet<String>,
    files: &mut Vec<(PathBuf, PathBuf, YamlLintConfig)>,
) -> Result<(), String> {
    for f in candidates {
        let (base_dir, cfg, notices) = resolve_ctx(f, global_cfg, cache)?;
        for notice in notices {
            if emitted_notices.insert(notice.clone()) {
                eprintln!("{notice}");
            }
        }
        let ignored = cfg.is_file_ignored(f, &base_dir);
        let yaml_ok = cfg.is_yaml_candidate(f, &base_dir);
        if !ignored && yaml_ok {
            files.push((f.clone(), base_dir, cfg));
        }
    }

    for ef in explicit_files {
        let (base_dir, cfg, notices) = resolve_ctx(ef, global_cfg, cache)?;
        for notice in notices {
            if emitted_notices.insert(notice.clone()) {
                eprintln!("{notice}");
            }
        }
        let ignored = cfg.is_file_ignored(ef, &base_dir);
        let yaml_ok = cfg.is_yaml_candidate(ef, &base_dir);
        if !ignored && yaml_ok {
            files.push((ef.clone(), base_dir, cfg));
        }
    }

    Ok(())
}

fn process_results(
    files: &[(PathBuf, PathBuf, YamlLintConfig)],
    results: Vec<(usize, Result<Vec<LintProblem>, String>)>,
    output_format: OutputFormat,
    no_warnings: bool,
) -> (bool, bool) {
    let mut has_error = false;
    let mut has_warning = false;

    for (idx, outcome) in results {
        let (path, ..) = &files[idx];
        match outcome {
            Err(message) => {
                eprintln!("{message}");
                has_error = true;
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
                        eprintln!("{}", path.display());
                        for problem in problems {
                            eprintln!("{}", format_standard(problem));
                            match problem.level {
                                Severity::Error => has_error = true,
                                Severity::Warning => has_warning = true,
                            }
                        }
                        eprintln!();
                    }
                    OutputFormat::Colored => {
                        eprintln!("\u{001b}[4m{}\u{001b}[0m", path.display());
                        for problem in problems {
                            eprintln!("{}", format_colored(problem));
                            match problem.level {
                                Severity::Error => has_error = true,
                                Severity::Warning => has_warning = true,
                            }
                        }
                        eprintln!();
                    }
                    OutputFormat::Github => {
                        eprintln!("::group::{}", path.display());
                        for problem in problems {
                            eprintln!("{}", format_github(problem, path));
                            match problem.level {
                                Severity::Error => has_error = true,
                                Severity::Warning => has_warning = true,
                            }
                        }
                        eprintln!("::endgroup::");
                        eprintln!();
                    }
                    OutputFormat::Parsable => {
                        for problem in problems {
                            eprintln!("{}", format_parsable(problem, path));
                            match problem.level {
                                Severity::Error => has_error = true,
                                Severity::Warning => has_warning = true,
                            }
                        }
                    }
                }
            }
        }
    }

    (has_error, has_warning)
}

fn format_standard(problem: &LintProblem) -> String {
    let mut line = format!("  {}:{}", problem.line, problem.column);
    line.push_str(&" ".repeat(12usize.saturating_sub(line.len())));
    line.push_str(problem.level.as_str());
    line.push_str(&" ".repeat(21usize.saturating_sub(line.len())));
    line.push_str(&problem.message);
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
    line.push_str(&problem.message);
    if let Some(rule) = problem.rule {
        line.push_str("  \u{001b}[2m(");
        line.push_str(rule);
        line.push_str(")\u{001b}[0m");
    }
    line
}

fn format_github(problem: &LintProblem, path: &Path) -> String {
    let mut line = format!(
        "::{} file={},line={},col={}::{}:{} ",
        problem.level.as_str(),
        path.display(),
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
    line.push_str(&problem.message);
    line
}

fn format_parsable(problem: &LintProblem, path: &Path) -> String {
    let mut line = format!(
        "{}:{}:{}: [{}] {}",
        path.display(),
        problem.line,
        problem.column,
        problem.level.as_str(),
        problem.message
    );
    if let Some(rule) = problem.rule {
        line.push_str(" (");
        line.push_str(rule);
        line.push(')');
    }
    line
}
