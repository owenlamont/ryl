use ryl_yaml_parser::ScalarStyle;

use super::{QuoteStyle, quote_value, target};

pub(crate) struct Spelling(String);

impl Spelling {
    pub(super) fn materialize(selected: target::Selected, retained: &str) -> Self {
        let text = match selected.into_recipe() {
            target::Recipe::Text(text) => text,
            target::Recipe::SingleRetainedMultiline => {
                quote_value(&retained.replace("\\\"", "\""), QuoteStyle::Single)
            }
        };
        Self(text)
    }

    pub(super) fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn style(&self) -> ScalarStyle {
        match self.0.as_bytes().first() {
            Some(b'\'') => ScalarStyle::SingleQuoted,
            Some(b'"') => ScalarStyle::DoubleQuoted,
            _ => ScalarStyle::Plain,
        }
    }

    pub(crate) fn into_text(self) -> String {
        self.0
    }
}
