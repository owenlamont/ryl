use ryl_yaml_parser::ScalarStyle;

use super::{QuoteStyle, quote_value, target};

pub(crate) struct Spelling(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub(crate) struct LayoutStyle(ScalarStyle);

impl LayoutStyle {
    pub(super) const fn retained(style: ScalarStyle) -> Self {
        Self(style)
    }

    pub(crate) const fn is_folded(self) -> bool {
        matches!(self.0, ScalarStyle::Folded)
    }
    pub(crate) const fn is_plain(self) -> bool {
        matches!(self.0, ScalarStyle::Plain)
    }
    pub(crate) const fn is_double(self) -> bool {
        matches!(self.0, ScalarStyle::DoubleQuoted)
    }
    pub(crate) const fn foldable(self) -> bool {
        !matches!(self.0, ScalarStyle::Literal)
    }
    pub(crate) const fn quote_width(self) -> usize {
        match self.0 {
            ScalarStyle::SingleQuoted | ScalarStyle::DoubleQuoted => 1,
            _ => 0,
        }
    }

    pub(crate) const fn is_block(self) -> bool {
        matches!(self.0, ScalarStyle::Literal | ScalarStyle::Folded)
    }

    pub(crate) const fn inline_content_offset(self) -> Option<usize> {
        match self.0 {
            ScalarStyle::Plain => Some(0),
            ScalarStyle::SingleQuoted | ScalarStyle::DoubleQuoted => Some(1),
            ScalarStyle::Literal | ScalarStyle::Folded => None,
        }
    }
}

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

    pub(crate) fn cursor(&self) -> StyleCursor<'_> {
        StyleCursor(&self.0)
    }

    pub(crate) fn at(&self, start: usize) -> Option<ScalarStyle> {
        self.0
            .binary_search_by_key(&start, |(at, _)| *at)
            .ok()
            .map(|index| self.0[index].1)
    }

    pub(crate) fn layout_at(&self, start: usize) -> Option<LayoutStyle> {
        self.at(start).map(LayoutStyle)
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

pub(crate) struct StyleCursor<'a>(&'a [(usize, ScalarStyle)]);

impl StyleCursor<'_> {
    pub(crate) fn layout_at(&mut self, start: usize) -> Option<LayoutStyle> {
        while self.0.first().is_some_and(|(at, _)| *at < start) {
            self.0 = &self.0[1..];
        }
        self.0
            .first()
            .filter(|(at, _)| *at == start)
            .map(|(_, style)| LayoutStyle(*style))
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

    pub(crate) fn empty(self) -> bool {
        self.text.is_empty()
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
