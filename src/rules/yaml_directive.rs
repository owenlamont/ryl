//! `yaml-directive`: require a `%YAML` version directive before every document, so each
//! declares the version its plain scalars resolve under, and under
//! `target-version = "1.2"` report one declaring `%YAML 1.1` or `1.0`.
//! Sources: YAML 1.2.2 §6.8.1, YAML 1.1 type repository.
//!
//! Safe `--fix` stamps `%YAML 1.2`, the version ryl reads an undeclared document as, so
//! no value moves. It runs only under `target-version = "1.2"`: under `"1.1"` the stamp
//! would drop the document's 1.1-safe quoting. It declines a document that would need a
//! `...` before it while `document-end` forbids one.
//!
//! The fix migrates a `%YAML 1.1` document in one edit, respelling every scalar whose 1.1
//! value 1.2 reads differently and writing `%YAML 1.2`: a respelling alone can move a
//! value under 1.1 (`0o777` is a string to `PyYAML`). A document holding a scalar with no
//! 1.2 core spelling, or one `PyYAML` reads as a string and other 1.1 readers do not,
//! keeps its report and no fix.

use std::borrow::Cow;

use ryl_yaml_parser::{Event, ScalarStyle, Tag};

use crate::config::{TargetVersion, YamlLintConfig};
use crate::directives::{Directives, PerLineRuleApply};
use crate::rules::support::line_syntax::buffer_newline;
use crate::rules::support::span_utils::{BytePos, marker_byte_offset};
use crate::rules::support::yaml_version::{
    Directive, Migration, Value, collect_directives, event_version, migration,
    resolves_as_yaml_1_1, value,
};
use crate::rules::{document_end, document_start};
use crate::text_version::edits::{Edit, render};
use crate::text_version::{Analysis, TextVersion};
use crate::yaml_dom::core_schema_suffix;

pub const ID: &str = "yaml-directive";
pub const MESSAGE: &str = "missing version directive \"%YAML\"";

#[derive(Debug, Clone, Copy)]
pub struct Config {
    targets_1_2: bool,
    end_marker_forbidden: bool,
}

