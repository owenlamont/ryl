//! `ryl format`'s `[format] sequence-style` and `mapping-style`: flow collections
//! rewritten in block style, each entry's text copied as written.

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
}

/// A flow collection the pass rewrites, or leaves alone for `refused`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub line: usize,
    pub column: usize,
    pub rule: &'static str,
    pub refused: Option<&'static str>,
}

impl Finding {
    #[must_use]
    pub fn into_problem(self) -> LintProblem {
        let kind = if self.rule == braces::ID {
            "mapping"
        } else {
            "sequence"
        };
        LintProblem {
            line: self.line,
            column: self.column,
            level: Severity::Error,
            message: self.refused.map_or_else(
                || format!("flow {kind} would become block"),
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
    let to_block = |target, rule| {
        target == CollectionStyleTarget::Block
            && !directives.disables_any(rule)
            && !disables_file(buffer)
    };
    let restyler = Restyler {
        buffer,
        nodes: tree(buffer),
        sequences: to_block(cfg.sequences, brackets::ID),
        mappings: to_block(cfg.mappings, braces::ID),
        indent: usize::from(cfg.indent),
        newline: buffer_newline(buffer),
    };
    let mut edits = Vec::new();
    let mut findings = Vec::new();
    for (index, node) in restyler.nodes.iter().enumerate() {
        if !restyler.converts(index)
            || node
                .parent
                .is_some_and(|parent| restyler.nodes[parent].flow)
        {
            continue;
        }
        let outcome = restyler.outermost(index);
        findings.push(Finding {
            line: node.line,
            column: node.column + 1,
            rule: if node.kind == Kind::Mapping {
                braces::ID
            } else {
                brackets::ID
            },
            refused: outcome.as_ref().err().copied(),
        });
        edits.extend(outcome.ok());
    }
    (edits, findings)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Scalar { plain: bool },
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
    comment: bool,
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
            Event::Scalar(_, style, ..) => (
                Kind::Scalar {
                    plain: style == ScalarStyle::Plain,
                },
                false,
            ),
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
                let comment = node.comment;
                if let Some(&parent) = open.last() {
                    nodes[parent].comment |= comment;
                }
                continue;
            }
            Event::Comment(..) => {
                if let Some(&index) = open.last() {
                    nodes[index].comment = true;
                }
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
            comment: false,
        });
        if matches!(kind, Kind::Sequence | Kind::Mapping) {
            open.push(index);
        }
    }
    nodes
}

fn is_space(ch: char) -> bool {
    matches!(ch, ' ' | '\t' | '\n' | '\r')
}

struct Restyler<'a> {
    buffer: &'a str,
    nodes: Vec<Node>,
    sequences: bool,
    mappings: bool,
    indent: usize,
    newline: &'static str,
}

impl Restyler<'_> {
    fn converts(&self, index: usize) -> bool {
        let node = &self.nodes[index];
        node.braced
            && !node.children.is_empty()
            && match node.kind {
                Kind::Sequence => self.sequences,
                _ => self.mappings,
            }
    }

    /// The edit rewriting collection `index`, whose parent is block or the document.
    fn outermost(&self, index: usize) -> Result<Edit, &'static str> {
        let node = &self.nodes[index];
        if let Some(parent) = node.parent
            && self.nodes[parent].kind == Kind::Mapping
            && self.nodes[parent]
                .children
                .iter()
                .position(|&child| child == index)
                .expect("a node is among its parent's children")
                % 2
                == 0
        {
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
        if outer.match_indices(['-', '?', ':']).any(|(at, _)| {
            outer[at + 1..].starts_with("  ") && (at == 0 || outer[..at].ends_with(' '))
        }) {
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
        if node.comment {
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
                if key.kind == (Kind::Scalar { plain: true })
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
        if !self.converts(index) {
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
        if !self.converts(index) {
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
        if node.kind == (Kind::Scalar { plain: true })
            && self.buffer[node.start..].starts_with('?')
        {
            return Err("an entry starts with `?`");
        }
        self.text(pos, node.end)
    }

    fn text(&self, from: usize, to: usize) -> Result<&str, &'static str> {
        let written = self.buffer[from..to.max(from)].trim_matches(is_space);
        let text = written
            .strip_prefix('?')
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
