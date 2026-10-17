use std::borrow::Cow;
use std::ops::Range;

use granit_parser::{Event, Placement, ScalarStyle, Span, Tag};

pub(super) enum Text {
    Source(Range<usize>),
    Decoded(String),
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
                    Self::Decoded(value.to_string())
                }
            }
            Cow::Owned(value) => Self::Decoded(value),
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
    Scalar(Text, ScalarStyle, usize, Option<Tag>),
    Comment(Text, Placement),
    Other(Event<'static>),
}

impl StoredEvent {
    pub(super) fn new(source: &str, event: Event<'_>) -> Self {
        let event = match event {
            Event::Scalar(value, style, anchor, tag) => {
                return Self::Scalar(
                    Text::new(source, value),
                    style,
                    anchor,
                    tag.map(Cow::into_owned),
                );
            }
            Event::Comment(value, placement) => {
                return Self::Comment(Text::new(source, value), placement);
            }
            Event::SequenceStart(style, anchor, tag) => Some(Event::SequenceStart(
                style,
                anchor,
                tag.map(|tag| Cow::Owned(tag.into_owned())),
            )),
            Event::MappingStart(style, anchor, tag) => Some(Event::MappingStart(
                style,
                anchor,
                tag.map(|tag| Cow::Owned(tag.into_owned())),
            )),
            Event::StreamStart => Some(Event::StreamStart),
            Event::StreamEnd => Some(Event::StreamEnd),
            Event::DocumentStart(explicit, version) => {
                Some(Event::DocumentStart(explicit, version))
            }
            Event::DocumentEnd => Some(Event::DocumentEnd),
            Event::Alias(anchor) => Some(Event::Alias(anchor)),
            Event::SequenceEnd => Some(Event::SequenceEnd),
            end => (end == Event::MappingEnd).then_some(Event::MappingEnd),
        };
        Self::Other(event.expect("all granit events are represented"))
    }

    pub(super) fn borrow<'a>(&'a self, source: &'a str) -> Event<'a> {
        match self {
            Self::Scalar(value, style, anchor, tag) => Event::Scalar(
                Cow::Borrowed(value.get(source)),
                *style,
                *anchor,
                tag.as_ref().map(Cow::Borrowed),
            ),
            Self::Comment(value, placement) => {
                Event::Comment(Cow::Borrowed(value.get(source)), *placement)
            }
            Self::Other(event) => event.clone(),
        }
    }
}

pub(super) fn borrow_events<'a>(
    source: &'a str,
    events: &'a [(StoredEvent, Span)],
) -> Vec<(Event<'a>, Span)> {
    events
        .iter()
        .map(|(event, span)| (event.borrow(source), *span))
        .collect()
}