impl Config {
    #[must_use]
    pub fn resolve(cfg: &YamlLintConfig) -> Self {
        Self {
            targets_1_2: cfg.target_version() == TargetVersion::Yaml12,
            end_marker_forbidden: cfg.rule_level(document_end::ID).is_some()
                && !document_end::Config::resolve(cfg).requires_marker(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub line: usize,
    pub column: usize,
    pub message: String,
}

#[must_use]
pub fn check(buffer: &str, cfg: Config) -> Vec<Violation> {
    check_with_analysis(TextVersion::new(buffer).analysis(), cfg)
}

pub(crate) fn check_with_analysis(input: Analysis<'_>, cfg: Config) -> Vec<Violation> {
    let missing = document_start::starts(input)
        .filter(|start| !start.declared)
        .map(|start| Violation {
            line: start.line,
            column: if start.explicit { start.column + 1 } else { 1 },
            message: MESSAGE.to_owned(),
        });
    let older = older_directives(input.text(), cfg).map(|directive| {
        let (major, minor) = directive.version;
        Violation {
            line: directive.line,
            column: directive.column,
            message: format!(
                "version directive \"%YAML {major}.{minor}\" is older than \
                 target-version \"1.2\""
            ),
        }
    });
    missing.chain(older).collect()
}

fn older_directives(buffer: &str, cfg: Config) -> impl Iterator<Item = Directive> {
    cfg.targets_1_2
        .then(|| collect_directives(buffer))
        .into_iter()
        .flatten()
        .filter(|directive| resolves_as_yaml_1_1(Some(directive.version)))
}

/// Migrates the `%YAML 1.1` documents when any can be, else stamps the undeclared ones,
/// so one pass never mixes line edits with line inserts for the reconcile to align.
#[must_use]
pub fn fix(
    buffer: &str,
    cfg: &Config,
    per_line: &[PerLineRuleApply<'_>],
) -> Option<String> {
    if !cfg.targets_1_2 {
        return None;
    }
    let source = TextVersion::new(buffer);
    let input = source.analysis();
    migrate(input, *cfg, per_line).or_else(|| stamp(input, *cfg))
}

fn migrate(
    input: Analysis<'_>,
    cfg: Config,
    per_line: &[PerLineRuleApply<'_>],
) -> Option<String> {
    let buffer = input.text();
    // YAML 1.1 reads a raw NEL, LS or PS as a line break and 1.2 as content.
    if buffer.contains(['\u{85}', '\u{2028}', '\u{2029}']) {
        return None;
    }
    let output = render(buffer, migration_edits(input, cfg))?;
    let output = Directives::parse_per_line_with_analysis(input, per_line)
        .reconcile(ID, buffer, &output);
    (output != buffer
        && values_match(input, TextVersion::new(output.as_str()).analysis()))
    .then_some(output)
}

fn migration_edits(input: Analysis<'_>, cfg: Config) -> Vec<Edit> {
    let mut directives = older_directives(input.text(), cfg);
    let mut edits = Vec::new();
    let mut document: Option<Vec<Edit>> = None;
    for (event, span) in input.event_iter() {
        match event {
            Event::DocumentStart(_, version) => {
                document =
                    resolves_as_yaml_1_1(version.map(event_version)).then(|| {
                        let directive = directives.next().expect(
                            "a document declaring YAML 1.1 follows its directive",
                        );
                        vec![(directive.start, directive.end, "%YAML 1.2".to_owned())]
                    });
            }
            Event::Scalar(text, style, _, tag) => {
                let Some(pending) = document.as_mut() else {
                    continue;
                };
                let suffix = tag.as_deref().map(tag_suffix);
                match migration(&text, style == ScalarStyle::Plain, suffix.as_deref()) {
                    Migration::Keep => {}
                    Migration::Respell(spelling) => {
                        let start = marker_byte_offset(span.start);
                        let end = BytePos::new(start.get() + text.len());
                        pending.push((start, end, spelling));
                    }
                    Migration::Decline => document = None,
                }
            }
            Event::DocumentEnd => edits.extend(document.take().into_iter().flatten()),
            _ => {}
        }
    }
    edits
}

/// Whether every scalar of `after` resolves, under its document's version, to the value
/// its counterpart in `before` does, and every other event matches.
fn values_match(before: Analysis<'_>, after: Analysis<'_>) -> bool {
    resolved(before).eq(resolved(after))
}

fn resolved(input: Analysis<'_>) -> impl Iterator<Item = (Option<Value>, Event<'_>)> {
    let mut yaml_1_1 = false;
    input
        .event_iter()
        .filter_map(move |(event, _)| match event {
            Event::DocumentStart(_, version) => {
                yaml_1_1 = resolves_as_yaml_1_1(version.map(event_version));
                None
            }
            Event::Scalar(text, style, anchor, tag) => {
                let suffix = tag.as_deref().map(tag_suffix);
                let resolved = value(
                    &text,
                    style == ScalarStyle::Plain,
                    suffix.as_deref(),
                    yaml_1_1,
                );
                let text = if resolved.is_some() {
                    Cow::Borrowed("")
                } else {
                    text
                };
                Some((resolved, Event::Scalar(text, style, anchor, tag)))
            }
            other => Some((None, other)),
        })
}

/// A tag's core-schema suffix: `!` for the non-specific tag, empty for any other tag
/// outside the core schema (`!env`).
fn tag_suffix(tag: &Tag) -> Cow<'_, str> {
    if tag.handle().is_empty() && tag.suffix() == "!" {
        return Cow::Borrowed("!");
    }
    core_schema_suffix(tag).unwrap_or_default()
}

fn stamp(input: Analysis<'_>, cfg: Config) -> Option<String> {
    let buffer = input.text();
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
    render(buffer, implicit.chain(explicit).collect())
}
