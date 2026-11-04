use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Number, Value};
use std::fmt;

const MAX_NESTING: usize = 128;

pub(super) fn parse(text: &str) -> Result<Value, json5::Error> {
    json5::from_str::<Bounded>(text).map(|parsed| parsed.0)
}

pub(super) fn parse_jsonc(
    text: &str,
) -> Result<Value, jsonc_parser::errors::ParseError> {
    jsonc_parser::parse_to_serde_value::<Bounded>(
        text,
        &jsonc_parser::ParseOptions {
            allow_hexadecimal_numbers: false,
            allow_bare_decimal_point_numbers: false,
            ..Default::default()
        },
    )
    .map(|parsed| parsed.0)
}

struct Bounded(Value);
struct Seed(usize);
impl<'de> Deserialize<'de> for Bounded {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Seed(0).deserialize(deserializer).map(Self)
    }
}
impl<'de> DeserializeSeed<'de> for Seed {
    type Value = Value;
    fn deserialize<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Value, D::Error> {
        deserializer.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for Seed {
    type Value = Value;
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(
            "a finite configuration value with at most 128 nested containers",
        )
    }
    fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Value, E> {
        Ok(Value::Bool(value))
    }
    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Value, E> {
        Ok(value.into())
    }
    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Value, E> {
        Ok(value.into())
    }
    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Value, E> {
        Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| E::custom("non-finite JSON5 numbers are not supported"))
    }
    fn visit_str<E: de::Error>(self, value: &str) -> Result<Value, E> {
        Ok(value.into())
    }
    fn visit_string<E: de::Error>(self, value: String) -> Result<Value, E> {
        Ok(value.into())
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Value, A::Error> {
        if self.0 >= MAX_NESTING {
            return Err(de::Error::invalid_length(self.0 + 1, &self));
        }
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(Seed(self.0 + 1))? {
            values.push(value);
        }
        Ok(Value::Array(values))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
        if self.0 >= MAX_NESTING {
            return Err(de::Error::invalid_length(self.0 + 1, &self));
        }
        let mut values = Map::new();
        while let Some(key) = map.next_key::<String>()? {
            values.insert(key, map.next_value_seed(Seed(self.0 + 1))?);
        }
        Ok(Value::Object(values))
    }
}
