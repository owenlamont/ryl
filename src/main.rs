#![forbid(unsafe_code)]
#![deny(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use rayon::prelude::*;
use ryl::{gather_yaml_from_dir, parse_yaml_file};

#[derive(Parser, Debug)]
#[command(name = "ryl", version, about = "Fast YAML linter written in Rust")]
struct Cli {
    /// One or more paths: files and/or directories
    #[arg(value_name = "PATH_OR_FILE", num_args = 1..)]
    inputs: Vec<PathBuf>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    if cli.inputs.is_empty() {
        eprintln!("error: expected one or more paths (files and/or directories)");
        return ExitCode::from(2);
    }

    let inputs = cli.inputs;

    // Determine files to parse from mixed inputs.
    // - Directories: recursively gather only .yml/.yaml
    // - Files: include as-is (even if extension isn't yaml)
    let mut files: Vec<PathBuf> = Vec::new();
    for p in inputs {
        if p.is_dir() {
            files.extend(gather_yaml_from_dir(&p));
        } else {
            files.push(p);
        }
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
