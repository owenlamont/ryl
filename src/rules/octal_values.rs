//! `octal-values`: flag implicit (`0777`, `-0777`) and/or explicit (`0o777`) octal
//! integers, which surprise readers expecting decimal. Mirrors yamllint's `octal-values`,
//! which skips the signed form YAML 1.1 reads as octal too. No safe `--fix`: resolving
//! `010` needs the author's intent (integer 8, integer 10, or the string "010"). In a
//! document YAML 1.1 and 1.2 readers must both load, an unsafe fix writes the decimal
//! `source-version` reads (`0777` is 777 under `"1.2"`, 511 under `"1.1"`).

use ryl_yaml_parser::{Event, ScalarStyle};

use crate::config::{TargetVersion, YamlLintConfig};
use crate::rules::support::span_utils::{BytePos, apply_replacements};
use crate::rules::support::yaml_version::{Readers, Respelling, event_version};
use crate::text_version::{Analysis, TextVersion};

pub const ID: &str = "octal-values";

#[derive(Debug, Clone)]
pub struct Config {
    forbid_implicit: bool,
    forbid_explicit: bool,
    target: TargetVersion,
    respelling: Respelling,
}

impl Config {
    #[must_use]
    pub fn resolve(cfg: &YamlLintConfig) -> Self {
        Self {
            forbid_implicit: cfg.rule_option_bool(ID, "forbid-implicit-octal", true),
            forbid_explicit: cfg.rule_option_bool(ID, "forbid-explicit-octal", true),
            target: cfg.target_version(),
            respelling: Respelling::resolve(cfg, ID),
        }
    }

    fn message(&self, value: &str) -> Option<String> {
        if self.forbid_implicit && is_implicit_octal(value) {
            Some(format!("forbidden implicit octal value \"{value}\""))
        } else if self.forbid_explicit && is_explicit_octal(value) {
            Some(format!("forbidden explicit octal value \"{value}\""))
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub line: usize,
    pub column: usize,
    pub message: String,
    pub hidden_unsafe_fix: bool,
}

#[must_use]
pub fn check(buffer: &str, cfg: &Config) -> Vec<Violation> {
    check_with_analysis(TextVersion::new(buffer).analysis(), cfg)
}

pub(crate) fn check_with_analysis(input: Analysis<'_>, cfg: &Config) -> Vec<Violation> {
    walk(input, cfg).0
}

#[must_use]
pub fn fix(buffer: &str, cfg: &Config) -> Option<String> {
    let replacements = walk(TextVersion::new(buffer).analysis(), cfg).1;
    (!replacements.is_empty()).then(|| apply_replacements(buffer, replacements))
}

type Replacement = (BytePos, BytePos, String);

fn walk(input: Analysis<'_>, cfg: &Config) -> (Vec<Violation>, Vec<Replacement>) {
    let respelling = cfg.respelling;
    let mut readers = Readers::of(None, cfg.target);
    let mut diagnostics = Vec::new();
    let mut replacements = Vec::new();
    for (event, span) in input.event_iter() {
        match event {
            Event::DocumentStart(_, version) => {
                readers = Readers::of(version.map(event_version), cfg.target);
            }
            Event::Scalar(value, ScalarStyle::Plain, _, None) => {
                let Some(mut message) = cfg.message(&value) else {
                    continue;
                };
                let edit = (readers == Readers::Both)
                    .then(|| respelling.edit(input.text(), span, &value))
                    .flatten();
                if let Some((_, _, spelling)) = &edit {
                    message.push_str(&respelling.note(&value, spelling));
                }
                diagnostics.push(Violation {
                    line: span.end.line(),
                    column: span.end.col() + 1,
                    message,
                    hidden_unsafe_fix: edit.is_some() && respelling.hidden(),
                });
                replacements.extend(edit.filter(|_| respelling.enabled()));
            }
            _ => {}
        }
    }
    (diagnostics, replacements)
}

fn is_implicit_octal(value: &str) -> bool {
    let bytes = value.strip_prefix(['-', '+']).unwrap_or(value).as_bytes();
    if bytes.len() <= 1 || bytes[0] != b'0' {
        return false;
    }
    if !bytes.iter().all(u8::is_ascii_digit) {
        return false;
    }
    bytes[1..].iter().all(|b| (b'0'..=b'7').contains(b))
}

fn is_explicit_octal(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() <= 2 || bytes[0] != b'0' || bytes[1] != b'o' {
        return false;
    }
    bytes[2..].iter().all(|b| (b'0'..=b'7').contains(b))
}
