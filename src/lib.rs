#![forbid(unsafe_code)]
#![deny(
    clippy::all,
    clippy::pedantic,
    clippy::cargo,
    clippy::cognitive_complexity
)]

pub mod cli_support;
pub mod conf;
pub mod config;
pub mod config_schema;
pub mod decoder;
pub mod discover;
pub mod fix;
pub mod lint;
pub mod migrate;
pub mod rules;

pub use discover::{gather_yaml_from_dir, is_yaml_path};
pub use lint::{LintProblem, Severity, lint_file};
