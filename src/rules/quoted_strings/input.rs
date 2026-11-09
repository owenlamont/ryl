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

struct SimpleKey {
    start: ryl_yaml_parser::Marker,
    end: usize,
    parent: Option<usize>,
    characters: usize,
}

fn simple_keys(input: crate::text_version::Analysis<'_>) -> Vec<SimpleKey> {
    use crate::rules::support::mapping_key_walker::Walker;
    use crate::text_version::lexical::Kind;
    use ryl_yaml_parser::Event;

    let buffer = input.text();
    let mut indicators = input
        .tokens()
        .iter(buffer)
        .filter_map(|(span, kind)| matches!(kind, Kind::Key).then_some(span))
        .peekable();
    let mut walker = Walker::new();
    let mut keys: Vec<SimpleKey> = Vec::new();
    let mut containers: Vec<(Option<usize>, Option<usize>)> = Vec::new();
    for (event, span) in input.event_iter() {
        let parent = containers.last().and_then(|&(own, parent)| own.or(parent));
        let key_node = matches!(
            event,
            Event::Scalar(..)
                | Event::Alias(_)
                | Event::SequenceStart(..)
                | Event::MappingStart(..)
        );
        let own = if key_node && walker.expects_key() {
            indicators
                .next_if(|key| key.start.index() <= span.start.index())
                .filter(|key| key.slice(buffer) != Some("?"))
                .map(|key| {
                    keys.push(SimpleKey {
                        start: key.start,
                        end: 0,
                        parent,
                        characters: 0,
                    });
                    keys.len() - 1
                })
        } else {
            None
        };
        match event {
            Event::Scalar(_, style, _, _) => {
                if let Some(index) = own {
                    keys[index].end =
                        super::scalar_source_bounds(buffer, style, &span).1.get();
                }
                walker.skip_node();
            }
            Event::Alias(_) => {
                if let Some(index) = own {
                    keys[index].end = super::marker_byte_offset(span.end).get();
                }
                walker.skip_node();
            }
            Event::MappingStart(style, _, _) => {
                containers.push((own, parent));
                walker.enter_mapping((), style);
            }
            Event::SequenceStart(style, _, _) => {
                containers.push((own, parent));
                walker.enter_sequence(style);
            }
            Event::MappingEnd | Event::SequenceEnd => {
                if let Some((Some(index), _)) = containers.pop() {
                    keys[index].end = super::marker_byte_offset(span.end).get();
                }
                walker.exit_container();
            }
            Event::DocumentStart(..) => walker.reset(),
            _ => {}
        }
    }
    for key in &mut keys {
        key.characters = buffer[super::marker_byte_offset(key.start).get()..key.end]
            .chars()
            .count();
    }
    keys
}

fn enclosing_keys(keys: &[SimpleKey], at: usize) -> impl Iterator<Item = usize> + '_ {
    let before =
        keys.partition_point(|key| super::marker_byte_offset(key.start).get() <= at);
    std::iter::successors(before.checked_sub(1), |&index| keys[index].parent)
        .filter(move |&index| at < keys[index].end)
}

fn reserve_edit(
    keys: &mut [SimpleKey],
    buffer: &str,
    start: super::BytePos,
    end: super::BytePos,
    text: &str,
) {
    let before = buffer[start.get()..end.get()].chars().count();
    let after = text.chars().count();
    let enclosing: Vec<_> = enclosing_keys(keys, start.get()).collect();
    for index in enclosing {
        keys[index].characters = keys[index].characters + after - before;
    }
}

pub(super) fn key_spacing(
    input: crate::text_version::Analysis<'_>,
    cfg: &super::Config,
) -> Vec<crate::text_version::edits::Edit> {
    use crate::rules::{braces, colons, commas};
    let Some(policy) = cfg.requirements.as_ref().filter(|policy| policy.check_keys)
    else {
        return Vec::new();
    };
    let rules = [
        (
            colons::ID,
            policy
                .key_spacing
                .colons
                .as_ref()
                .map_or_else(Vec::new, |cfg| colons::edits(input, cfg)),
        ),
        (
            commas::ID,
            policy
                .key_spacing
                .commas
                .as_ref()
                .map_or_else(Vec::new, |cfg| commas::edits(input, cfg)),
        ),
        (
            braces::ID,
            policy
                .key_spacing
                .braces
                .as_ref()
                .map_or_else(Vec::new, |cfg| braces::edits(input, cfg)),
        ),
    ];
    let directives =
        crate::directives::Directives::scoped(input, crate::directives::Scope::Format);
    rules
        .into_iter()
        .flat_map(|(rule, edits)| {
            edits
                .into_iter()
                .filter(|(start, _, _)| {
                    let line = input
                        .byte_line_starts()
                        .partition_point(|&at| at <= start.get());
                    !directives.is_disabled(rule, line)
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

pub(super) fn constrain_keys(
    input: crate::text_version::Analysis<'_>,
    cfg: &super::Config,
    replacements: &mut Vec<super::Replacement>,
    spacing: &[crate::text_version::edits::Edit],
) -> Vec<super::Violation> {
    if !cfg
        .requirements
        .as_ref()
        .is_some_and(|policy| policy.check_keys)
    {
        return Vec::new();
    }
    let mut keys = simple_keys(input);
    for (start, end, text) in spacing {
        reserve_edit(&mut keys, input.text(), *start, *end, text);
    }
    let directives =
        crate::directives::Directives::scoped(input, crate::directives::Scope::Format);
    for (start, end, spelling) in replacements.iter() {
        let line = input
            .byte_line_starts()
            .partition_point(|&at| at <= start.get());
        if !directives.is_disabled(super::ID, line) {
            reserve_edit(&mut keys, input.text(), *start, *end, spelling.as_str());
        }
    }
    replacements.retain(|(start, _, _)| {
        !enclosing_keys(&keys, start.get()).any(|index| keys[index].characters > 1024)
    });
    keys.into_iter().filter(|key| key.characters > 1024).map(|key| super::Violation {
        line: key.start.line(), column: key.start.col() + 1,
        message: "cannot add required quotes: the implicit key would exceed 1024 characters; shorten the key or use an explicit `?` key".to_owned(),
    }).collect()
}
