//! `%YAML` version-directive handling shared by the rules and the lint engine: scan
//! directives, resolve the version in effect per document, and recognise the plain
//! scalars YAML 1.1 resolves to a non-string so quote removal stays value-preserving
//! for a 1.1 reader, carry a 1.1 document's values over to 1.2, and respell a scalar
//! YAML 1.1 and 1.2 read differently so both read the value its author meant.

use std::fmt::Debug;
use std::ops::{Add, Mul};
use std::str::FromStr;
use std::sync::LazyLock;

use num_bigint::BigInt;
use ordered_float::OrderedFloat;
use regex::Regex;
use ryl_yaml_parser::{Scanner, Span, StrInput, TokenType, YamlVersion};

use crate::config::{SourceVersion, TargetVersion, YamlLintConfig};
use crate::rules::quoted_strings;
use crate::rules::support::span_utils::{BytePos, marker_byte_offset};
use crate::yaml_dom::{
    core_schema_int, is_core_schema_null, parse_core_schema_bool, parse_core_schema_fp,
};

pub type Version = (u32, u32);

/// The `%YAML` version the parser reports on `Event::DocumentStart`, which already
/// resolves the directive to its document, so rules take it from the event rather than
/// rescanning.
#[must_use]
pub const fn event_version(version: YamlVersion) -> Version {
    (version.major, version.minor)
}

#[derive(Debug, Clone, Copy)]
pub struct Directive {
    pub line: usize,
    pub column: usize,
    pub version: Version,
    pub(crate) start: BytePos,
    pub(crate) end: BytePos,
}

pub(crate) fn collect_directives(buffer: &str) -> Vec<Directive> {
    if !buffer.contains("%YAML") {
        return Vec::new();
    }
    let mut directives = Vec::new();
    let mut scanner = Scanner::new(StrInput::new(buffer));
    // The scanner reports a `%YAML` only where it is a real directive (not block-scalar
    // or plain-scalar text that happens to start with `%YAML`); it stops at the first
    // lexical error, after which no further directive can be reached anyway.
    while let Some(Ok(token)) = scanner.next() {
        let (span, token_type) = token.into_parts();
        if let TokenType::VersionDirective(major, minor) = token_type {
            directives.push(Directive {
                line: span.start.line(),
                column: span.start.col() + 1,
                version: (major, minor),
                start: marker_byte_offset(span.start),
                end: marker_byte_offset(span.end),
            });
        }
    }
    directives
}

/// A document resolves under YAML 1.1 when it explicitly declares a pre-1.2 version;
/// an absent directive and `%YAML 1.2`+ resolve under the 1.2 core schema.
#[must_use]
pub const fn resolves_as_yaml_1_1(version: Option<Version>) -> bool {
    matches!(version, Some((1, minor)) if minor <= 1)
}

/// The readers a document's plain scalars must keep their values for: its `%YAML`
/// directive's version, else both 1.1 and 1.2 readers until `target-version = "1.2"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Readers {
    Yaml11,
    Both,
    #[default]
    Yaml12,
}

impl Readers {
    #[must_use]
    pub const fn of(version: Option<Version>, target: TargetVersion) -> Self {
        match (version, target) {
            _ if resolves_as_yaml_1_1(version) => Self::Yaml11,
            (None, TargetVersion::Yaml11) => Self::Both,
            _ => Self::Yaml12,
        }
    }
}

#[must_use]
pub fn first_unsupported_major(buffer: &str) -> Option<Directive> {
    collect_directives(buffer)
        .into_iter()
        .find(|directive| directive.version.0 != 1)
}

#[must_use]
pub fn first_higher_minor(buffer: &str) -> Option<Directive> {
    collect_directives(buffer)
        .into_iter()
        .find(|directive| directive.version.0 == 1 && directive.version.1 > 2)
}

