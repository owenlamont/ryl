#![forbid(unsafe_code)]
#![deny(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use ignore::WalkBuilder;
use rayon::prelude::*;
use ryl::config::{ConfigContext, Overrides, YamlLintConfig, discover_config, discover_per_file};
use ryl::parse_yaml_file;

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

fn build_global_cfg(inputs: &[PathBuf], cli: &Cli) -> Option<ConfigContext> {
    if inputs.is_empty() {
        return None;
    }
    if cli.config_data.is_some()
        || cli.config_file.is_some()
        || std::env::var("YAMLLINT_CONFIG_FILE").is_ok()
    {
        Some(
            discover_config(
                inputs,
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
    }
}

fn ignored_for(
    path: &Path,
    global_cfg: Option<&ConfigContext>,
    cache: &mut HashMap<PathBuf, (PathBuf, YamlLintConfig)>,
) -> bool {
    global_cfg.map_or_else(
        || {
            let start = path
                .parent()
                .map_or_else(|| PathBuf::from("."), PathBuf::from);
            let entry = cache.get(&start).cloned();
            let (base_dir, cfg) = entry.unwrap_or_else(|| {
                let ctx = discover_per_file(path).unwrap_or_else(|e| {
                    eprintln!("{e}");
                    std::process::exit(2);
                });
                let pair = (ctx.base_dir.clone(), ctx.config);
                cache.insert(start.clone(), pair.clone());
                pair
            });
            cfg.is_file_ignored(path, &base_dir)
        },
        |gc| gc.config.is_file_ignored(path, &gc.base_dir),
    )
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

    // Build a global config if -d/-c provided or env var set; else None for per-file discovery.
    let global_cfg = build_global_cfg(&cli.inputs, &cli);
    let inputs = cli.inputs;
    // Usage error when no inputs
    if inputs.is_empty() {
        eprintln!("error: expected one or more paths (files and/or directories)");
        return ExitCode::from(2);
    }

    // Determine files to parse from mixed inputs.
    // - Directories: recursively gather only .yml/.yaml
    // - Files: include as-is (even if extension isn't yaml)
    let (candidates, explicit_files) = gather_inputs(&inputs);

    // Filter directory candidates via ignores, respecting global vs per-file behavior.
    let mut cache: HashMap<PathBuf, (PathBuf, YamlLintConfig)> = HashMap::new();
    let mut filtered: Vec<PathBuf> = Vec::new();
    for f in candidates {
        let ignored = ignored_for(&f, global_cfg.as_ref(), &mut cache);
        let yaml_ok = global_cfg.as_ref().map_or_else(
            || {
                let start = f.parent().map_or_else(|| PathBuf::from("."), PathBuf::from);
                let (base_dir, cfg) = cache.get(&start).cloned().unwrap_or_else(|| {
                    let ctx = discover_per_file(&f).unwrap_or_else(|e| {
                        eprintln!("{e}");
                        std::process::exit(2);
                    });
                    let pair = (ctx.base_dir.clone(), ctx.config);
                    cache.insert(start.clone(), pair.clone());
                    pair
                });
                cfg.is_yaml_candidate(&f, &base_dir)
            },
            |gc| gc.config.is_yaml_candidate(&f, &gc.base_dir),
        );
        if !ignored && yaml_ok {
            filtered.push(f);
        }
    }

    // Combine with explicit files (parity: explicit files are also filtered by ignores).
    let mut files: Vec<PathBuf> = filtered;
    for ef in explicit_files {
        let ignored = ignored_for(&ef, global_cfg.as_ref(), &mut cache);
        let yaml_ok = global_cfg.as_ref().map_or_else(
            || {
                let start = ef
                    .parent()
                    .map_or_else(|| PathBuf::from("."), PathBuf::from);
                let (base_dir, cfg) = cache.get(&start).cloned().unwrap_or_else(|| {
                    let ctx = discover_per_file(&ef).unwrap_or_else(|e| {
                        eprintln!("{e}");
                        std::process::exit(2);
                    });
                    let pair = (ctx.base_dir.clone(), ctx.config);
                    cache.insert(start.clone(), pair.clone());
                    pair
                });
                cfg.is_yaml_candidate(&ef, &base_dir)
            },
            |gc| gc.config.is_yaml_candidate(&ef, &gc.base_dir),
        );
        if !ignored && yaml_ok {
            files.push(ef);
        }
    }

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
