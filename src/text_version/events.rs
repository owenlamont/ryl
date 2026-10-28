use std::borrow::Cow;

use ryl_yaml_parser::{
    Event, Placement, ScalarStyle, StructureStyle, Tag, YamlVersion,
};

#[derive(Default)]
pub(super) struct Tables {
    decoded: Vec<Box<str>>,
    nodes: Vec<(usize, Option<Box<Tag>>)>,
}

pub(super) enum Text {
    Source([u32; 2]),
    Decoded(u32),
}

impl Text {
    fn new(source: &str, value: Cow<'_, str>, tables: &mut Tables) -> Self {
        match value {
            Cow::Borrowed(value) => {
                let start =
                    (value.as_ptr() as usize).wrapping_sub(source.as_ptr() as usize);
                let end = start.saturating_add(value.len());
                if source.get(start..end) == Some(value)
                    && let (Ok(start), Ok(end)) =
                        (u32::try_from(start), u32::try_from(end))
                {
                    Self::Source([start, end])
                } else {
                    Self::decoded(value.into(), tables)
                }
            }
            Cow::Owned(value) => Self::decoded(value.into_boxed_str(), tables),
        }
    }

    fn decoded(value: Box<str>, tables: &mut Tables) -> Self {
        let index =
            u32::try_from(tables.decoded.len()).expect("decoded value count fits u32");
        tables.decoded.push(value);
        Self::Decoded(index)
    }

    fn get<'a>(&'a self, source: &'a str, tables: &'a Tables) -> &'a str {
        match self {
            Self::Source([start, end]) => &source[*start as usize..*end as usize],
            Self::Decoded(index) => &tables.decoded[*index as usize],
        }
    }
}

pub(super) enum StoredEvent {
    Scalar(Text, ScalarStyle, u32),
    Comment(Text, Placement),
    StreamStart,
    StreamEnd,
    DocumentStart(bool, Option<YamlVersion>),
    DocumentEnd,
    Alias(usize),
    SequenceStart(StructureStyle, u32),
    SequenceEnd,
    MappingStart(StructureStyle, u32),
    MappingEnd,
}

impl StoredEvent {
    pub(super) fn new(source: &str, event: Event<'_>, tables: &mut Tables) -> Self {
        fn node(anchor: usize, tag: Option<Cow<'_, Tag>>, tables: &mut Tables) -> u32 {
            if anchor == 0 && tag.is_none() {
                return 0;
            }
            tables
                .nodes
                .push((anchor, tag.map(|tag| Box::new(tag.into_owned()))));
            u32::try_from(tables.nodes.len()).expect("node metadata count fits u32")
        }
        match event {
            Event::Scalar(value, style, anchor, metadata) => Self::Scalar(
                Text::new(source, value, tables),
                style,
                node(anchor, metadata, tables),
            ),
            Event::Comment(value, placement) => {
                Self::Comment(Text::new(source, value, tables), placement)
            }
            Event::SequenceStart(style, anchor, metadata) => {
                Self::SequenceStart(style, node(anchor, metadata, tables))
            }
            Event::MappingStart(style, anchor, metadata) => {
                Self::MappingStart(style, node(anchor, metadata, tables))
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
                .expect("all parser events are represented"),
        }
    }

    pub(super) fn borrow<'a>(
        &'a self,
        source: &'a str,
        tables: &'a Tables,
    ) -> Event<'a> {
        let node = |index: u32| {
            if index == 0 {
                (0, None)
            } else {
                let (anchor, tag) = &tables.nodes[index as usize - 1];
                (*anchor, tag.as_deref().map(Cow::Borrowed))
            }
        };
        match self {
            Self::Scalar(value, style, metadata) => {
                let (anchor, tag) = node(*metadata);
                Event::Scalar(
                    Cow::Borrowed(value.get(source, tables)),
                    *style,
                    anchor,
                    tag,
                )
            }
            Self::Comment(value, placement) => {
                Event::Comment(Cow::Borrowed(value.get(source, tables)), *placement)
            }
            Self::StreamStart => Event::StreamStart,
            Self::StreamEnd => Event::StreamEnd,
            Self::DocumentStart(explicit, version) => {
                Event::DocumentStart(*explicit, *version)
            }
            Self::DocumentEnd => Event::DocumentEnd,
            Self::Alias(anchor) => Event::Alias(*anchor),
            Self::SequenceStart(style, metadata) => {
                let (anchor, tag) = node(*metadata);
                Event::SequenceStart(*style, anchor, tag)
            }
            Self::SequenceEnd => Event::SequenceEnd,
            Self::MappingStart(style, metadata) => {
                let (anchor, tag) = node(*metadata);
                Event::MappingStart(*style, anchor, tag)
            }
            Self::MappingEnd => Event::MappingEnd,
        }
    }
}
