// Derived from saphyr (`saphyr/src/scalar.rs`), dual-licensed MIT OR
// Apache-2.0; ryl ships under MIT. Trimmed to the surface ryl needs:
// `ScalarOwned` with parse helpers plus the borrowed-lifetime `Scalar` used
// when parsing scalars from the event stream.
//
// `parse_from_cow` resolves null/bool per the YAML 1.2 core schema (so
// `True`/`Null` are bool/null, matching saphyr's post-0.0.6 resolver rather
// than the narrower 0.0.6 release). `Yes`/`No`/`On`/`Off` are intentionally
// left as strings: those are YAML 1.1 booleans, and ryl targets YAML 1.2.

use std::borrow::Cow;

use granit_parser::{ScalarStyle, Tag};
use ordered_float::OrderedFloat;

#[derive(Debug, Clone, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub enum Scalar<'input> {
    Null,
    Boolean(bool),
    Integer(i64),
    FloatingPoint(OrderedFloat<f64>),
    String(Cow<'input, str>),
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub enum ScalarOwned {
    Null,
    Boolean(bool),
    Integer(i64),
    FloatingPoint(OrderedFloat<f64>),
    String(String),
}

impl<'input> Scalar<'input> {
    #[must_use]
    pub fn into_owned(self) -> ScalarOwned {
        match self {
            Self::Null => ScalarOwned::Null,
            Self::Boolean(v) => ScalarOwned::Boolean(v),
            Self::Integer(v) => ScalarOwned::Integer(v),
            Self::FloatingPoint(v) => ScalarOwned::FloatingPoint(v),
            Self::String(v) => ScalarOwned::String(v.into_owned()),
        }
    }

    pub fn parse_from_cow_and_metadata(
        v: Cow<'input, str>,
        style: ScalarStyle,
        tag: Option<&Cow<'input, Tag>>,
    ) -> Option<Self> {
        // An explicit core-schema tag fixes the type regardless of quoting style
        // (`!!int "1"` is the integer 1, not the string "1"), so it is resolved
        // before the non-plain-is-a-string fallback.
        match tag.map(Cow::as_ref) {
            Some(tag) if tag.is_yaml_core_schema() => match tag.suffix.as_str() {
                "bool" => parse_core_schema_bool(&v).map(Self::Boolean),
                "int" => parse_core_schema_int(&v).map(Self::Integer),
                "float" => parse_core_schema_fp(&v)
                    .map(OrderedFloat)
                    .map(Self::FloatingPoint),
                "null" => is_core_schema_null(&v).then_some(Self::Null),
                "str" => Some(Self::String(v)),
                _ => None,
            },
            _ if style != ScalarStyle::Plain => Some(Self::String(v)),
            _ => Some(Self::parse_from_cow(v)),
        }
    }

    #[must_use]
    pub fn parse_from_cow(v: Cow<'input, str>) -> Self {
        if let Some(integer) = parse_core_schema_int(&v) {
            return Self::Integer(integer);
        }
        if is_core_schema_null(&v) {
            return Self::Null;
        }
        if let Some(boolean) = parse_core_schema_bool(&v) {
            return Self::Boolean(boolean);
        }
        parse_core_schema_fp(&v).map_or_else(
            || Self::String(v),
            |float| Self::FloatingPoint(float.into()),
        )
    }
}

/// Parse a YAML 1.2 core-schema boolean, accepting every spelling the schema's
/// `true|True|TRUE|false|False|FALSE` production allows. Shared so an explicitly
/// `!!bool`-tagged scalar resolves the same spellings as an untagged one
/// (`!!bool TRUE` == `true`).
#[must_use]
pub fn parse_core_schema_bool(v: &str) -> Option<bool> {
    match v {
        "true" | "True" | "TRUE" => Some(true),
        "false" | "False" | "FALSE" => Some(false),
        _ => None,
    }
}

/// Whether `v` is a YAML 1.2 core-schema null spelling (`~|null|Null|NULL`).
/// Shared so an explicitly `!!null`-tagged scalar resolves the same spellings as
/// an untagged one (`!!null NULL` == `~`).
#[must_use]
pub fn is_core_schema_null(v: &str) -> bool {
    matches!(v, "~" | "null" | "Null" | "NULL")
}

/// Parse a YAML 1.2 core-schema integer, honouring the `0x`/`0o` radix prefixes
/// and a leading `+`. Shared so an explicitly `!!int`-tagged scalar resolves the
/// same spellings as an untagged one (`!!int 0xB` == `11`).
#[must_use]
pub fn parse_core_schema_int(v: &str) -> Option<i64> {
    if let Some(hex) = v.strip_prefix("0x") {
        i64::from_str_radix(hex, 16).ok()
    } else if let Some(octal) = v.strip_prefix("0o") {
        i64::from_str_radix(octal, 8).ok()
    } else {
        v.parse::<i64>().ok()
    }
}

#[must_use]
pub fn parse_core_schema_fp(v: &str) -> Option<f64> {
    match v {
        ".inf" | ".Inf" | ".INF" | "+.inf" | "+.Inf" | "+.INF" => Some(f64::INFINITY),
        "-.inf" | "-.Inf" | "-.INF" => Some(f64::NEG_INFINITY),
        ".nan" | ".NaN" | ".NAN" => Some(f64::NAN),
        _ if v.as_bytes().iter().any(u8::is_ascii_digit) => v.parse::<f64>().ok(),
        _ => None,
    }
}
