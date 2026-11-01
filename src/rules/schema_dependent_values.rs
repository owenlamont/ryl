//! `schema-dependent-values`: flag plain scalars that YAML 1.1 and the YAML 1.2 core
//! schema read as different values, in a document with no `%YAML` directive under
//! `target-version = "1.1"`. `truthy`, `octal-values` and `float-values` own `yes`/`no`,
//! `0777`/`0o777` and `1e5`. Sources: YAML 1.1 type repository, YAML 1.2.2 §10.3.2.
//! No safe `--fix`: quoting, retyping or rewriting each needs the author's intent.

use std::sync::LazyLock;

use regex::Regex;
use ryl_yaml_parser::{Event, ScalarStyle};

use crate::config::{TargetVersion, YamlLintConfig};
use crate::rules::support::yaml_version::{
    Readers, event_version, resolves_to_nonstring_in_yaml_1_1,
};
use crate::text_version::model::resolves_to_string_for;

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
            || value.trim_start_matches(['-', '+']).starts_with("0b")
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
pub fn check(buffer: &str, cfg: &Config) -> Vec<Violation> {
    check_with_analysis(
        crate::text_version::TextVersion::new(buffer).analysis(),
        cfg,
    )
}

pub(crate) fn check_with_analysis(
    input: crate::text_version::Analysis<'_>,
    cfg: &Config,
) -> Vec<Violation> {
    let mut readers = Readers::of(None, cfg.target);
    input
        .event_iter()
        .filter_map(|(event, span)| match event {
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
                Some(Violation {
                    line: span.start.line(),
                    column: span.start.col() + 1,
                    message: format!(
                        "plain \"{value}\" is {yaml12} in YAML 1.2 but {yaml11} in YAML 1.1"
                    ),
                })
            }
            _ => None,
        })
        .collect()
}
