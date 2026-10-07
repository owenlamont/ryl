//! `ryl format`'s `[format] sequence-style` and `mapping-style`: flow collections
//! rewritten in block style and leaf block ones in flow style, each entry's text copied
//! as written.

use granit_parser::{Event, Parser, ScalarStyle, StructureStyle};

use crate::config_schema::CollectionStyleTarget;
use crate::directives::{Directives, disables_file};
use crate::lint::{LintProblem, Severity};
use crate::rules::support::line_syntax::buffer_newline;
use crate::rules::support::span_utils::{
    BytePos, apply_replacements, marker_byte_offset,
};
use crate::rules::{braces, brackets};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    pub sequences: CollectionStyleTarget,
    pub mappings: CollectionStyleTarget,
    pub indent: u8,
    pub width: u16,
}

/// A collection the pass rewrites, or leaves alone for `refused`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub line: usize,
    pub column: usize,
    pub rule: &'static str,
    pub refused: Option<&'static str>,
    pub to_flow: bool,
}

impl Finding {
    #[must_use]
    pub fn into_problem(self) -> LintProblem {
        let kind = if self.rule == braces::ID {
            "mapping"
        } else {
            "sequence"
        };
        let (from, to) = if self.to_flow {
            ("block", "flow")
        } else {
            ("flow", "block")
        };
        LintProblem {
            line: self.line,
            column: self.column,
            level: Severity::Error,
            message: self.refused.map_or_else(
                || format!("{from} {kind} would become {to}"),
                |reason| format!("cannot convert to block safely: {reason}"),
            ),
            rule: Some(self.rule),
        }
    }
}

/// `buffer` with every convertible collection restyled, and the rules whose collections
/// changed; `None` when nothing changes.
#[must_use]
pub fn restyle(buffer: &str, cfg: Config) -> Option<(String, Vec<&'static str>)> {
    let (edits, findings) = plan(buffer, cfg);
    let restyled = apply_replacements(buffer, edits);
    (restyled != buffer && same_data(buffer, &restyled)).then(|| {
        let rules = findings
            .into_iter()
            .filter(|finding| finding.refused.is_none())
            .map(|finding| finding.rule)
            .collect();
        (restyled, rules)
    })
}

#[must_use]
pub fn findings(buffer: &str, cfg: Config) -> Vec<Finding> {
    plan(buffer, cfg).1
}

type Edit = (BytePos, BytePos, String);

fn plan(buffer: &str, cfg: Config) -> (Vec<Edit>, Vec<Finding>) {
    let directives = Directives::parse(buffer);
    let style = |target, rule| {
        (!directives.disables_any(rule) && !disables_file(buffer)).then_some(target)
    };
    let restyler = Restyler {
        buffer,
        nodes: tree(buffer),
        comments: comments(buffer),
        sequences: style(cfg.sequences, brackets::ID),
        mappings: style(cfg.mappings, braces::ID),
        indent: usize::from(cfg.indent),
        width: usize::from(cfg.width),
        newline: buffer_newline(buffer),
    };
    let mut edits = Vec::new();
    let mut findings = Vec::new();
    for (index, node) in restyler.nodes.iter().enumerate() {
        let finding = |refused, to_flow| Finding {
            line: node.line,
            column: node.column + 1,
            rule: if node.kind == Kind::Mapping {
                braces::ID
            } else {
                brackets::ID
            },
            refused,
            to_flow,
        };
        if let Some(edit) = restyler.flowed(index) {
            findings.push(finding(None, true));
            edits.push(edit);
        }
        if !restyler.to_block(index)
            || node
                .parent
                .is_some_and(|parent| restyler.nodes[parent].flow)
        {
            continue;
        }
        let outcome = restyler.outermost(index);
        findings.push(finding(outcome.as_ref().err().copied(), false));
        edits.extend(outcome.ok());
    }
    (edits, findings)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Scalar(ScalarStyle),
    Alias,
    Sequence,
    Mapping,
}

#[derive(Debug)]
struct Node {
    kind: Kind,
    flow: bool,
    /// A flow collection written with its own `[` or `{`, unlike an implicit `a: 1` pair.
    braced: bool,
    start: usize,
    end: usize,
    line: usize,
    column: usize,
    parent: Option<usize>,
    children: Vec<usize>,
}

