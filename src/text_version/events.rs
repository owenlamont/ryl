use std::borrow::Cow;
use std::ops::Range;

use granit_parser::{Event, Placement, ScalarStyle, StructureStyle, Tag, YamlVersion};

pub(super) enum Text {
    Source(Range<usize>),
    Decoded(Box<str>),
}

impl Text {
    fn new(source: &str, value: Cow<'_, str>) -> Self {
        match value {
            Cow::Borrowed(value) => {
                let start =
                    (value.as_ptr() as usize).wrapping_sub(source.as_ptr() as usize);
                let end = start.saturating_add(value.len());
                if source.get(start..end) == Some(value) {
                    Self::Source(start..end)
                } else {
                    Self::Decoded(value.into())
                }
            }
            Cow::Owned(value) => Self::Decoded(value.into_boxed_str()),
        }
    }

    fn get<'a>(&'a self, source: &'a str) -> &'a str {
        match self {
            Self::Source(range) => &source[range.clone()],
            Self::Decoded(value) => value,
        }
    }
}

pub(super) enum StoredEvent {
    Scalar(Text, ScalarStyle, usize, Option<Box<Tag>>),
    Comment(Text, Placement),
    StreamStart,
    StreamEnd,
    DocumentStart(bool, Option<YamlVersion>),
    DocumentEnd,
    Alias(usize),
    SequenceStart(StructureStyle, usize, Option<Box<Tag>>),
    SequenceEnd,
    MappingStart(StructureStyle, usize, Option<Box<Tag>>),
    MappingEnd,
}

impl StoredEvent {
    pub(super) fn significant(&self) -> bool {
        !matches!(
            self,
            Self::Comment(..) | Self::StreamStart | Self::StreamEnd
        )
    }

    pub(super) fn equivalent(
        &self,
        other: &Self,
        source: &str,
        rewritten: &str,
    ) -> bool {
        if let (
            Self::Scalar(left, left_style, left_anchor, left_tag),
            Self::Scalar(right, right_style, right_anchor, right_tag),
        ) = (self, other)
        {
            left.get(source) == right.get(rewritten)
                && left_style == right_style
                && left_anchor == right_anchor
                && left_tag == right_tag
        } else {
            self.borrow(source) == other.borrow(rewritten)
        }
    }

    pub(super) fn new(source: &str, event: Event<'_>) -> Self {
        let tag = |tag: Option<Cow<'_, Tag>>| tag.map(|tag| Box::new(tag.into_owned()));
        match event {
            Event::Scalar(value, style, anchor, metadata) => {
                Self::Scalar(Text::new(source, value), style, anchor, tag(metadata))
            }
            Event::Comment(value, placement) => {
                Self::Comment(Text::new(source, value), placement)
            }
            Event::SequenceStart(style, anchor, metadata) => {
                Self::SequenceStart(style, anchor, tag(metadata))
            }
            Event::MappingStart(style, anchor, metadata) => {
                Self::MappingStart(style, anchor, tag(metadata))
            }
            Event::StreamStart => Self::StreamStart,
            Event::StreamEnd => Self::StreamEnd,
            Event::DocumentStart(explicit, version) => {
                Self::DocumentStart(explicit, version)
            }
            Event::DocumentEnd => Self::DocumentEnd,
            Event::Alias(anchor) => Self::Alias(anchor),
            Event::SequenceEnd => Self::SequenceEnd,
            end => (end == Event::MappingEnd)
                .then_some(Self::MappingEnd)
                .expect("all granit events are represented"),
        }
    }

    pub(super) fn borrow<'a>(&'a self, source: &'a str) -> Event<'a> {
        match self {
            Self::Scalar(value, style, anchor, tag) => Event::Scalar(
                Cow::Borrowed(value.get(source)),
                *style,
                *anchor,
                tag.as_deref().map(Cow::Borrowed),
            ),
            Self::Comment(value, placement) => {
                Event::Comment(Cow::Borrowed(value.get(source)), *placement)
            }
            Self::StreamStart => Event::StreamStart,
            Self::StreamEnd => Event::StreamEnd,
            Self::DocumentStart(explicit, version) => {
                Event::DocumentStart(*explicit, *version)
            }
            Self::DocumentEnd => Event::DocumentEnd,
            Self::Alias(anchor) => Event::Alias(*anchor),
            Self::SequenceStart(style, anchor, tag) => {
                Event::SequenceStart(*style, *anchor, tag.as_deref().map(Cow::Borrowed))
            }
            Self::SequenceEnd => Event::SequenceEnd,
            Self::MappingStart(style, anchor, tag) => {
                Event::MappingStart(*style, *anchor, tag.as_deref().map(Cow::Borrowed))
            }
            Self::MappingEnd => Event::MappingEnd,
        }
    }
}
