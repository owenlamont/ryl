#![forbid(unsafe_code)]
#![deny(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use rayon::prelude::*;
use ryl::config::{Overrides, YamlLintConfig, discover_config, discover_per_file};
use ryl::{gather_yaml_from_dir, parse_yaml_file};

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

    /// Output format (reserved)
    #[arg(short = 'f', long = "format", value_name = "FORMAT")]
    format: Option<String>,

    /// Strict mode (reserved)
    #[arg(short = 's', long = "strict", default_value_t = false)]
    strict: bool,

    /// Suppress warnings (reserved)
    #[arg(long = "no-warnings", default_value_t = false)]
    no_warnings: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    if cli.inputs.is_empty() {
        eprintln!("error: expected one or more paths (files and/or directories)");
        return ExitCode::from(2);
    }

    let inputs = cli.inputs;

    // Build a global config if -d/-c provided or env var set; else None for per-file discovery.
    let global_cfg = if inputs.is_empty() {
        None
    } else if cli.config_data.is_some()
        || cli.config_file.is_some()
        || std::env::var("YAMLLINT_CONFIG_FILE").is_ok()
    {
        Some(
            discover_config(
                &inputs,
                &Overrides {
                    config_file: cli.config_file.clone(),
                    config_data: cli.config_data.clone(),
                },
            )
            .unwrap_or_else(|e| {
                eprintln!("{e}");
                std::process::exit(2);
            }),
        )
    } else {
        None
    };

    // Determine files to parse from mixed inputs.
    // - Directories: recursively gather only .yml/.yaml
    // - Files: include as-is (even if extension isn't yaml)
    let mut explicit_files: Vec<PathBuf> = Vec::new();
    let mut candidates: Vec<PathBuf> = Vec::new();
    for p in inputs {
        if p.is_dir() {
            candidates.extend(gather_yaml_from_dir(&p));
        } else {
            explicit_files.push(p);
        }
    }

    // Filter directory candidates via ignores, respecting global vs per-file behavior.
    let mut cache: HashMap<PathBuf, (PathBuf, YamlLintConfig)> = HashMap::new();
    let mut filtered: Vec<PathBuf> = Vec::new();
    for f in candidates {
        let ignored = global_cfg.as_ref().map_or_else(
            || {
                let start = f.parent().map_or_else(|| PathBuf::from("."), PathBuf::from);
                let entry = cache.get(&start).cloned();
                let (base_dir, cfg) = entry.unwrap_or_else(|| {
                    let ctx = discover_per_file(&f).unwrap_or_else(|e| {
                        eprintln!("{e}");
                        std::process::exit(2);
                    });
                    let pair = (ctx.base_dir.clone(), ctx.config);
                    cache.insert(start.clone(), pair.clone());
                    pair
                });
                cfg.is_file_ignored(&f, &base_dir)
            },
            |gc| gc.config.is_file_ignored(&f, &gc.base_dir),
        );
        if !ignored {
            filtered.push(f);
        }
    }

    // Combine with explicit files (explicit files are not filtered by ignores).
    let mut files: Vec<PathBuf> = filtered;
    files.extend(explicit_files);

    if cli.list_files {
        for f in &files {
            println!("{}", f.display());
        }
        return ExitCode::SUCCESS;
    }

    if files.is_empty() {
        return ExitCode::SUCCESS;
    }

    // Parse in parallel
    let errors: Vec<String> = files
        .par_iter()
        .map(|p| parse_yaml_file(p))
        .filter_map(std::result::Result::err)
        .collect();

    if errors.is_empty() {
        ExitCode::SUCCESS
    } else {
        for e in errors {
            eprintln!("{e}");
        }
        ExitCode::from(1)
    }
}
