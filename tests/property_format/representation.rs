//! Representation-level oracle for the format suite: what a document *is*, as opposed to
//! how it is laid out.
//!
//! Loading to plain values loses tags, aliases and duplicate keys, so [`representation`]
//! keeps granit's event stream, correcting ignored spaces-only EOF block lines. It resolves
//! each scalar against the version its document declares (or under YAML 1.1 throughout,
//! for [`yaml_1_1_representation`]) and drops layout-only details (scalar style, the
//! `---` marker, collection style).
//! [`annotations`] separately records each comment, keyed to the data event it sits
//! beside, and the anchor and alias names in source order.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use granit_parser::{
    Event, Parser, Placement, ScalarStyle, Scanner, Span, SpannedEventReceiver,
    StrInput, StructureStyle, Tag, TokenType,
};
use regex::Regex;
use ryl::yaml_dom::{Scalar, ScalarOwned, is_core_schema};

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
    /// granit numbers anchors internally; ordinals of first definition are what the
    /// alias graph means.
    anchor_ordinals: BTreeMap<usize, usize>,
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
            Event::Scalar(value, style, anchor, tag) => Node::Scalar {
                anchor: self.anchor(anchor),
                tag: tag_text(tag.as_ref()),
                value: self.resolve(value, style, tag.as_ref()),
            },
            Event::Alias(id) => Node::Alias(
                self.anchor_ordinals
                    .get(&id)
                    .copied()
                    .expect("granit rejects an alias to an undefined anchor"),
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

pub(super) fn scalar_tokens(output: &str) -> Vec<(granit_parser::Span, ScalarStyle)> {
    Scanner::new(StrInput::new(output))
        .map_while(Result::ok)
        .filter_map(|token| match token.into_parts() {
            (span, TokenType::Scalar(style, _)) => Some((span, style)),
            _ => None,
        })
        .collect()
}

pub(super) fn same_implicit_block_value(before: &str, after: &str) -> bool {
    let tokens = scalar_tokens(before);
    let Some((span, style)) = tokens.last() else {
        return false;
    };
    if !matches!(style, ScalarStyle::Literal | ScalarStyle::Folded)
        || span.end.byte_offset() != Some(before.len())
    {
        return false;
    }
    let first = first_content_line(before, span);
    let Some(header) = first
        .checked_sub(2)
        .and_then(|index| before.split_inclusive('\n').nth(index))
        .and_then(|line| BLOCK_HEADER.captures(line.trim_end_matches(['\r', '\n'])))
    else {
        return false;
    };
    if header[1].bytes().any(|byte| byte.is_ascii_digit()) {
        return false;
    }
    let index = tokens.len() - 1;
    let value = scalar_value(before, index);
    value.is_some() && value == scalar_value(after, index)
}

pub(super) fn first_content_line(output: &str, span: &granit_parser::Span) -> usize {
    let lines: Vec<_> = output.lines().collect();
    let mut first = span.start.line();
    if span.indent.is_none()
        && lines
            .get(first - 1)
            .and_then(|line| line.chars().nth(span.start.col()))
            .is_some_and(|ch| matches!(ch, '|' | '>'))
    {
        return first + 1;
    }
    while first > 1 && lines[first - 2].trim_matches([' ', '\t']).is_empty() {
        first -= 1;
    }
    first
}

pub(super) fn scalar_value(output: &str, index: usize) -> Option<noyalib::Value> {
    let tokens = scalar_tokens(output);
    let (span, style) = tokens.get(index)?;
    if !matches!(style, ScalarStyle::Literal | ScalarStyle::Folded) {
        return noyalib::from_str(span.slice(output)?).ok();
    }
    let first = first_content_line(output, span);
    let lines: Vec<_> = output.split_inclusive('\n').collect();
    let header = *lines.get(first.checked_sub(2)?)?;
    let captures = BLOCK_HEADER.captures(header.trim_end_matches(['\r', '\n']))?;
    let indicator = &captures[1];
    let explicit = indicator.chars().find_map(|ch| ch.to_digit(10));
    let parent = explicit.map_or(0, |indent| {
        span.indent
            .unwrap_or(indent as usize)
            .saturating_sub(indent as usize)
    });
    let begin: usize = lines[..first - 1].iter().map(|line| line.len()).sum();
    let body = output.get(begin..span.end.byte_offset()?)?;
    let mut fragment = format!("{}a: {indicator}\n{body}", " ".repeat(parent));
    // noyalib needs a terminal break to retain the final block line.
    if !body.is_empty() && !body.ends_with(['\r', '\n']) {
        fragment.push('\n');
    }
    let mut value: std::collections::BTreeMap<String, noyalib::Value> =
        noyalib::from_str(&fragment).ok()?;
    value.remove("a")
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
    let original = record(content, assume_yaml_1_1, false)?;
    let corrected = without_ignored_terminal_spaces(content);
    if corrected.len() == content.len() {
        Some(original.nodes)
    } else {
        record(corrected, assume_yaml_1_1, false).map(|recorder| recorder.nodes)
    }
}

fn without_ignored_terminal_spaces(content: &str) -> &str {
    let Some((_, spaces)) = content.rsplit_once('\n') else {
        return content;
    };
    if spaces.is_empty() || !spaces.bytes().all(|byte| byte == b' ') {
        return content;
    }
    let shortened = &content[..content.len() - spaces.len()];
    // granit includes an ignored EOF line; noyalib verifies the reference value.
    if same_implicit_block_value(content, shortened) {
        shortened
    } else {
        content
    }
}

pub fn annotations(content: &str) -> Option<Annotations> {
    let recorder = record(content, false, true)?;
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
