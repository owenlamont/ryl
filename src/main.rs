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
        Some(
            discover_config(
                inputs,
                &Overrides {
                    config_file: cli.config_file.clone(),
                    config_data,
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

fn resolve_ctx(
    path: &Path,
    global_cfg: Option<&ConfigContext>,
    cache: &mut HashMap<PathBuf, (PathBuf, YamlLintConfig)>,
) -> Result<(PathBuf, YamlLintConfig), String> {
    if let Some(gc) = global_cfg {
        return Ok((gc.base_dir.clone(), gc.config.clone()));
    }
    let start = path
        .parent()
        .map_or_else(|| PathBuf::from("."), PathBuf::from);
    if let Some(pair) = cache.get(&start).cloned() {
        return Ok(pair);
    }
    let ctx = discover_per_file(path)?;
    let pair = (ctx.base_dir.clone(), ctx.config);
    cache.insert(start, pair.clone());
    Ok(pair)
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

    // Determine files to parse from mixed inputs.
    // - Directories: recursively gather only .yml/.yaml
    // - Files: include as-is (even if extension isn't yaml)
    let (candidates, explicit_files) = gather_inputs(&inputs);

    // Filter directory candidates via ignores, respecting global vs per-file behavior.
    let mut cache: HashMap<PathBuf, (PathBuf, YamlLintConfig)> = HashMap::new();
    let mut filtered: Vec<PathBuf> = Vec::new();
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
            filtered.push(f);
        }
    }

    // Combine with explicit files (parity: explicit files are also filtered by ignores).
    let mut files: Vec<PathBuf> = filtered;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_ctx_handles_path_without_parent() {
        let mut cache: HashMap<PathBuf, (PathBuf, YamlLintConfig)> = HashMap::new();
        let p = PathBuf::from("");
        let _ = resolve_ctx(&p, None, &mut cache);
    }

    #[test]
    fn build_global_cfg_extends_shortcut() {
        let cli = Cli {
            inputs: vec![PathBuf::from("foo.yaml")],
            config_file: None,
            config_data: Some("relaxed".to_string()),
            list_files: false,
            format: None,
            strict: false,
            no_warnings: false,
        };
        let ctx = build_global_cfg(&cli.inputs, &cli).expect("config context");
        assert!(ctx.config.rule_names().iter().any(|r| r == "braces"));
    }

    #[test]
    fn build_global_cfg_retains_inline_yaml() {
        let yaml = "rules: {}".to_string();
        let cli = Cli {
            inputs: vec![PathBuf::from("foo.yaml")],
            config_file: None,
            config_data: Some(yaml),
            list_files: false,
            format: None,
            strict: false,
            no_warnings: false,
        };
        let ctx = build_global_cfg(&cli.inputs, &cli).expect("config context");
        assert!(ctx.config.rule_names().is_empty());
    }
}
