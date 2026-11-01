//! `schema-dependent-values`: flag plain scalars that YAML 1.1 and the YAML 1.2 core
//! schema read as different values, in a document with no `%YAML` directive under
//! `target-version = "1.1"`. `truthy`, `octal-values` and `float-values` own `yes`/`no`,
//! `0777`/`-0777`/`0o777` and `1e5`. Sources: YAML 1.1 type repository, YAML 1.2.2 §10.3.2.
//! No safe `--fix`: quoting, retyping or rewriting each needs the author's intent. An
//! unsafe fix writes the spelling both versions read as `source-version` does: quoted
//! under `"1.2"` (`08` becomes `8`), the 1.1 value under `"1.1"` (`22:22` becomes `1342`),
//! declining where YAML 1.1 readers disagree (`08`, `y`) or 1.2 has no type (dates).

use std::sync::LazyLock;

use regex::Regex;
use ryl_yaml_parser::{Event, ScalarStyle};

use crate::config::{TargetVersion, YamlLintConfig};
use crate::rules::support::span_utils::{BytePos, apply_replacements};
use crate::rules::support::yaml_version::{
    Readers, Respelling, event_version, resolves_to_nonstring_in_yaml_1_1,
};
use crate::text_version::model::resolves_to_string_for;
use crate::text_version::{Analysis, TextVersion};

pub const ID: &str = "schema-dependent-values";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Timestamp,
    Sexagesimal,
    Underscored,
    LeadingZero,
    SingleLetterBoolean,
}

impl Kind {
    const ALL: [Self; 5] = [
        Self::Timestamp,
        Self::Sexagesimal,
        Self::Underscored,
        Self::LeadingZero,
        Self::SingleLetterBoolean,
    ];

    const fn option(self) -> &'static str {
        match self {
            Self::Timestamp => "forbid-timestamps",
            Self::Sexagesimal => "forbid-sexagesimal",
            Self::Underscored => "forbid-underscored-numbers",
            Self::LeadingZero => "forbid-leading-zero-decimals",
            Self::SingleLetterBoolean => "forbid-single-letter-booleans",
        }
    }

    const fn readings(self) -> (&'static str, &'static str) {
        match self {
            Self::Timestamp => ("a string", "a timestamp"),
            Self::Sexagesimal => ("a string", "a sexagesimal number"),
            Self::Underscored => ("a string", "a number"),
            Self::LeadingZero => ("an integer", "a string"),
            Self::SingleLetterBoolean => ("a string", "a boolean"),
        }
    }

    fn classify(value: &str) -> Option<Self> {
        if LEADING_ZERO.is_match(value) {
            return Some(Self::LeadingZero);
        }
        if !(resolves_to_string_for(Readers::Yaml12, value)
            && resolves_to_nonstring_in_yaml_1_1(value))
        {
            return None;
        }
        if matches!(value, "y" | "Y" | "n" | "N") {
            Some(Self::SingleLetterBoolean)
        } else if value.as_bytes().get(..5).is_some_and(|head| {
            head[..4].iter().all(u8::is_ascii_digit) && head[4] == b'-'
        }) {
            Some(Self::Timestamp)
        } else if value.contains(':') {
            Some(Self::Sexagesimal)
        } else if value.contains('_')
            || matches!(
                value.trim_start_matches(['-', '+']).get(..2),
                Some("0b" | "0x")
            )
        {
            Some(Self::Underscored)
        } else {
            None
        }
    }
}

static LEADING_ZERO: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\A[-+]?0[0-9]*[89][0-9]*\z").expect("leading-zero regex is valid")
});

#[derive(Debug, Clone)]
pub struct Config {
    forbidden: Vec<Kind>,
    target: TargetVersion,
    respelling: Respelling,
}

impl Config {
    #[must_use]
    pub fn resolve(cfg: &YamlLintConfig) -> Self {
        Self {
            forbidden: Kind::ALL
                .into_iter()
                .filter(|kind| cfg.rule_option_bool(ID, kind.option(), true))
                .collect(),
            target: cfg.target_version(),
            respelling: Respelling::resolve(cfg, ID),
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
    walk(input, cfg).map(|(violation, _)| violation).collect()
}

#[must_use]
pub fn fix(buffer: &str, cfg: &Config) -> Option<String> {
    let replacements: Vec<_> = walk(TextVersion::new(buffer).analysis(), cfg)
        .filter_map(|(_, edit)| edit)
        .collect();
    (!replacements.is_empty()).then(|| apply_replacements(buffer, replacements))
}

fn walk<'a>(
    input: Analysis<'a>,
    cfg: &'a Config,
) -> impl Iterator<Item = (Violation, Option<(BytePos, BytePos, String)>)> + 'a {
    let respelling = cfg.respelling;
    let mut readers = Readers::of(None, cfg.target);
    input
        .event_iter()
        .filter_map(move |(event, span)| match event {
            Event::DocumentStart(_, version) => {
                readers = Readers::of(version.map(event_version), cfg.target);
                None
            }
            Event::Scalar(value, ScalarStyle::Plain, _, None)
                if readers == Readers::Both =>
            {
                let kind = Kind::classify(&value)
                    .filter(|kind| cfg.forbidden.contains(kind))?;
                let (yaml12, yaml11) = kind.readings();
                let edit = respelling.edit(input.text(), span, &value);
                let note = edit
                    .as_ref()
                    .map(|(_, _, spelling)| respelling.note(&value, spelling))
                    .unwrap_or_default();
                Some((
                    Violation {
                        line: span.start.line(),
                        column: span.start.col() + 1,
                        message: format!(
                            "plain \"{value}\" is {yaml12} in YAML 1.2 but {yaml11} in \
                             YAML 1.1{note}"
                        ),
                        hidden_unsafe_fix: edit.is_some() && respelling.hidden(),
                    },
                    edit.filter(|_| respelling.enabled()),
                ))
            }
            _ => None,
        })
}
