//! Representation-level oracle for the format suite: what a document *is*, as opposed to
//! how it is laid out.
//!
//! Loading to plain values loses tags, aliases and duplicate keys, so [`representation`]
//! keeps granit's event stream instead, with each scalar resolved against the version its
//! document declares (or under YAML 1.1 throughout, for [`yaml_1_1_representation`]) and
//! every layout-only detail dropped (scalar style, the `---` marker, collection style).
//! [`annotations`] separately records each comment, keyed to the data event it sits
//! beside, and the anchor and alias names in source order.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::sync::LazyLock;

use granit_parser::{
    Event, Parser, Placement, ScalarStyle, Scanner, Span, SpannedEventReceiver,
    StrInput, StructureStyle, Tag, TokenType,
};
use regex::Regex;
use ryl::yaml_dom::{Scalar, ScalarOwned, is_core_schema};

use crate::config::block_scalar_eof;

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
    /// line moves a collection's.
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
    /// granit numbers anchors internally; ordinals of first definition are what the
    /// alias graph means.
    anchor_ordinals: BTreeMap<usize, usize>,
    yaml_1_1: bool,
    assume_yaml_1_1: bool,
}

impl Recorder {
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
                let trailed = self.starts.last().filter(|_| inline);
                let after_events = trailed.map_or(self.counted, |&(last, ..)| {
                    let on_line = || {
                        self.starts
                            .iter()
                            .filter(|(start, ..)| *start == last.min(line))
                    };
                    on_line()
                        .find(|(.., block)| !block)
                        .or_else(|| on_line().next())
                        .or(trailed)
                        .map(|&(_, before, _)| before)
                        .expect("the trailed node is among the starts")
                });
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

fn record(content: &str, assume_yaml_1_1: bool) -> Option<Recorder> {
    let mut recorder = Recorder {
        assume_yaml_1_1,
        ..Recorder::default()
    };
    Parser::new_from_str(block_scalar_eof::without_indentation(content))
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

pub fn representation(content: &str) -> Option<Vec<Node>> {
    record(content, false).map(|recorder| recorder.nodes)
}

/// [`representation`] as a YAML 1.1 reader such as PyYAML sees it, whatever the
/// document's `%YAML` directive.
pub fn yaml_1_1_representation(content: &str) -> Option<Vec<Node>> {
    record(content, true).map(|recorder| recorder.nodes)
}

pub fn annotations(content: &str) -> Option<Annotations> {
    let recorder = record(content, false)?;
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
