use std::num::NonZeroU8;

use serde_json::{Map, Value};

use crate::config_schema::FormatTable;

pub(super) struct Converted {
    pub(super) excludes: Vec<String>,
    pub(super) targets: FormatTable,
    pub(super) indent: Option<u8>,
    pub(super) width: Option<u16>,
    pub(super) warnings: Vec<String>,
    pub(super) cleanup_safe: bool,
}

pub(super) struct Options {
    tool: &'static str,
    values: Map<String, Value>,
    default_nulls: bool,
    truncate_integers: bool,
    warnings: Vec<String>,
}

impl Options {
    pub(super) fn new(tool: &'static str, mut values: Map<String, Value>) -> Self {
        values.remove("$schema");
        Self {
            tool,
            values,
            default_nulls: false,
            truncate_integers: false,
            warnings: Vec::new(),
        }
    }

    pub(super) fn take(&mut self, key: &str) -> Option<Value> {
        self.values.remove(key)
    }

    pub(super) fn values_match(&self, key: &str, value: &str) -> bool {
        self.values.get(key).and_then(Value::as_str) == Some(value)
    }

    pub(super) fn strings(&mut self, key: &str) -> Result<Vec<String>, String> {
        match self.typed(key) {
            None => Ok(Vec::new()),
            Some(Value::Array(values)) if values.iter().all(Value::is_string) => {
                Ok(values
                    .into_iter()
                    .map(|value| value.as_str().expect("validated string").to_owned())
                    .collect())
            }
            Some(_) => Err(self.expected(key, "an array of strings")),
        }
    }

    pub(super) fn with_null_defaults(mut self) -> Self {
        self.default_nulls = true;
        self
    }

    pub(super) fn with_integer_truncation(mut self) -> Self {
        self.truncate_integers = true;
        self
    }

    pub(super) fn typed(&mut self, key: &str) -> Option<Value> {
        self.take(key)
            .filter(|value| !self.default_nulls || !value.is_null())
    }

    pub(super) fn boolean(&mut self, key: &str, default: bool) -> Result<bool, String> {
        match self.typed(key) {
            None => Ok(default),
            Some(Value::Bool(value)) => Ok(value),
            Some(_) => Err(self.expected(key, "a boolean")),
        }
    }

    pub(super) fn optional_boolean(
        &mut self,
        key: &str,
    ) -> Result<Option<bool>, String> {
        match self.typed(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Bool(value)) => Ok(Some(value)),
            Some(_) => Err(self.expected(key, "a boolean or null")),
        }
    }

    #[expect(
        clippy::cast_possible_truncation,
        reason = "the float is finite, integral and range-checked before conversion"
    )]
    pub(super) fn integer(&mut self, key: &str, default: i64) -> Result<i64, String> {
        let Some(value) = self.typed(key) else {
            return Ok(default);
        };
        if let Some(integer) = value.as_i64() {
            return Ok(integer);
        }
        let Some(number) = value
            .as_f64()
            .filter(|number| self.truncate_integers || number.fract() == 0.0)
        else {
            return Err(self.expected(key, "an integer"));
        };
        if (-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&number)
        {
            return Ok(number as i64);
        }
        self.warn(key, &value, &format!("outside the representable integer range; using source default {default}"));
        Ok(default)
    }

    pub(super) fn strict_integer(
        &mut self,
        key: &str,
        default: i64,
    ) -> Result<i64, String> {
        let Some(value) = self.typed(key) else {
            return Ok(default);
        };
        value
            .as_i64()
            .ok_or_else(|| self.expected(key, "an integer literal"))
    }

    pub(super) fn text(&mut self, key: &str, default: &str) -> Result<String, String> {
        match self.typed(key) {
            None => Ok(default.to_owned()),
            Some(Value::String(value)) => Ok(value),
            Some(_) => Err(self.expected(key, "a string")),
        }
    }

    pub(super) fn choice(
        &mut self,
        key: &str,
        default: &str,
        choices: &[&str],
    ) -> Result<String, String> {
        let value = self.text(key, default)?;
        if choices.contains(&value.as_str()) {
            Ok(value)
        } else {
            Err(self.expected(key, &format!("one of {}", choices.join(", "))))
        }
    }

    pub(super) fn dimension<T>(&mut self, key: &str, value: i64) -> Option<T>
    where
        T: TryFrom<i64>,
    {
        if value > 0
            && let Ok(value) = T::try_from(value)
        {
            return Some(value);
        }
        self.warn(
            key,
            &Value::from(value),
            "outside ryl's representable range; retaining a supported fallback",
        );
        None
    }

    pub(super) fn spacing(&mut self, key: &str, value: i64, default: u8) -> NonZeroU8 {
        self.dimension::<u8>(key, value)
            .and_then(NonZeroU8::new)
            .unwrap_or_else(|| {
                NonZeroU8::new(default).expect("the spacing default is positive")
            })
    }

    pub(super) fn blank_limit(&mut self, key: &str, value: i64) -> u8 {
        u8::try_from(value).unwrap_or_else(|_| {
            self.warn(
                key,
                &Value::from(value),
                "outside ryl's blank-line range; retaining its default of 2",
            );
            2
        })
    }

    pub(super) fn unmapped(
        &mut self,
        key: &str,
        reason: &str,
        expected: &str,
        valid: impl FnOnce(&Value) -> bool,
    ) -> Result<(), String> {
        if let Some(value) = self.take(key) {
            if !valid(&value) {
                return Err(self.expected(key, expected));
            }
            self.warn(key, &value, reason);
        }
        Ok(())
    }

    pub(super) fn warn(&mut self, key: &str, value: &Value, reason: &str) {
        self.warnings.push(format!(
            "{} option {} = {}: {}",
            self.tool, key, value, reason
        ));
    }

    pub(super) fn note(&mut self, reason: &str) {
        self.warnings.push(format!("{}: {reason}", self.tool));
    }

    pub(super) fn finish(
        self,
        targets: FormatTable,
        indent: Option<u8>,
        width: Option<u16>,
    ) -> Converted {
        let cleanup_safe = self.values.is_empty();
        Converted {
            excludes: Vec::new(),
            cleanup_safe,
            targets,
            indent,
            width,
            warnings: self.into_warnings(),
        }
    }

    pub(super) fn has_unknown_options(&self) -> bool {
        !self.values.is_empty()
    }

    pub(super) fn into_warnings(mut self) -> Vec<String> {
        for (key, value) in std::mem::take(&mut self.values) {
            self.warn(
                &key,
                &value,
                "not imported; no equivalent YAML formatter setting",
            );
        }
        self.warnings
    }

    fn expected(&self, key: &str, expected: &str) -> String {
        format!(
            "invalid {} option {}: expected {expected}",
            self.tool,
            escaped(key)
        )
    }
}

pub(super) fn escaped(text: &str) -> String {
    crate::cli_support::sanitize_control(text).into_owned()
}

pub(super) fn strings(value: &Value) -> bool {
    value
        .as_array()
        .is_some_and(|items| items.iter().all(Value::is_string))
}

pub(super) fn string_or_strings(value: &Value) -> bool {
    value.is_string() || strings(value)
}

pub(super) fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_f64() != Some(0.0),
        Value::String(value) => !value.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}
