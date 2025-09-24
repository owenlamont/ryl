#![forbid(unsafe_code)]
#![deny(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]

use std::collections::HashMap;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, ValueEnum};
use ignore::WalkBuilder;
use rayon::prelude::*;
use ryl::cli_support::resolve_ctx;
use ryl::config::{ConfigContext, Overrides, YamlLintConfig, discover_config};
use ryl::{LintProblem, Severity, lint_file};

fn gather_inputs(inputs: &[PathBuf]) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut explicit_files = Vec::new();
    let mut candidates = Vec::new();
    for p in inputs.iter().cloned() {
        if p.is_dir() {
            let walker = WalkBuilder::new(&p)
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
            explicit_files.push(p);
        }
    }
    (candidates, explicit_files)
}

fn build_global_cfg(inputs: &[PathBuf], cli: &Cli) -> Result<Option<ConfigContext>, String> {
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
    #[arg(value_name = "PATH_OR_FILE", num_args = 1..)]
    inputs: Vec<PathBuf>,

    /// Path to configuration file (yaml)
    #[arg(short = 'c', long = "config-file", value_name = "FILE")]
    config_file: Option<PathBuf>,

    /// Inline configuration data (yaml)
    #[arg(short = 'd', long = "config-data", value_name = "YAML")]
    config_data: Option<String>,

    /// List files that would be linted (reserved)
    #[arg(long = "list-files", default_value_t = false)]
    list_files: bool,

    /// Output format (auto, standard, colored, github, parsable)
    #[arg(short = 'f', long = "format", default_value_t = CliFormat::Auto, value_enum)]
    format: CliFormat,

    /// Strict mode (reserved)
    #[arg(short = 's', long = "strict", default_value_t = false)]
    strict: bool,

    /// Suppress warnings (reserved)
    #[arg(long = "no-warnings", default_value_t = false)]
    no_warnings: bool,
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
    std::env::var_os("GITHUB_ACTIONS").is_some() && std::env::var_os("GITHUB_WORKFLOW").is_some()
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

    if cli.inputs.is_empty() {
        eprintln!("error: expected one or more paths (files and/or directories)");
        return ExitCode::from(2);
    }

    // Build a global config if -d/-c provided or env var set; else None for per-file discovery.
    let global_cfg = match build_global_cfg(&cli.inputs, &cli) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let inputs = cli.inputs;

    // Determine files to parse from mixed inputs.
    // - Directories: recursively gather only .yml/.yaml
    // - Files: include as-is (even if extension isn't yaml)
    let (candidates, explicit_files) = gather_inputs(&inputs);

    // Filter directory candidates via ignores, respecting global vs per-file behavior.
    let mut cache: HashMap<PathBuf, (PathBuf, YamlLintConfig)> = HashMap::new();
    let mut files: Vec<(PathBuf, PathBuf, YamlLintConfig)> = Vec::new();
    for f in candidates {
        let (base_dir, cfg) = match resolve_ctx(&f, global_cfg.as_ref(), &mut cache) {
            Ok(pair) => pair,
            Err(e) => {
                eprintln!("{e}");
                return ExitCode::from(2);
            }
        };
        let ignored = cfg.is_file_ignored(&f, &base_dir);
        let yaml_ok = cfg.is_yaml_candidate(&f, &base_dir);
        if !ignored && yaml_ok {
            files.push((f, base_dir, cfg));
        }
    }

    for ef in explicit_files {
        let (base_dir, cfg) = match resolve_ctx(&ef, global_cfg.as_ref(), &mut cache) {
            Ok(pair) => pair,
            Err(e) => {
                eprintln!("{e}");
                return ExitCode::from(2);
            }
        };
        let ignored = cfg.is_file_ignored(&ef, &base_dir);
        let yaml_ok = cfg.is_yaml_candidate(&ef, &base_dir);
        if !ignored && yaml_ok {
            files.push((ef, base_dir, cfg));
        }
    }

    if cli.list_files {
        for (path, ..) in &files {
            println!("{}", path.display());
        }
        return ExitCode::SUCCESS;
    }

    if files.is_empty() {
        return ExitCode::SUCCESS;
    }

    let mut results: Vec<(usize, Result<Vec<LintProblem>, String>)> = files
        .par_iter()
        .enumerate()
        .map(|(idx, (path, base_dir, cfg))| (idx, lint_file(path, cfg, base_dir)))
        .collect();

    results.sort_by_key(|(idx, _)| *idx);

    let output_format = detect_output_format(cli.format);
    let (has_error, has_warning) = process_results(&files, results, output_format, cli.no_warnings);

    if has_error {
        ExitCode::from(1)
    } else if has_warning && cli.strict {
        ExitCode::from(2)
    } else {
        ExitCode::SUCCESS
    }
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
                    .filter(|problem| !(no_warnings && problem.level == Severity::Warning))
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