fn tree(buffer: &str) -> Vec<Node> {
    let mut nodes: Vec<Node> = Vec::new();
    let mut open: Vec<usize> = Vec::new();
    for (event, span) in Parser::new_from_str(buffer).map_while(Result::ok) {
        let (start, end) = (
            marker_byte_offset(span.start).get(),
            marker_byte_offset(span.end).get(),
        );
        let (kind, flow) = match event {
            Event::Scalar(_, style, ..) => (Kind::Scalar(style), false),
            Event::Alias(_) => (Kind::Alias, false),
            Event::SequenceStart(style, ..) => {
                (Kind::Sequence, style == StructureStyle::Flow)
            }
            Event::MappingStart(style, ..) => {
                (Kind::Mapping, style == StructureStyle::Flow)
            }
            Event::SequenceEnd | Event::MappingEnd => {
                let index = open.pop().expect("an end event closes an open collection");
                let last = nodes[index].children.last().map(|&child| nodes[child].end);
                let node = &mut nodes[index];
                node.end = if node.braced {
                    end
                } else {
                    last.unwrap_or(node.start)
                };
                continue;
            }
            _ => continue,
        };
        let index = nodes.len();
        let parent = open.last().copied();
        if let Some(parent) = parent {
            nodes[parent].children.push(index);
        }
        nodes.push(Node {
            kind,
            flow,
            braced: flow && end > start && buffer[start..].starts_with(['[', '{']),
            start,
            end,
            line: span.start.line(),
            column: span.start.col(),
            parent,
            children: Vec::new(),
        });
        if matches!(kind, Kind::Sequence | Kind::Mapping) {
            open.push(index);
        }
    }
    nodes
}

/// The byte span of each comment in `buffer`.
fn comments(buffer: &str) -> Vec<(usize, usize)> {
    Parser::new_from_str(buffer)
        .map_while(Result::ok)
        .filter(|(event, _)| matches!(event, Event::Comment(..)))
        .map(|(_, span)| {
            (
                marker_byte_offset(span.start).get(),
                marker_byte_offset(span.end).get(),
            )
        })
        .collect()
}

fn is_space(ch: char) -> bool {
    matches!(ch, ' ' | '\t' | '\n' | '\r')
}

/// Whether `text` has a `-`, `?` or `:` indicator followed by more than one space, which
/// `hyphens` and `colons` leave alone only while the entry after it spans lines.
fn widened_indicator(text: &str) -> bool {
    text.match_indices(['-', '?', ':']).any(|(at, _)| {
        text[at + 1..].starts_with("  ") && (at == 0 || text[..at].ends_with(' '))
    })
}

struct Restyler<'a> {
    buffer: &'a str,
    nodes: Vec<Node>,
    comments: Vec<(usize, usize)>,
    sequences: Option<CollectionStyleTarget>,
    mappings: Option<CollectionStyleTarget>,
    indent: usize,
    width: usize,
    newline: &'static str,
}

