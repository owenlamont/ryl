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
pub mod directives;
pub mod discover;
pub mod fix;
pub mod lint;
#[cfg(feature = "lsp")]
pub mod lsp;
pub mod markdown_embed;
pub mod migrate;
pub mod report;
pub mod rules;
pub mod yaml_dom;

pub use discover::{gather_yaml_from_dir, is_yaml_path};
pub use lint::{LintProblem, Severity, lint_file, lint_markdown_file, lint_str};
pub use markdown_embed::{
    EmbeddedRegion, MarkdownSources, RegionKind, extract_regions, lint_markdown_str,
};
pub use report::{ReportEntry, render_gitlab, render_junit};
