use ryl_yaml_parser::ScalarStyle;

pub(crate) struct RetainedRegions<'a> {
    input: crate::text_version::Analysis<'a>,
}

impl<'a> RetainedRegions<'a> {
    pub(super) const fn new(input: crate::text_version::Analysis<'a>) -> Self {
        Self { input }
    }

    pub(crate) fn contains(&self, at: usize) -> bool {
        self.input.parsed().scalar_contains(at)
    }
}

pub(crate) struct RetainedShape<'a> {
    buffer: &'a str,
    range: std::ops::Range<usize>,
}

impl<'a> RetainedShape<'a> {
    pub(super) const fn new(buffer: &'a str, range: std::ops::Range<usize>) -> Self {
        Self { buffer, range }
    }

    pub(crate) fn indentation_indicator(&self) -> bool {
        self.buffer[self.range.start + 1..]
            .chars()
            .take_while(|&ch| ch.is_ascii_digit() || matches!(ch, '+' | '-'))
            .any(|ch| ch.is_ascii_digit())
    }

    pub(crate) fn trailing_breaks(&self) -> usize {
        let from = self.buffer[..self.range.start]
            .char_indices()
            .next_back()
            .map_or(0, |(at, _)| at);
        self.buffer[from..self.range.end]
            .char_indices()
            .rev()
            .take_while(|(_, ch)| {
                matches!(ch, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c')
            })
            .filter(|(at, ch)| {
                *ch == '\n'
                    || (*ch == '\r' && !self.buffer[from + at + 1..].starts_with('\n'))
            })
            .count()
    }
}

#[derive(Clone, Copy)]
pub(super) struct Initial<'a> {
    pub(super) style: ScalarStyle,
    content: &'a str,
}

impl<'a> Initial<'a> {
    pub(super) const fn new(style: ScalarStyle, content: &'a str) -> Self {
        Self { style, content }
    }

    pub(super) fn any_escape(self) -> bool {
        self.content.contains('\\')
    }

    pub(super) fn non_quote_escape(self) -> bool {
        self.content
            .match_indices('\\')
            .any(|(at, _)| !self.content[at + 1..].starts_with('"'))
    }

    pub(super) fn multiline(self) -> bool {
        self.content.contains('\n') || self.content.contains('\r')
    }

    pub(super) fn escaped_line_ending(self) -> bool {
        self.multiline()
            && (self.content.contains("\\\n") || self.content.contains("\\\r\n"))
    }
}

pub(crate) struct LineContent<'a> {
    text: &'a str,
}

impl<'a> LineContent<'a> {
    pub(super) const fn new(text: &'a str) -> Self {
        Self { text }
    }
    pub(crate) const fn empty(&self) -> bool {
        self.text.is_empty()
    }
    pub(crate) fn starts_indent(&self) -> bool {
        self.text.starts_with([' ', '\t'])
    }
    pub(crate) fn blank(&self) -> bool {
        self.text.trim().is_empty()
    }
}

#[derive(PartialEq, Eq)]
pub(crate) enum ContentKind {
    Empty,
    Whitespace,
    Text,
}

impl LineContent<'_> {
    pub(crate) fn kind(&self) -> ContentKind {
        if self.empty() {
            ContentKind::Empty
        } else if self.starts_indent() {
            ContentKind::Whitespace
        } else {
            ContentKind::Text
        }
    }
}

pub(crate) struct RetainedTail(bool);

impl RetainedTail {
    pub(super) fn observe(buffer: &str, end: usize) -> Self {
        Self(buffer[..end].ends_with(['\n', '\r']))
    }
    pub(crate) const fn ends_in_break(&self) -> bool {
        self.0
    }
}

impl LineContent<'_> {
    pub(crate) fn horizontal_whitespace(&self) -> bool {
        self.text.bytes().all(|ch| matches!(ch, b' ' | b'\t'))
    }
}

pub(super) fn constrain_keys(
    input: crate::text_version::Analysis<'_>,
    replacements: &mut Vec<super::Replacement>,
) -> Vec<super::Violation> {
    use crate::rules::support::mapping_key_walker::Walker;
    use crate::text_version::lexical::Kind;
    use ryl_yaml_parser::Event;

    let buffer = input.text();
    let grows = |&(start, end, ref spelling): &super::Replacement| {
        spelling.as_str().chars().count()
            > buffer[start.get()..end.get()].chars().count()
    };
    if !replacements.iter().any(grows) {
        return Vec::new();
    }
    let mut indicators = input
        .tokens()
        .iter(buffer)
        .filter_map(|(span, kind)| matches!(kind, Kind::Key).then_some(span))
        .peekable();
    let mut walker = Walker::new();
    let mut keys = Vec::new();
    for (event, span) in input.event_iter() {
        let simple = if walker.expects_key()
            && matches!(
                event,
                Event::Scalar(..)
                    | Event::Alias(_)
                    | Event::SequenceStart(..)
                    | Event::MappingStart(..)
            ) {
            indicators
                .next_if(|key| key.start.index() <= span.start.index())
                .filter(|key| key.slice(buffer) != Some("?"))
        } else {
            None
        };
        match event {
            Event::Scalar(_, style, _, _) => {
                let collection_key = walker.any_metadata(|&simple| simple);
                if simple.is_some() || collection_key {
                    keys.push((
                        simple.map_or(span.start, |key| key.start),
                        super::scalar_source_bounds(buffer, style, &span).1.get(),
                        collection_key,
                    ));
                }
                walker.skip_node();
            }
            Event::Alias(_) => walker.skip_node(),
            Event::MappingStart(..) => walker.enter_mapping((), simple.is_some()),
            Event::SequenceStart(..) => walker.enter_sequence(simple.is_some()),
            Event::MappingEnd | Event::SequenceEnd => walker.exit_container(),
            Event::DocumentStart(..) => walker.reset(),
            _ => {}
        }
    }
    let mut refusals = Vec::new();
    replacements.retain(|replacement| {
        if !grows(replacement) {
            return true;
        }
        let (start, end, spelling) = replacement;
        let after = spelling.as_str().chars().count();
        let before = keys.partition_point(|&(key, _, _)| super::marker_byte_offset(key).get() <= start.get());
        let Some(&(key, key_end, collection_key)) = before.checked_sub(1).map(|index| &keys[index]) else {
            return true;
        };
        if start.get() >= key_end {
            return true;
        }
        if collection_key {
            return false;
        }
        let key_start = super::marker_byte_offset(key).get();
        let characters = buffer[key_start..start.get()].chars().count()
            + after + buffer[end.get()..key_end].chars().count();
        if characters <= 1024 {
            return true;
        }
        refusals.push(super::Violation {
            line: key.line(), column: key.col() + 1,
            message: "cannot add required quotes: the implicit key would exceed 1024 characters; shorten the key or use an explicit `?` key".to_owned(),
        });
        false
    });
    refusals
}
