use ryl_yaml_parser::ScalarStyle;

use super::{QuoteStyle, quote_value, target};

pub(crate) struct Spelling(String);

#[derive(Default)]
pub(crate) struct Styles(Vec<(usize, ScalarStyle)>);

impl Styles {
    pub(super) fn selected<'a>(
        replacements: impl Iterator<Item = (usize, &'a Spelling)>,
    ) -> Self {
        let mut styles: Vec<_> = replacements
            .map(|(at, spelling)| (at, spelling.style()))
            .collect();
        styles.sort_by_key(|(at, _)| *at);
        Self(styles)
    }

    pub(crate) fn at(&self, start: usize) -> Option<ScalarStyle> {
        self.0
            .binary_search_by_key(&start, |(at, _)| *at)
            .ok()
            .map(|index| self.0[index].1)
    }

    pub(crate) fn restyled(&self) -> Restyled {
        Restyled(
            self.0
                .iter()
                .filter(|(_, style)| *style != ScalarStyle::DoubleQuoted)
                .map(|(start, _)| *start)
                .collect(),
        )
    }
}

#[derive(Default)]
pub(crate) struct Restyled(Vec<usize>);

impl Restyled {
    pub(crate) fn contains(&self, start: usize) -> bool {
        self.0.binary_search(&start).is_ok()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Retained<'a> {
    style: ScalarStyle,
    text: &'a str,
}

impl<'a> Retained<'a> {
    pub(super) const fn new(style: ScalarStyle, text: &'a str) -> Self {
        Self { style, text }
    }

    pub(crate) fn flow_compatible(self) -> bool {
        match self.style {
            ScalarStyle::Plain => {
                !self.text.is_empty()
                    && !self.text.contains([',', '[', ']', '{', '}'])
                    && !self.text.starts_with([':', '?'])
            }
            ScalarStyle::SingleQuoted | ScalarStyle::DoubleQuoted => true,
            ScalarStyle::Literal | ScalarStyle::Folded => false,
        }
    }

    pub(crate) fn starts_question(self) -> bool {
        self.is_plain() && self.text.starts_with('?')
    }

    pub(crate) fn is_plain(self) -> bool {
        self.style == ScalarStyle::Plain
    }
}

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
