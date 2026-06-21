// Derived from saphyr (`saphyr/src/scalar.rs`), MIT OR Apache-2.0; ryl ships under
// MIT. `resolve_plain_scalar` resolves null/bool per the YAML 1.2 core schema, so
// `True`/`Null` are bool/null; `Yes`/`No`/`On`/`Off` stay strings because they are
// YAML 1.1 booleans and ryl targets YAML 1.2.

use std::borrow::Cow;

use granit_parser::{ScalarStyle, Tag};
use ordered_float::OrderedFloat;

use super::core_schema_suffix;

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

    pub fn resolve_scalar(
        v: Cow<'input, str>,
        style: ScalarStyle,
        tag: Option<&Cow<'input, Tag>>,
    ) -> Option<Self> {
        // An explicit core tag fixes the type regardless of quoting (`!!int "1"` is
        // integer 1), so it resolves before the non-plain-is-a-string fallback.
        // Matching the suffix (not the handle) makes a verbatim
        // `!<tag:yaml.org,2002:int>` resolve like the `!!int` shorthand.
        match tag.map(Cow::as_ref).and_then(core_schema_suffix).as_deref() {
            Some("bool") => parse_core_schema_bool(&v).map(Self::Boolean),
            Some("int") => parse_core_schema_int(&v).map(Self::Integer),
            Some("float") => parse_core_schema_fp(&v)
                .map(OrderedFloat)
                .map(Self::FloatingPoint),
            Some("null") => is_core_schema_null(&v).then_some(Self::Null),
            // `merge` resolves to the same string identity as a plain `<<`, so the two
            // merge-key spellings are recognised as one key.
            Some("str" | "merge") => Some(Self::String(v)),
            // A core tag naming a non-scalar type (`!!seq`, `!!map`) cannot resolve a
            // scalar value.
            Some(_) => None,
            None if style != ScalarStyle::Plain => Some(Self::String(v)),
            None => Some(Self::resolve_plain_scalar(v)),
        }
    }

    #[must_use]
    pub fn resolve_plain_scalar(v: Cow<'input, str>) -> Self {
        if let Some(integer) = parse_core_schema_int(&v) {
            return Self::Integer(integer);
        }
        // A decimal integer overflowing `i64` keeps its exact text rather than
        // reparsing as `f64`, which would collapse distinct large integers onto one
        // value (a false-positive duplicate key under `check-canonical`). Hex/octal
        // overflow spellings already fall through to a string (they cannot parse as
        // `f64`).
        if is_decimal_integer_spelling(&v) {
            return Self::String(v);
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

/// A YAML 1.2 core-schema boolean (`true|True|TRUE|false|False|FALSE`). Shared so a
/// `!!bool`-tagged scalar resolves the same spellings as an untagged one.
#[must_use]
pub fn parse_core_schema_bool(v: &str) -> Option<bool> {
    match v {
        "true" | "True" | "TRUE" => Some(true),
        "false" | "False" | "FALSE" => Some(false),
        _ => None,
    }
}

/// A YAML 1.2 core-schema null spelling (`~|null|Null|NULL`, plus the empty plain
/// scalar). Shared so a `!!null`-tagged scalar resolves the same spellings as an
/// untagged one; a quoted empty scalar stays a string (non-plain scalars resolve
/// before this is reached).
#[must_use]
pub fn is_core_schema_null(v: &str) -> bool {
    matches!(v, "" | "~" | "null" | "Null" | "NULL")
}

/// A decimal integer spelling (`[-+]?[0-9]+`). Reached only after
/// `parse_core_schema_int` fails, so `true` means an integer that overflows `i64`.
#[must_use]
fn is_decimal_integer_spelling(v: &str) -> bool {
    let digits = v.strip_prefix(['+', '-']).unwrap_or(v);
    !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
}

/// A YAML 1.2 core-schema integer, honouring `0x`/`0o` radix prefixes and a leading
/// `+`. Shared so a `!!int`-tagged scalar resolves the same spellings as an untagged
/// one (`!!int 0xB` == `11`).
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
