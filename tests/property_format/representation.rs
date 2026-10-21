//! Representation-level oracle for the format suite: what a document *is*, as opposed to
//! how it is laid out.
//!
//! Loading to plain values loses tags, aliases and duplicate keys, so
//! [`representation`] keeps the parser's event stream, correcting block scalar values
//! where the parser differs from the reference parser. It resolves each scalar against
//! the version its document declares (or under YAML 1.1 throughout, for
//! [`yaml_1_1_representation`]) and drops layout-only details (scalar style, the
//! `---` marker, collection style).
//! [`annotations`] separately records each comment, keyed to the data event it sits
//! beside, and the anchor and alias names in source order.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use regex::Regex;
use ryl::yaml_dom::{Scalar, ScalarOwned, is_core_schema};
use ryl_yaml_parser::{
    Event, Parser, Placement, ScalarStyle, Scanner, Span, SpannedEventReceiver,
    StrInput, StructureStyle, Tag, TokenType,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Core(ScalarOwned),
    /// A core tag the content does not satisfy (`!!int abc`); kept as written.
    Unresolvable(String),
    /// Compared by spelling where the oracle has no resolver of its own: a YAML 1.1
    /// non-string or core-tagged scalar, or a 1.2 integer beyond `i64`. Spelling equality
    /// is stricter than value equality, which only flags more rewrites.
    Spelling(String),
    /// Content under a local or non-core tag, which the application resolves.
    Application(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    DocumentStart,
    DocumentEnd,
    SequenceStart {
        anchor: Option<usize>,
        tag: Option<String>,
    },
    SequenceEnd,
    MappingStart {
        anchor: Option<usize>,
        tag: Option<String>,
    },
    MappingEnd,
    Scalar {
        anchor: Option<usize>,
        tag: Option<String>,
        value: Value,
    },
    Alias(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    /// Data events before it, so a comment that moves to another node differs. A
    /// `DocumentStart` is not counted: `document-start` may insert `---` above a leading
    /// comment, which stays at the head of its document. Collection ends are not counted
    /// either, since block ones come after a comment their flow form precedes. An inline
    /// comment counts only those before the first node on the line of the node it trails,
    /// so a flow collection turned block may keep its trailing comment on its key's line;
    /// a block node's start counts as first only alone, since joining or breaking a `-`
    /// line moves a collection's. A comment immediately after a sequence dash counts
    /// before that sequence's start, regardless of when the parser emits the start.
    pub after_events: usize,
    pub inline: bool,
    /// Payload with whitespace trimmed around it and after its leading `#` run: the
    /// `comments` rule may add the space after `###`, and trailing whitespace is not content.
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Annotations {
    pub comments: Vec<Comment>,
    pub anchor_names: Vec<String>,
    pub alias_names: Vec<String>,
}

#[derive(Default)]
struct Recorder {
    nodes: Vec<Node>,
    counted: usize,
    /// The line each counted node but a document end starts on, `counted` before it, and
    /// whether it is a block node's start.
    starts: Vec<(usize, usize, bool)>,
    comments: Vec<Comment>,
    dash_comments: BTreeSet<(usize, usize)>,
    /// the parser numbers anchors internally; ordinals of first definition are what the
    /// alias graph means.
    anchor_ordinals: BTreeMap<usize, usize>,
    terminal_block_values: BTreeMap<usize, (String, Span, bool, bool)>,
    yaml_1_1: bool,
    assume_yaml_1_1: bool,
}

impl Recorder {
    fn inline_after_events(&self, line: usize) -> usize {
        let Some(&(last, ..)) = self.starts.last() else {
            return self.counted;
        };
        let on_line = || {
            self.starts
                .iter()
                .filter(|(start, ..)| *start == last.min(line))
        };
        on_line()
            .find(|(.., block)| !block)
            .or_else(|| on_line().next())
            .or_else(|| self.starts.last())
            .map(|&(_, before, _)| before)
            .expect("the trailed node is among the starts")
    }

    fn anchor(&mut self, id: usize) -> Option<usize> {
        (id > 0).then(|| {
            let next = self.anchor_ordinals.len();
            *self.anchor_ordinals.entry(id).or_insert(next)
        })
    }

    fn resolve(
        &self,
        value: Cow<'_, str>,
        style: ScalarStyle,
        tag: Option<&Cow<'_, Tag>>,
    ) -> Value {
        match tag {
            Some(tag) if !is_core_schema(tag) => Value::Application(value.into_owned()),
            Some(_) if self.yaml_1_1 => Value::Spelling(value.into_owned()),
            Some(_) => Scalar::resolve_scalar(value.clone(), style, tag).map_or_else(
                || Value::Unresolvable(value.into_owned()),
                |scalar| Value::Core(scalar.into_owned()),
            ),
            None if self.yaml_1_1
                && style == ScalarStyle::Plain
                && YAML_1_1_NONSTRING.is_match(&value) =>
            {
                Value::Spelling(value.into_owned())
            }
            None if self.yaml_1_1 && style == ScalarStyle::Plain => {
                Value::Core(ScalarOwned::String(value.into_owned()))
            }
            None if style == ScalarStyle::Plain && YAML_1_2_INT.is_match(&value) => {
                match Scalar::resolve_scalar(value.clone(), style, None) {
                    Some(Scalar::Integer(int)) => {
                        Value::Core(ScalarOwned::Integer(int))
                    }
                    _ => Value::Spelling(value.into_owned()),
                }
            }
            None => Value::Core(
                Scalar::resolve_scalar(value, style, None)
                    .expect("an untagged scalar always resolves")
                    .into_owned(),
            ),
        }
    }
}

fn comment_text(text: &str) -> String {
    let body = text.trim_start_matches('#');
    format!("{}{}", &text[..text.len() - body.len()], body.trim_start())
}

fn tag_text(tag: Option<&Cow<'_, Tag>>) -> Option<String> {
    tag.map(|tag| tag.to_string())
}

impl<'input> SpannedEventReceiver<'input> for Recorder {
    fn on_event(&mut self, event: Event<'input>, span: Span) {
        let line = span.start.line();
        let block = matches!(
            event,
            Event::SequenceStart(StructureStyle::Block, ..)
                | Event::MappingStart(StructureStyle::Block, ..)
                | Event::Scalar(_, ScalarStyle::Literal | ScalarStyle::Folded, ..)
        );
        let node = match event {
            Event::Comment(text, placement) => {
                let inline = placement == Placement::Right;
                let after_events = if self
                    .dash_comments
                    .contains(&(line, span.start.col()))
                {
                    if matches!(self.nodes.last(), Some(Node::SequenceStart { .. }))
                        && self.starts.last().is_some_and(|&(start, ..)| start == line)
                    {
                        self.counted - 1
                    } else {
                        self.counted
                    }
                } else {
                    if inline {
                        self.inline_after_events(line)
                    } else {
                        self.counted
                    }
                };
                self.comments.push(Comment {
                    after_events,
                    inline,
                    text: comment_text(text.trim()),
                });
                return;
            }
            Event::DocumentStart(_, version) => {
                self.yaml_1_1 = self.assume_yaml_1_1
                    || version.is_some_and(|version| {
                        (version.major, version.minor) <= (1, 1)
                    });
                Node::DocumentStart
            }
            Event::DocumentEnd => Node::DocumentEnd,
            Event::SequenceStart(_, anchor, tag) => Node::SequenceStart {
                anchor: self.anchor(anchor),
                tag: tag_text(tag.as_ref()),
            },
            Event::SequenceEnd => Node::SequenceEnd,
            Event::MappingStart(_, anchor, tag) => Node::MappingStart {
                anchor: self.anchor(anchor),
                tag: tag_text(tag.as_ref()),
            },
            Event::MappingEnd => Node::MappingEnd,
            Event::Scalar(value, style, anchor, tag) => {
                let corrected =
                    matches!(style, ScalarStyle::Literal | ScalarStyle::Folded)
                        .then(|| span.end.byte_offset())
                        .flatten()
                        .and_then(|end| self.terminal_block_values.remove(&end))
                        .and_then(|(content, span, followed, marker_followed)| {
                            reference_terminal_block_value(
                                &content,
                                &span,
                                style,
                                &value,
                                followed,
                                marker_followed,
                            )
                        });
                let value = corrected.map_or(value, Cow::Owned);
                Node::Scalar {
                    anchor: self.anchor(anchor),
                    tag: tag_text(tag.as_ref()),
                    value: self.resolve(value, style, tag.as_ref()),
                }
            }
            Event::Alias(id) => Node::Alias(
                self.anchor_ordinals
                    .get(&id)
                    .copied()
                    .expect("the parser rejects an alias to an undefined anchor"),
            ),
            _ => return,
        };
        if !matches!(
            node,
            Node::DocumentStart | Node::SequenceEnd | Node::MappingEnd
        ) {
            if node != Node::DocumentEnd {
                self.starts.push((line, self.counted, block));
            }
            self.counted += 1;
        }
        self.nodes.push(node);
    }
}

fn record(
    content: &str,
    assume_yaml_1_1: bool,
    with_comments: bool,
    terminal_block_values: BTreeMap<usize, (String, Span, bool, bool)>,
) -> Option<Recorder> {
    let mut dash_comments = BTreeSet::new();
    let mut dash_line = None;
    if with_comments {
        for token in Scanner::new(StrInput::new(content)) {
            let (span, kind) = token.ok()?.into_parts();
            match kind {
                TokenType::BlockEntry => dash_line = Some(span.start.line()),
                TokenType::Comment(_) if dash_line == Some(span.start.line()) => {
                    dash_comments.insert((span.start.line(), span.start.col()));
                    dash_line = None;
                }
                _ => dash_line = None,
            }
        }
    }
    let mut recorder = Recorder {
        assume_yaml_1_1,
        dash_comments,
        terminal_block_values,
        ..Recorder::default()
    };
    Parser::new_from_str(content)
        .load(&mut recorder, true)
        .ok()?;
    Some(recorder)
}

pub fn check_values(input: &str, output: &str) -> Result<(), String> {
    let before = representation(input);
    let after = representation(output);
    if before.is_some() != after.is_some() {
        return Err(format!("parse-preservation: output {output:?}"));
    }
    if before != after {
        return Err(format!(
            "value-preservation: output {output:?}; before {before:?}; after {after:?}"
        ));
    }
    Ok(())
}

pub fn check_annotations(input: &str, output: &str) -> Result<(), String> {
    let (before, after) = (annotations(input), annotations(output));
    if before != after {
        return Err(format!(
            "comment/anchor fidelity: output {output:?}; before {before:?}; after {after:?}"
        ));
    }
    Ok(())
}

pub fn check_yaml_1_1_preserved(input: &str, output: &str) -> Result<(), String> {
    let (before, after) = (
        yaml_1_1_representation(input),
        yaml_1_1_representation(output),
    );
    if before != after {
        return Err(format!(
            "yaml-1.1 value-preservation: output {output:?}; before {before:?}; after {after:?}"
        ));
    }
    Ok(())
}

pub(super) static BLOCK_HEADER: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| {
        regex::Regex::new(r"([|>][1-9+-]{0,2})(?:[ \t]*(?:#.*)?)$").unwrap()
    });

pub(super) fn scalar_tokens(output: &str) -> Vec<(ryl_yaml_parser::Span, ScalarStyle)> {
    Scanner::new(StrInput::new(output))
        .map_while(Result::ok)
        .filter_map(|token| match token.into_parts() {
            (span, TokenType::Scalar(style, _)) => Some((span, style)),
            _ => None,
        })
        .collect()
}

pub(super) fn first_content_line(output: &str, span: &ryl_yaml_parser::Span) -> usize {
    let lines = yaml_lines(output);
    let mut first = span.start.line();
    if span.indent.is_none()
        && lines
            .get(first - 1)
            .and_then(|line| line.chars().nth(span.start.col()))
            .is_some_and(|ch| matches!(ch, '|' | '>'))
    {
        return first + 1;
    }
    while first > 1
        && lines[first - 2]
            .trim_matches([' ', '\t', '\r', '\n'])
            .is_empty()
    {
        first -= 1;
    }
    first
}

pub(super) fn yaml_lines(content: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut remaining = content;
    while let Some(index) = remaining.find(['\r', '\n']) {
        let end = index
            + if remaining[index..].starts_with("\r\n") {
                2
            } else {
                1
            };
        let (line, rest) = remaining.split_at(end);
        lines.push(line);
        remaining = rest;
    }
    if !remaining.is_empty() {
        lines.push(remaining);
    }
    lines
}

pub fn representation(content: &str) -> Option<Vec<Node>> {
    value_representation(content, false)
}

/// [`representation`] as a YAML 1.1 reader such as PyYAML sees it, whatever the
/// document's `%YAML` directive.
pub fn yaml_1_1_representation(content: &str) -> Option<Vec<Node>> {
    value_representation(content, true)
}

fn value_representation(content: &str, assume_yaml_1_1: bool) -> Option<Vec<Node>> {
    let mut values = BTreeMap::new();
    for (span, style) in scalar_tokens(content) {
        if !matches!(style, ScalarStyle::Literal | ScalarStyle::Folded) {
            continue;
        }
        let Some((end, prefix)) = span
            .end
            .byte_offset()
            .and_then(|end| content.get(..end).map(|prefix| (end, prefix)))
        else {
            continue;
        };
        let tail = content[end..].trim_start_matches([' ', '\r', '\n']);
        let followed = !tail.is_empty();
        let first = first_content_line(content, &span);
        let lines = yaml_lines(prefix);
        let blank_only = first
            .checked_sub(1)
            .and_then(|index| lines.get(index..))
            .is_some_and(|body| body.iter().all(|line| line.trim().is_empty()));
        if followed
            && !tail.starts_with("...")
            && !tail.starts_with("---")
            && !blank_only
        {
            continue;
        }
        let marker_followed = tail.starts_with("...") || tail.starts_with("---");
        values.insert(end, (prefix.to_owned(), span, followed, marker_followed));
    }
    record(content, assume_yaml_1_1, false, values).map(|recorder| recorder.nodes)
}

pub(super) fn reference_terminal_block_value(
    content: &str,
    span: &ryl_yaml_parser::Span,
    style: ScalarStyle,
    loaded: &str,
    followed: bool,
    marker_followed: bool,
) -> Option<String> {
    if !matches!(style, ScalarStyle::Literal | ScalarStyle::Folded)
        || span.end.byte_offset() != Some(content.len())
    {
        return None;
    }
    let lines = yaml_lines(content);
    let first = first_content_line(content, span);
    let (header_line, body) = first
        .checked_sub(2)
        .and_then(|index| lines.get(index).zip(lines.get(index + 1..)))?;
    let body = if followed
        && body
            .last()
            .is_some_and(|line| !line.ends_with(['\r', '\n']) && line.trim().is_empty())
    {
        &body[..body.len() - 1]
    } else {
        body
    };
    let header = BLOCK_HEADER.captures(header_line.trim_end_matches(['\r', '\n']))?;
    let indicator = &header[1];
    let (indent, root_explicit) = if let Some(explicit) =
        indicator.bytes().find(u8::is_ascii_digit)
    {
        let marker = header.get(1)?;
        let prefix = &header_line[..marker.start()];
        let leading = prefix.bytes().take_while(|byte| *byte == b' ').count();
        let mut parent = leading;
        let mut rest = &prefix[leading..];
        let mut depth = 0usize;
        let root = Parser::new_from_str(content).map_while(Result::ok).any(
            |(event, event_span)| {
                match event {
                    Event::MappingStart(..) | Event::SequenceStart(..) => depth += 1,
                    Event::MappingEnd | Event::SequenceEnd => depth -= 1,
                    Event::Scalar(
                        _,
                        ScalarStyle::Literal | ScalarStyle::Folded,
                        ..,
                    ) => {
                        return depth == 0
                            && event_span.start.index() == span.start.index()
                            && event_span.end.index() == span.end.index();
                    }
                    _ => {}
                }
                false
            },
        );
        if root {
            // A root scalar's indentation parameter starts at -1.
            parent += usize::from(explicit - b'0') - 1;
        } else {
            while let Some(tail) = rest.strip_prefix('-') {
                let blanks = tail.bytes().take_while(|byte| *byte == b' ').count();
                if blanks == 0 {
                    break;
                }
                rest = &tail[blanks..];
                if !rest.trim().is_empty() {
                    parent += blanks + 1;
                }
            }
            parent += usize::from(explicit - b'0');
        }
        if root {
            (parent, true)
        } else if body
            .iter()
            .all(|line| line.trim_matches([' ', '\r', '\n']).is_empty())
            && let Some(colon) = prefix.find(':')
            && prefix[..colon].trim().is_empty()
            && prefix[colon + 1..].contains(':')
        {
            let key = colon
                + 1
                + prefix[colon + 1..]
                    .bytes()
                    .take_while(|byte| *byte == b' ')
                    .count();
            (key + usize::from(explicit - b'0'), false)
        } else if body
            .iter()
            .all(|line| line.trim_matches([' ', '\r', '\n']).is_empty())
            && let Some(dash) = prefix.rfind("- ")
            && prefix.rfind(':') < Some(dash)
        {
            (dash + usize::from(explicit - b'0'), false)
        } else {
            (span.indent.unwrap_or(parent), false)
        }
    } else if followed
        && body
            .iter()
            .all(|line| line.trim_matches([' ', '\r', '\n']).is_empty())
    {
        let marker = header.get(1)?;
        let prefix = &header_line[..marker.start()];
        let leading = prefix.bytes().take_while(|byte| *byte == b' ').count();
        let compact_mapping = prefix.rfind(':') > prefix.rfind('-')
            && prefix.split_whitespace().any(|token| token == "-");
        (
            if marker_followed && compact_mapping {
                0
            } else {
                leading + 1
            },
            false,
        )
    } else {
        (
            body.iter()
                .find(|line| !line.trim_matches([' ', '\r', '\n']).is_empty())
                .map(|line| line.bytes().take_while(|byte| *byte == b' ').count())
                .unwrap_or_else(|| {
                    body.iter()
                        .map(|line| {
                            line.bytes().take_while(|byte| *byte == b' ').count()
                        })
                        .max()
                        .unwrap_or(0)
                }),
            false,
        )
    };
    let chunks: Vec<_> = body
        .iter()
        .map(|line| {
            let raw = line.trim_end_matches(['\r', '\n']);
            (
                raw.get(indent..).unwrap_or_default(),
                raw.len() != line.len() || !raw.is_empty(),
            )
        })
        .collect();
    let loaded = if root_explicit {
        // the parser counts root explicit indentation one level deeper than the
        // reference.
        let mut adjusted = lines[..first - 1].concat();
        for line in body {
            adjusted.push(' ');
            adjusted.push_str(line);
        }
        Cow::Owned(
            Scanner::new(StrInput::new(&adjusted))
                .map_while(Result::ok)
                .find_map(|token| match token.into_parts().1 {
                    TokenType::Scalar(
                        ScalarStyle::Literal | ScalarStyle::Folded,
                        value,
                    ) => Some(value.into_owned()),
                    _ => None,
                })?,
        )
    } else {
        Cow::Borrowed(loaded)
    };
    let mut value = if body
        .iter()
        .all(|line| line.trim_matches([' ', '\r', '\n']).is_empty())
    {
        let mut tail = String::new();
        for &(text, has_break) in &chunks {
            tail.push_str(text);
            if has_break {
                tail.push('\n');
            }
        }
        tail.trim_end_matches('\n').to_owned()
    } else {
        loaded.trim_end_matches('\n').to_owned()
    };
    let last_content = chunks.iter().rposition(|(text, _)| !text.is_empty());
    let trailing_blank =
        last_content.map_or(chunks.len(), |index| chunks.len() - index - 1);
    let content_chunks = &chunks[..last_content.map_or(0, |index| index + 1)];
    let whitespace_start = content_chunks
        .iter()
        .rposition(|(text, _)| !text.trim().is_empty())
        .map_or(0, |index| index + 1);
    let blank_count = content_chunks[whitespace_start..]
        .iter()
        .take_while(|(text, _)| text.is_empty())
        .count();
    if style == ScalarStyle::Folded
        && blank_count >= 2
        && whitespace_start
            .checked_sub(1)
            .is_some_and(|index| !content_chunks[index].0.starts_with([' ', '\t']))
    {
        let tail_len = content_chunks[whitespace_start + blank_count..]
            .iter()
            .map(|(text, has_break)| text.len() + usize::from(*has_break))
            .sum::<usize>()
            - usize::from(content_chunks.last()?.1);
        let at = value.len().checked_sub(tail_len)?;
        value.insert_str(at, &"\n".repeat(blank_count - 1));
    }
    // The reference parser counts keep-chomped blank tails differently by style.
    let breaks = if indicator.contains('-') {
        0
    } else if indicator.contains('+') {
        match last_content {
            None if style == ScalarStyle::Folded => {
                if trailing_blank == 0 {
                    1
                } else {
                    2 * trailing_blank
                }
            }
            None => match trailing_blank {
                0 => 0,
                1 => 1,
                count => 2 * count - 2,
            },
            Some(_) if trailing_blank == 0 => 1,
            Some(index) => {
                trailing_blank * if style == ScalarStyle::Folded { 3 } else { 2 }
                    + usize::from(
                        style == ScalarStyle::Folded
                            && chunks[index].0.starts_with([' ', '\t']),
                    )
            }
        }
    } else {
        usize::from(
            last_content.is_some()
                || (style == ScalarStyle::Literal && trailing_blank >= 2),
        )
    };
    value.extend(std::iter::repeat_n('\n', breaks));
    Some(value)
}

pub fn annotations(content: &str) -> Option<Annotations> {
    let recorder = record(content, false, true, BTreeMap::new())?;
    let mut anchor_names = Vec::new();
    let mut alias_names = Vec::new();
    for token in Scanner::new(StrInput::new(content)) {
        match token.ok()?.into_parts().1 {
            TokenType::Anchor(name) => anchor_names.push(name.into_owned()),
            TokenType::Alias(name) => alias_names.push(name.into_owned()),
            _ => {}
        }
    }
    Some(Annotations {
        comments: recorder.comments,
        anchor_names,
        alias_names,
    })
}

// Written from PyYAML's 1.1 resolver plus the `0o` octal yamllint adds, rather than
// shared with `quoted-strings`, so a gap in the fixer's own 1.1 table cannot hide here
// too.
static YAML_1_1_NONSTRING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"\A(?:",
        r"yes|Yes|YES|no|No|NO|true|True|TRUE|false|False|FALSE|on|On|ON|off|Off|OFF",
        r"|[-+]?0b[01_]+|[-+]?0o?[0-7_]+|[-+]?(?:0|[1-9][0-9_]*)|[-+]?0x[0-9a-fA-F_]+",
        r"|[-+]?[1-9][0-9_]*(?::[0-5]?[0-9])+",
        r"|[-+]?[0-9][0-9_]*\.[0-9_]*(?:[eE][-+][0-9]+)?|\.[0-9][0-9_]*(?:[eE][-+][0-9]+)?",
        r"|[-+]?[0-9][0-9_]*(?::[0-5]?[0-9])+\.[0-9_]*|[-+]?\.(?:inf|Inf|INF)|\.(?:nan|NaN|NAN)",
        r"|~|null|Null|NULL|",
        r"|[0-9]{4}-[0-9]{2}-[0-9]{2}",
        r"|[0-9]{4}-[0-9]{1,2}-[0-9]{1,2}(?:[Tt]|[ \t]+)[0-9]{1,2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]*)?(?:[ \t]*(?:Z|[-+][0-9]{1,2}(?::[0-9]{2})?))?",
        r"|<<|=",
        r")\z",
    ))
    .expect("YAML 1.1 implicit-type regex is valid")
});

static YAML_1_2_INT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\A(?:[-+]?[0-9]+|0o[0-7]+|0x[0-9a-fA-F]+)\z")
        .expect("YAML 1.2 core-schema int regex is valid")
});
