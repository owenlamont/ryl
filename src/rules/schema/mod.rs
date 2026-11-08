//! Local JSON Schema validation; no safe fix can infer the intended data.
//! Sources: JSON Schema 2020-12; YAML 1.2.2 core schema.

mod loading;
mod projection;

use std::path::Path;
use std::sync::Arc;

use jsonschema::{Validator, error::ValidationErrorKind};

use crate::config::{PathGlob, YamlLintConfig};
use crate::text_version::Analysis;

pub(crate) use loading::prepare;

pub const ID: &str = "schema";
pub(crate) const MAX_DIAGNOSTICS: usize = 100;
pub(crate) const TRUNCATION: &str = "further schema diagnostics truncated (limit 100)";

#[derive(Clone)]
pub(crate) struct Prepared {
    pub(crate) associations: Vec<Association>,
    pub(crate) digest: blake3::Hash,
}

#[derive(Clone)]
pub(crate) struct Association {
    pub(crate) files: Vec<PathGlob>,
    validator: Arc<Validator>,
}

impl Association {
    pub(crate) fn matches(&self, path: &Path) -> bool {
        (self.files.iter().all(|glob| glob.negated)
            || self
                .files
                .iter()
                .any(|glob| !glob.negated && glob.matches(path)))
            && self
                .files
                .iter()
                .filter(|glob| glob.negated)
                .all(|glob| glob.matches(path))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

impl Violation {
    fn at(span: ryl_yaml_parser::Span, message: String) -> Self {
        Self {
            line: span.start.line(),
            column: span.start.col() + 1,
            message,
        }
    }
}

#[must_use]
pub fn check(buffer: &str, cfg: &YamlLintConfig, path: &Path) -> Vec<Violation> {
    check_with_analysis(
        crate::text_version::TextVersion::new(buffer).analysis(),
        cfg,
        path,
    )
}

pub(crate) fn check_with_analysis(
    input: Analysis<'_>,
    cfg: &YamlLintConfig,
    path: &Path,
) -> Vec<Violation> {
    let Some(association) = cfg.schema_for(path) else {
        return Vec::new();
    };
    let mut diagnostics = Vec::new();
    let mut budget = projection::Budget::default();
    let mut events = input.event_iter().peekable();
    while events.peek().is_some() {
        let projected = match projection::document(&mut events, &mut budget) {
            Ok(Some(projected)) => projected,
            Ok(None) => break,
            Err(error) => {
                let message = if diagnostics.len() == MAX_DIAGNOSTICS {
                    TRUNCATION.to_owned()
                } else {
                    format!("cannot validate: {}", error.reason)
                };
                diagnostics.push(Violation::at(*error.span, message));
                if diagnostics.len() > MAX_DIAGNOSTICS || budget.exhausted() {
                    break;
                }
                continue;
            }
        };
        for error in association.validator.iter_errors(&projected.value) {
            if diagnostics.len() == MAX_DIAGNOSTICS {
                diagnostics.push(Violation::at(projected.root, TRUNCATION.to_owned()));
                return diagnostics;
            }
            let pointer = error.instance_path().to_string();
            let span = match error.kind() {
                ValidationErrorKind::AdditionalProperties { unexpected } => unexpected
                    .first()
                    .and_then(|key| {
                        projected
                            .keys
                            .get(&projection::child_pointer(&pointer, key))
                    })
                    .copied(),
                _ => None,
            }
            .unwrap_or_else(|| projected.span(&pointer));
            let message = if matches!(
                error.kind(),
                ValidationErrorKind::RegexEngineFailure { .. }
                    | ValidationErrorKind::BacktrackLimitExceeded { .. }
            ) {
                format!("cannot validate: {error}")
            } else {
                error.to_string()
            };
            diagnostics.push(Violation::at(span, message));
        }
    }
    diagnostics
}