/// Whether YAML 1.1 reads the plain scalar `value` as a non-string, so dropping its
/// quotes would change its value for a 1.1 reader.
#[must_use]
pub fn resolves_to_nonstring_in_yaml_1_1(value: &str) -> bool {
    YAML_1_1_NONSTRING.is_match(value)
        || match value.chars().next() {
            Some('.') => GO_YAML_FLOAT.is_match(value),
            Some(c) if c.is_ascii_digit() || matches!(c, '-' | '+') => {
                GO_YAML_FLOAT.is_match(&value.replace('_', ""))
            }
            _ => false,
        }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Value {
    Bool(bool),
    Int(BigInt),
    Float(OrderedFloat<f64>),
}

impl Value {
    const fn tag(&self) -> &'static str {
        match self {
            Self::Bool(_) => "bool",
            Self::Int(_) => "int",
            Self::Float(_) => "float",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Migration {
    Keep,
    /// A 1.2 core spelling of the scalar's 1.1 value.
    Respell(String),
    /// The value has no 1.2 core spelling, or `PyYAML` reads a string where another 1.1
    /// reader does not.
    Decline,
}

/// How a scalar of a `%YAML 1.1` document carries over to 1.2; `tag` is its core-schema
/// suffix, `!` for the non-specific tag, which `PyYAML` and ruamel.yaml resolve like no tag
/// although the spec reads a string, and empty for any other tag outside the core schema.
pub(crate) fn migration(text: &str, plain: bool, tag: Option<&str>) -> Migration {
    let typed = matches!(tag, Some("bool" | "int" | "float"));
    let implicit = matches!(tag, None | Some("!"));
    if !typed
        && (!implicit
            || !plain
            || text == "<<"
            || is_core_schema_null(text)
            || !resolves_to_nonstring_in_yaml_1_1(text))
    {
        return Migration::Keep;
    }
    match yaml_1_1_value(text) {
        Some((value, _)) if core_value(text).as_ref() == Some(&value) => {
            Migration::Keep
        }
        Some((value, spelling))
            if plain && tag.is_none_or(|tag| tag == value.tag()) =>
        {
            Migration::Respell(spelling)
        }
        _ => Migration::Decline,
    }
}

/// The value a plain or core-typed scalar resolves to under its document's version:
/// `None` for a string, a null, or a value YAML 1.2 cannot spell.
pub(crate) fn value(
    text: &str,
    plain: bool,
    tag: Option<&str>,
    yaml_1_1: bool,
) -> Option<Value> {
    if !(plain && matches!(tag, None | Some("!"))
        || matches!(tag, Some("bool" | "int" | "float")))
    {
        None
    } else if yaml_1_1 {
        yaml_1_1_value(text).map(|(value, _)| value)
    } else {
        core_value(text)
    }
}

/// The unsafe fix `truthy`, `octal-values` and `schema-dependent-values` offer for a plain
/// scalar of a document YAML 1.1 and 1.2 readers must both load: a spelling both read as
/// the value `source-version` reads.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Respelling {
    source: SourceVersion,
    quote: char,
    enabled: bool,
    hinted: bool,
}

impl Respelling {
    pub(crate) fn resolve(cfg: &YamlLintConfig, rule: &str) -> Self {
        let single = cfg.rule_level(quoted_strings::ID).is_some()
            && cfg.rule_option_str(quoted_strings::ID, "quote-type") == Some("single");
        Self {
            source: cfg.source_version(),
            quote: if single { '\'' } else { '"' },
            enabled: cfg.applies_unsafe_fix(rule),
            hinted: cfg.hints_unsafe_fixes() && cfg.fix().allows_rule(rule),
        }
    }

    pub(crate) const fn enabled(self) -> bool {
        self.enabled
    }

    /// Whether `ryl check` counts the fix in its unsafe-fix hint.
    pub(crate) const fn hidden(self) -> bool {
        !self.enabled && self.hinted
    }

    /// The edit respelling the single-line plain scalar `text` at `span` of `buffer`.
    pub(crate) fn edit(
        self,
        buffer: &str,
        span: Span,
        text: &str,
    ) -> Option<(BytePos, BytePos, String)> {
        let start = marker_byte_offset(span.start);
        let end = BytePos::new(start.get() + text.len());
        (buffer.get(start.get()..end.get()) == Some(text))
            .then(|| self.respell(text))
            .flatten()
            .map(|spelling| (start, end, spelling))
    }

    /// `None` where `PyYAML` reads a string other 1.1 readers do not, a timestamp, or a
    /// value no shared spelling carries.
    fn respell(self, text: &str) -> Option<String> {
        let value = match self.source {
            SourceVersion::Yaml12 => core_value(text),
            SourceVersion::Yaml11 => Some(yaml_1_1_value(text)?.0),
        };
        // Every scalar the three rules flag is digits, letters, `-`, `_`, `:`, `.`, `+` and
        // spaces, so quoting it needs no escape.
        let Some(value) = value else {
            return Some(format!("{quote}{text}{quote}", quote = self.quote));
        };
        let spelling = match &value {
            Value::Bool(value) => value.to_string(),
            Value::Int(value) => value.to_string(),
            Value::Float(value) => format!("{:?}", value.0),
        };
        (core_value(&spelling).as_ref() == Some(&value)
            && yaml_1_1_value(&spelling).is_some_and(|(read, _)| read == value))
        .then_some(spelling)
    }

    /// The message suffix naming the fix, once the config enables it.
    pub(crate) fn note(self, old: &str, new: &str) -> String {
        if self.enabled {
            format!("; unsafe fix: {old} -> {new}")
        } else {
            String::new()
        }
    }
}

fn core_value(text: &str) -> Option<Value> {
    parse_core_schema_bool(text)
        .map(Value::Bool)
        .or_else(|| core_schema_int(text).map(Value::Int))
        .or_else(|| parse_core_schema_fp(text).map(|float| Value::Float(float.into())))
}

/// `text`'s value under `PyYAML`'s YAML 1.1 resolver, which ruamel.yaml's 1.1 mode shares,
/// with a YAML 1.2 core spelling of it: `None` where `PyYAML` reads a string, a timestamp
/// or a type 1.2 lacks (`=`), or fails to load it (`0b_`).
fn yaml_1_1_value(text: &str) -> Option<(Value, String)> {
    let negative = text.starts_with('-');
    let digits = text.trim_start_matches(['-', '+']).replace('_', "");
    if let Some(value) = yaml_1_1_bool(text) {
        Some((Value::Bool(value), value.to_string()))
    } else if YAML_1_1_INT.is_match(text) {
        let (magnitude, radix) = match digits.as_bytes() {
            [b'0', b'b', body @ ..] => (BigInt::parse_bytes(body, 2), 2),
            [b'0', b'x', body @ ..] => (BigInt::parse_bytes(body, 16), 16),
            [b'0', body @ ..] if !body.is_empty() => (BigInt::parse_bytes(body, 8), 8),
            _ => (Some(sexagesimal::<BigInt>(&digits)), 10),
        };
        let magnitude = magnitude?;
        let value = if negative { -magnitude } else { magnitude };
        let spelling = match radix {
            8 if !negative => format!("0o{value:o}"),
            16 if !negative => format!("0x{value:x}"),
            _ => value.to_string(),
        };
        Some((Value::Int(value), spelling))
    } else if YAML_1_1_FLOAT.is_match(text) {
        let magnitude = match digits.to_ascii_lowercase().as_str() {
            ".inf" => f64::INFINITY,
            ".nan" => f64::NAN,
            number => sexagesimal::<f64>(number),
        };
        let value = if negative { -magnitude } else { magnitude };
        let spelling = if digits.contains(':') {
            format!("{value:?}")
        } else {
            text.replace('_', "")
        };
        Some((Value::Float(value.into()), spelling))
    } else {
        None
    }
}

/// A base-60 number summed from its least significant part, as `PyYAML` sums it, so a
/// float rounds the same way; a number without `:` is its one part.
fn sexagesimal<T>(number: &str) -> T
where
    T: Clone + FromStr + Add<Output = T> + Mul<Output = T> + From<u8>,
    T::Err: Debug,
{
    number
        .rsplit(':')
        .fold((T::from(0), T::from(1)), |(sum, base), part| {
            let part: T = part
                .parse()
                .expect("the YAML 1.1 number regexes admit only digits between colons");
            (sum + part * base.clone(), base * T::from(60))
        })
        .0
}

fn yaml_1_1_bool(text: &str) -> Option<bool> {
    match text {
        "yes" | "Yes" | "YES" | "true" | "True" | "TRUE" | "on" | "On" | "ON" => {
            Some(true)
        }
        "no" | "No" | "NO" | "false" | "False" | "FALSE" | "off" | "Off" | "OFF" => {
            Some(false)
        }
        _ => None,
    }
}

// go-yaml drops every `_` before matching this unless the value starts with `.`, so `0_8`
// and `1_0e5` load as floats but `.5_e2` stays a string.
static GO_YAML_FLOAT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\A[-+]?(?:\.[0-9]+|[0-9]+(?:\.[0-9]*)?)(?:[eE][-+]?[0-9]+)?\z")
        .expect("go-yaml float regex is valid")
});

