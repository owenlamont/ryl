// Vendored from saphyr v0.0.6 (`saphyr/src/scalar.rs`).
// Trimmed to the surface ryl needs: `ScalarOwned` with parse helpers, plus the
// borrowed-lifetime `Scalar` used when parsing scalars from the event stream.
// The original is dual-licensed MIT OR Apache-2.0; ryl ships under MIT.

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
        if style != ScalarStyle::Plain {
            return Some(Self::String(v));
        }
        match tag.map(Cow::as_ref) {
            Some(tag) if tag.is_yaml_core_schema() => match tag.suffix.as_str() {
                "bool" => v.parse::<bool>().ok().map(Self::Boolean),
                "int" => v.parse::<i64>().ok().map(Self::Integer),
                "float" => parse_core_schema_fp(&v)
                    .map(OrderedFloat)
                    .map(Self::FloatingPoint),
                "null" => match v.as_ref() {
                    "~" | "null" => Some(Self::Null),
                    _ => None,
                },
                "str" => Some(Self::String(v)),
                _ => None,
            },
            _ => Some(Self::parse_from_cow(v)),
        }
    }

    #[must_use]
    pub fn parse_from_cow(v: Cow<'input, str>) -> Self {
        let s = &*v;
        let bytes = s.as_bytes();

        if bytes.len() >= 2 {
            match (bytes[0], bytes[1]) {
                (b'0', b'x') => {
                    if let Ok(i) = i64::from_str_radix(&s[2..], 16) {
                        return Self::Integer(i);
                    }
                }
                (b'0', b'o') => {
                    if let Ok(i) = i64::from_str_radix(&s[2..], 8) {
                        return Self::Integer(i);
                    }
                }
                (b'+', _) => {
                    if let Ok(i) = s[1..].parse::<i64>() {
                        return Self::Integer(i);
                    }
                }
                _ => {}
            }
        }

        match bytes.len() {
            1 if bytes[0] == b'~' => return Self::Null,
            4 => {
                let folded = bytes[0] & 0xDF;
                if folded == b'N' && matches!(s, "null" | "Null" | "NULL") {
                    return Self::Null;
                } else if folded == b'T' && matches!(s, "true" | "True" | "TRUE") {
                    return Self::Boolean(true);
                }
            }
            5 if matches!(s, "false" | "False" | "FALSE") => {
                return Self::Boolean(false);
            }
            _ => {}
        }

        if let Ok(integer) = s.parse::<i64>() {
            return Self::Integer(integer);
        }

        if let Some(float) = parse_core_schema_fp(s) {
            return Self::FloatingPoint(float.into());
        }

        Self::String(v)
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
