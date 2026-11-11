//! JSON Schema validation; no safe fix can infer the intended data.
//! Sources: JSON Schema 2020-12; YAML 1.2.2 core schema.

mod complexity;
mod diagnostics;
pub(crate) mod directives;
#[cfg(feature = "lsp")]
pub(crate) mod hover;
mod loading;
#[doc(hidden)]
pub mod net;
mod projection;

use std::path::Path;
use std::sync::Arc;

use jsonschema::{Validator, error::ValidationErrorKind};

use crate::config::{PathGlob, YamlLintConfig};
use crate::text_version::Analysis;

pub(crate) use loading::prepare;

pub(crate) enum Preparation {
    Ready(Option<Arc<Prepared>>),
    NeedsRemotePreparation,
}

pub const ID: &str = "schema";
pub(crate) const MAX_SCHEMA_BYTES: usize = 16 * 1024 * 1024;
pub(crate) const MAX_SCHEMA_RESOURCES: usize = 1024;

#[cfg(debug_assertions)]
thread_local! { static DIRECTIVE_SCANS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

#[cfg(debug_assertions)]
#[doc(hidden)]
#[must_use]
pub fn directive_scan_count() -> usize {
    DIRECTIVE_SCANS.get()
}

#[cfg(debug_assertions)]
fn record_directive_scan() {
    DIRECTIVE_SCANS.set(DIRECTIVE_SCANS.get() + 1);
}

#[doc(hidden)]
#[must_use]
pub fn document_directives(text: &str) -> Vec<Option<String>> {
    let version = crate::text_version::TextVersion::new(text);
    let input = version.analysis();
    directives::headers(input)
        .into_iter()
        .map(|header| header.and_then(|header| header.value))
        .collect()
}
#[doc(hidden)]
#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct PreparationError {
    pub line: usize,
    pub column: usize,
    pub message: String,
}
pub(crate) const MAX_DIAGNOSTICS: usize = 100;
pub(crate) const TRUNCATION: &str = "further schema diagnostics truncated (limit 100)";

#[derive(Clone)]
pub(crate) struct Prepared {
    pub(crate) associations: Vec<Association>,
    pub(crate) directives: std::collections::BTreeMap<String, Association>,
    pub(crate) digest: blake3::Hash,
    pub(crate) bytes: usize,
    pub(crate) resources: usize,
}

#[derive(Clone)]
pub(crate) struct Association {
    pub(crate) files: Vec<PathGlob>,
    validator: Arc<Validator>,
    registry: Arc<jsonschema::Registry<'static>>,
    root_uri: jsonschema::Uri<String>,
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
    let fallback = cfg.schema_for(path);
    let headers = cfg.schema_directives().then(|| directives::headers(input));
    if fallback.is_none() && headers.is_none() {
        return Vec::new();
    }
    let mut diagnostics = Vec::new();
    let mut budget = projection::Budget::default();
    let mut events = input
        .event_iter()
        .filter(|(event, _)| !matches!(event, ryl_yaml_parser::Event::Comment(..)))
        .peekable();
    let mut document = 0;
    while let Some((start, version)) =
        events.by_ref().find_map(|(event, start)| match event {
            ryl_yaml_parser::Event::DocumentStart(_, version) => Some((start, version)),
            _ => None,
        })
    {
        let selected = headers
            .as_ref()
            .and_then(|headers| headers.get(document))
            .and_then(Option::as_ref)
            .and_then(|header| header.value.as_deref());
        document += 1;
        let association = selected_association(cfg, fallback, selected);
        let Some(association) = association else {
            for (event, _) in events.by_ref() {
                if matches!(event, ryl_yaml_parser::Event::DocumentEnd) {
                    break;
                }
            }
            continue;
        };
        let projected =
            match projection::document(&mut events, &mut budget, start, version) {
                Ok(projected) => projected,
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
        if !validate_document(&projected, association, &mut diagnostics) {
            break;
        }
    }
    diagnostics
}

fn selected_association<'a>(
    cfg: &'a YamlLintConfig,
    fallback: Option<&'a Association>,
    selected: Option<&str>,
) -> Option<&'a Association> {
    match selected {
        Some(value) if value.eq_ignore_ascii_case("none") => None,
        Some(value) => cfg.schema_directive_for(value),
        None => fallback,
    }
}

fn validate_document(
    projected: &projection::Projection,
    association: &Association,
    diagnostics: &mut Vec<Violation>,
) -> bool {
    for error in association.validator.iter_errors(&projected.value) {
        if diagnostics.len() == MAX_DIAGNOSTICS {
            diagnostics.push(Violation::at(projected.root, TRUNCATION.to_owned()));
            return false;
        }
        let error = diagnostics::best_match(&error, association, &projected.value);
        let pointer = error.instance_path().to_string();
        let span = match error.kind() {
            ValidationErrorKind::AdditionalProperties { unexpected }
            | ValidationErrorKind::UnevaluatedProperties { unexpected } => unexpected
                .first()
                .and_then(|key| {
                    projected
                        .keys
                        .get(&projection::child_pointer(&pointer, key))
                })
                .copied(),
            ValidationErrorKind::AdditionalItems { limit } => {
                Some(projected.span(&format!("{pointer}/{limit}")))
            }
            ValidationErrorKind::UnevaluatedItems { unexpected } => projected
                .value
                .pointer(&pointer)
                .and_then(serde_json::Value::as_array)
                .and_then(|items| {
                    diagnostics::unevaluated_item_index(items, unexpected)
                })
                .map(|index| projected.span(&format!("{pointer}/{index}"))),
            ValidationErrorKind::PropertyNames { error } => projected
                .keys
                .get(&projection::child_pointer(
                    &pointer,
                    error
                        .instance()
                        .as_str()
                        .expect("property name is a string"),
                ))
                .copied(),
            _ => None,
        }
        .unwrap_or_else(|| projected.span(&pointer));
        let message = match error.kind() {
            ValidationErrorKind::Contains => {
                "array items do not satisfy the `contains` constraints".to_owned()
            }
            ValidationErrorKind::MinItems { limit } => format!(
                "value has fewer than {limit} item{}",
                if *limit == 1 { "" } else { "s" }
            ),
            ValidationErrorKind::MinProperties { limit } => format!(
                "value has fewer than {limit} {}",
                if *limit == 1 {
                    "property"
                } else {
                    "properties"
                }
            ),
            ValidationErrorKind::RegexEngineFailure { .. }
            | ValidationErrorKind::BacktrackLimitExceeded { .. } => {
                format!("cannot validate: {}", error.masked())
            }
            _ => error.masked().to_string(),
        };
        diagnostics.push(Violation::at(span, message));
    }
    true
}