static YAML_1_1_NONSTRING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(YAML_1_1_NONSTRING_PATTERN)
        .expect("YAML 1.1 implicit-type regex is valid")
});

macro_rules! yaml_1_1_float {
    () => {
        concat!(
            r"[-+]?[0-9][0-9_]*\.[0-9_]*(?:[eE][-+][0-9]+)?|\.[0-9][0-9_]*(?:[eE][-+][0-9]+)?",
            r"|[-+]?[0-9][0-9_]*(?::[0-5]?[0-9])+\.[0-9_]*|[-+]?\.(?:inf|Inf|INF)|\.(?:nan|NaN|NAN)",
        )
    };
}

macro_rules! yaml_1_1_int {
    () => {
        concat!(
            r"[-+]?0b[0-1_]+|[-+]?0[0-7_]+|[-+]?(?:0|[1-9][0-9_]*)|[-+]?0x[0-9a-fA-F_]+",
            r"|[-+]?[1-9][0-9_]*(?::[0-5]?[0-9])+",
        )
    };
}

static YAML_1_1_FLOAT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(r"\A(?:", yaml_1_1_float!(), r")\z"))
        .expect("YAML 1.1 float regex is valid")
});

static YAML_1_1_INT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(r"\A(?:", yaml_1_1_int!(), r")\z"))
        .expect("YAML 1.1 int regex is valid")
});

// PyYAML's 1.1 resolvers plus yamllint's `0o` octal and go-yaml v2's `y`/`n` booleans.
const YAML_1_1_NONSTRING_PATTERN: &str = concat!(
    r"\A(?:",
    r"y|Y|n|N|yes|Yes|YES|no|No|NO|true|True|TRUE|false|False|FALSE|on|On|ON|off|Off|OFF",
    r"|",
    yaml_1_1_float!(),
    r"|[-+]?0o[0-7_]+|",
    yaml_1_1_int!(),
    r"|<<|~|null|Null|NULL|",
    r"|[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]",
    r"|[0-9][0-9][0-9][0-9]-[0-9][0-9]?-[0-9][0-9]?(?:[Tt]|[ \t]+)[0-9][0-9]?:[0-9][0-9]:[0-9][0-9](?:\.[0-9]*)?(?:[ \t]*(?:Z|[-+][0-9][0-9]?(?::[0-9][0-9])?))?",
    r"|=",
    r")\z",
);