impl Restyler<'_> {
    fn target(&self, kind: Kind) -> Option<CollectionStyleTarget> {
        if kind == Kind::Sequence {
            self.sequences
        } else {
            self.mappings
        }
    }

    fn to_block(&self, index: usize) -> bool {
        let node = &self.nodes[index];
        node.braced
            && !node.children.is_empty()
            && self.target(node.kind) == Some(CollectionStyleTarget::Block)
    }

    fn is_key(&self, index: usize) -> bool {
        self.nodes[index].parent.is_some_and(|parent| {
            self.nodes[parent].kind == Kind::Mapping
                && self.nodes[parent]
                    .children
                    .iter()
                    .position(|&child| child == index)
                    .expect("a node is among its parent's children")
                    % 2
                    == 0
        })
    }

    fn has_comment(&self, from: usize, to: usize) -> bool {
        self.comments
            .iter()
            .any(|(start, _)| (from..to).contains(start))
    }

    /// The edit rewriting leaf block collection `index` in flow style, if it holds no
    /// comment, fits on its owner's line, and every entry reads the same in flow context.
    fn flowed(&self, index: usize) -> Option<Edit> {
        let node = &self.nodes[index];
        if node.flow
            || node.parent.is_none()
            || self.target(node.kind) != Some(CollectionStyleTarget::Flow)
            || self.is_key(index)
        {
            return None;
        }
        let last = self.nodes[*node.children.last()?].end;
        let line_end = self.buffer[last..]
            .find(['\n', '\r'])
            .map_or(self.buffer.len(), |at| last + at);
        let start = self.buffer[..node.start].trim_end_matches(is_space).len();
        if self.has_comment(start, line_end)
            || self.comments.iter().any(|(_, end)| *end == start)
        {
            return None;
        }
        let mut pos = node.start;
        let mut entries = Vec::new();
        if node.kind == Kind::Sequence {
            for &child in &node.children {
                entries.push(self.flow_entry(child, pos)?.to_string());
                pos = self.nodes[child].end;
            }
        } else {
            for &[key, value] in node.children.as_chunks::<2>().0 {
                let key_text = self.flow_entry(key, pos)?;
                let colon = self.skip_space(self.nodes[key].end);
                let value_text = self.flow_entry(value, colon + 1)?;
                let separator = if self.nodes[key].kind == Kind::Alias {
                    " :"
                } else {
                    ":"
                };
                entries.push(format!("{key_text}{separator} {value_text}"));
                pos = self.nodes[value].end;
            }
        }
        let (open, close) = if node.kind == Kind::Sequence {
            ('[', ']')
        } else {
            ('{', '}')
        };
        let text = format!(" {open}{}{close}", entries.join(", "));
        let line_start = self.buffer[..start]
            .rfind(['\n', '\r'])
            .map_or(0, |at| at + 1);
        let width =
            self.buffer[line_start..start].chars().count() + text.chars().count();
        let owner = self.buffer[line_start..].split(['\n', '\r']).next();
        (width <= self.width && !owner.is_some_and(widened_indicator)).then_some((
            BytePos::new(start),
            BytePos::new(last),
            text,
        ))
    }

    /// Entry `index`'s text from byte `pos`, if it reads the same in flow context: an
    /// alias, a quoted scalar, or a one-line plain one with no flow indicator and no
    /// leading `:` or `?`.
    fn flow_entry(&self, index: usize, pos: usize) -> Option<&str> {
        let node = &self.nodes[index];
        let written = &self.buffer[node.start..node.end];
        let fits = match node.kind {
            Kind::Scalar(ScalarStyle::Plain) => {
                !written.is_empty()
                    && !written.contains([',', '[', ']', '{', '}'])
                    && !written.starts_with([':', '?'])
            }
            Kind::Scalar(ScalarStyle::SingleQuoted | ScalarStyle::DoubleQuoted)
            | Kind::Alias => true,
            _ => false,
        };
        fits.then(|| self.text(pos, node.end).ok()).flatten()
    }

    /// The edit rewriting collection `index`, whose parent is block or the document.
    fn outermost(&self, index: usize) -> Result<Edit, &'static str> {
        let node = &self.nodes[index];
        if self.is_key(index) {
            return Err("it is a mapping key");
        }
        let line_start = self.buffer[..node.start]
            .rfind(['\n', '\r'])
            .map_or(0, |at| at + 1);
        let prefix = &self.buffer[line_start..node.start];
        let line_end = self.buffer[node.end..]
            .find(['\n', '\r'])
            .map_or(self.buffer.len(), |at| node.end + at);
        let rest = self.buffer[node.end..line_end].trim_matches(is_space);
        let comment = rest.starts_with('#').then_some(rest);
        let keyed = node.parent.is_some_and(|parent| {
            let siblings = &self.nodes[parent].children;
            let at = siblings.iter().position(|&child| child == index);
            self.nodes[parent].kind == Kind::Mapping
                && at.is_some_and(|at| self.nodes[siblings[at - 1]].line == node.line)
        });
        let one_line = !self.buffer[node.start..node.end].contains(['\n', '\r']);
        if comment.is_some() && !(keyed && one_line) {
            return Err("a trailing comment has no key line to move to");
        }
        let end = if comment.is_some() {
            line_end
        } else {
            node.end
        };
        let compact = prefix
            .split(' ')
            .all(|token| token.is_empty() || token == "-");
        let dashes = prefix.trim_end_matches(' ');
        let outer = if compact {
            dashes.strip_suffix('-').unwrap_or(dashes)
        } else {
            prefix
        };
        if widened_indicator(outer) {
            return Err("an indicator before it has extra spaces");
        }
        if compact {
            let (start, col, gap) = if dashes.is_empty() {
                (node.start, prefix.len(), "")
            } else {
                (line_start + dashes.len(), dashes.len() + 1, " ")
            };
            let body = self.block(index, node.start + 1, col)?;
            return Ok((
                BytePos::new(start),
                BytePos::new(end),
                format!("{gap}{body}"),
            ));
        }
        let col = node
            .parent
            .map_or(0, |parent| self.nodes[parent].column + self.indent);
        let body = self.block(index, node.start + 1, col)?;
        let comment =
            comment.map_or_else(String::new, |comment| format!("  {comment}"));
        Ok((
            BytePos::new(line_start + prefix.trim_end_matches(is_space).len()),
            BytePos::new(end),
            format!("{comment}{}{body}", self.break_to(col)),
        ))
    }

    /// Collection `index` in block style, its entries starting at byte `pos` and its
    /// first line written at column `col`.
    fn block(
        &self,
        index: usize,
        mut pos: usize,
        col: usize,
    ) -> Result<String, &'static str> {
        let node = &self.nodes[index];
        if self.has_comment(node.start, node.end) {
            return Err("it holds a comment");
        }
        let mut lines = Vec::new();
        if node.kind == Kind::Sequence {
            for &child in &node.children {
                lines.push(format!("- {}", self.item(child, pos, col + 2)?));
                pos = self.after_entry(self.nodes[child].end);
            }
        } else {
            for pair in node.children.as_chunks::<2>().0 {
                let (key, value) = (&self.nodes[pair[0]], &self.nodes[pair[1]]);
                if matches!(key.kind, Kind::Sequence | Kind::Mapping) {
                    return Err("a key is a collection");
                }
                if key.start == key.end {
                    return Err("a key is empty");
                }
                let key_text = self.copy(pair[0], pos)?;
                if key_text.chars().count() > 1024 {
                    return Err("a key is longer than 1024 characters");
                }
                let colon = self.skip_space(key.end);
                let has_colon = self.buffer[colon..].starts_with(':');
                if key.kind == Kind::Scalar(ScalarStyle::Plain)
                    && has_colon
                    && !self.buffer[colon + 1..].starts_with(is_space)
                {
                    return Err("a plain key's `:` has no space after it");
                }
                let value_start = colon + usize::from(has_colon);
                let separator = if key.kind == Kind::Alias { " :" } else { ":" };
                let value_text = self.value(pair[1], value_start, col)?;
                lines.push(format!("{key_text}{separator}{value_text}"));
                pos = self.after_entry(value.end.max(value_start));
            }
        }
        Ok(lines.join(&self.break_to(col)))
    }

    /// A sequence entry's text after its `- `, written at column `col`.
    fn item(
        &self,
        index: usize,
        pos: usize,
        col: usize,
    ) -> Result<String, &'static str> {
        let node = &self.nodes[index];
        if node.flow && !node.braced {
            return self.block(index, pos, col);
        }
        if !self.to_block(index) {
            return self.copy(index, pos).map(str::to_string);
        }
        let properties = self.text(pos, node.start)?;
        let body = self.block(index, node.start + 1, col)?;
        Ok(if properties.is_empty() {
            body
        } else {
            format!("{properties}{}{body}", self.break_to(col))
        })
    }

    /// A mapping value's text after its key's `:`, the key written at column `col`.
    fn value(
        &self,
        index: usize,
        pos: usize,
        col: usize,
    ) -> Result<String, &'static str> {
        let node = &self.nodes[index];
        if !self.to_block(index) {
            let text = self.copy(index, pos)?;
            return Ok(if text.is_empty() {
                String::new()
            } else {
                format!(" {text}")
            });
        }
        let properties = self.text(pos, node.start)?;
        let inner = col + self.indent;
        let body = self.block(index, node.start + 1, inner)?;
        let properties = if properties.is_empty() {
            String::new()
        } else {
            format!(" {properties}")
        };
        Ok(format!("{properties}{}{body}", self.break_to(inner)))
    }

    /// Node `index`'s text from byte `pos`; `PyYAML` reads a plain `?x` in flow as a key.
    fn copy(&self, index: usize, pos: usize) -> Result<&str, &'static str> {
        let node = &self.nodes[index];
        if node.kind == Kind::Scalar(ScalarStyle::Plain)
            && self.buffer[node.start..].starts_with('?')
        {
            return Err("an entry starts with `?`");
        }
        self.text(pos, node.end)
    }

    /// The text from byte `from` to `to` without its `?` or `-` indicator.
    fn text(&self, from: usize, to: usize) -> Result<&str, &'static str> {
        let written = self.buffer[from..to.max(from)].trim_matches(is_space);
        let text = written
            .strip_prefix(['?', '-'])
            .filter(|rest| rest.starts_with(is_space))
            .map_or(written, |rest| rest.trim_start_matches(is_space));
        if text.contains(['\n', '\r']) {
            return Err("an entry spans lines");
        }
        Ok(text)
    }

    fn skip_space(&self, from: usize) -> usize {
        self.buffer[from..]
            .find(|ch| !is_space(ch))
            .map_or(self.buffer.len(), |at| from + at)
    }

    fn after_entry(&self, end: usize) -> usize {
        let next = self.skip_space(end);
        next + usize::from(self.buffer[next..].starts_with(','))
    }

    fn break_to(&self, col: usize) -> String {
        format!("{}{}", self.newline, " ".repeat(col))
    }
}

/// Whether `after` holds the same data as `before`, comments and collection style aside.
fn same_data(before: &str, after: &str) -> bool {
    fn data(text: &str) -> Option<Vec<Event<'_>>> {
        Parser::new_from_str(text)
            .map(|item| {
                item.map(|(event, _)| match event {
                    Event::SequenceStart(_, anchor, tag) => {
                        Event::SequenceStart(StructureStyle::Block, anchor, tag)
                    }
                    Event::MappingStart(_, anchor, tag) => {
                        Event::MappingStart(StructureStyle::Block, anchor, tag)
                    }
                    event => event,
                })
            })
            .filter(|item| !matches!(item, Ok(Event::Comment(..))))
            .collect::<Result<_, _>>()
            .ok()
    }
    data(before) == data(after)
}
