//! `yaml-directive`: require a `%YAML` version directive before every document, so each
//! declares the version its plain scalars resolve under. Sources: YAML 1.2.2 §6.8.1.
//!
//! Safe `--fix` stamps `%YAML 1.2`, the version ryl reads an undeclared document as, so
//! no value moves. It runs only under `target-version = "1.2"`: under `"1.1"` the stamp
//! would drop the document's 1.1-safe quoting. It declines a document that would need a
//! `...` before it while `document-end` forbids one.

use crate::config::{TargetVersion, YamlLintConfig};
use crate::rules::support::line_syntax::buffer_newline;
use crate::rules::support::span_utils::BytePos;
use crate::rules::{document_end, document_start};
use crate::text_version::{Analysis, TextVersion};

pub const ID: &str = "yaml-directive";
pub const MESSAGE: &str = "missing version directive \"%YAML\"";

#[derive(Debug, Clone, Copy)]
pub struct Config {
    fixable: bool,
    end_marker_forbidden: bool,
}

impl Config {
    #[must_use]
    pub fn resolve(cfg: &YamlLintConfig) -> Self {
        Self {
            fixable: cfg.target_version() == TargetVersion::Yaml12,
            end_marker_forbidden: cfg.rule_level(document_end::ID).is_some()
                && !document_end::Config::resolve(cfg).requires_marker(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub line: usize,
    pub column: usize,
}

#[must_use]
pub fn check(buffer: &str) -> Vec<Violation> {
    check_with_analysis(TextVersion::new(buffer).analysis())
}

pub(crate) fn check_with_analysis(input: Analysis<'_>) -> Vec<Violation> {
    document_start::starts(input)
        .filter(|start| !start.declared)
        .map(|start| Violation {
            line: start.line,
            column: if start.explicit { start.column + 1 } else { 1 },
        })
        .collect()
}

#[must_use]
pub fn fix(buffer: &str, cfg: &Config) -> Option<String> {
    if !cfg.fixable {
        return None;
    }
    let source = TextVersion::new(buffer);
    let input = source.analysis();
    let newline = buffer_newline(buffer);
    let directive = format!("%YAML 1.2{newline}");
    let implicit = document_start::edits(input, document_start::Config::new(true))
        .into_iter()
        .map(|(at, end, marker)| (at, end, format!("{directive}{marker}")));
    let explicit = document_start::starts(input)
        .filter(|start| start.explicit && !start.declared)
        .filter(|start| start.follows_end_marker || !cfg.end_marker_forbidden)
        .map(|start| {
            let end = if start.follows_end_marker {
                String::new()
            } else {
                format!("...{newline}")
            };
            let at = BytePos::new(start.offset);
            (at, at, format!("{end}{directive}"))
        });
    crate::text_version::edits::render(buffer, implicit.chain(explicit).collect())
}
