#![forbid(unsafe_code)]
#![deny(clippy::all, clippy::pedantic, clippy::nursery, clippy::cargo)]

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use ignore::WalkBuilder;
use rayon::prelude::*;

struct NullSink;
impl<'i> saphyr_parser::EventReceiver<'i> for NullSink {
    fn on_event(&mut self, _ev: saphyr_parser::Event<'i>) {}
}

#[derive(Parser, Debug)]
#[command(name = "ryl", version, about = "Fast YAML linter written in Rust")]
struct Cli {
    /// One or more paths: files and/or directories
    #[arg(value_name = "PATH_OR_FILE", num_args = 1..)]
    inputs: Vec<PathBuf>,
}

fn is_yaml_path(path: &Path) -> bool {
    matches!(
        path.extension().and_then(OsStr::to_str).map(str::to_ascii_lowercase),
        Some(ref ext) if ext == "yml" || ext == "yaml"
    )
}

fn gather_yaml_from_dir(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let walker = WalkBuilder::new(dir)
        .hidden(false)
        .ignore(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .follow_links(false)
        .build();

    for entry in walker.flatten() {
        let p = entry.path();
        if p.is_file() && is_yaml_path(p) {
            files.push(p.to_path_buf());
        }
    }
    files
}

fn parse_yaml_file(path: &Path) -> Result<(), String> {
    let content = fs::read_to_string(path)
        .map_err(|e| format!("failed to read {}: {}", path.display(), e))?;
    let mut parser = saphyr_parser::Parser::new_from_str(&content);
    let mut sink = NullSink;
    match parser.load(&mut sink, true) {
        Ok(()) => Ok(()),
        Err(e) => {
            let m = e.marker();
            let msg = e.info();
            Err(format!(
                "{}\n  {}:{}       error    syntax error: {} (syntax)",
                path.display(),
                m.line(),
                m.col() + 1,
                msg
            ))
        }
    }
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
