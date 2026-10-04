//! Whole-line extents of each mapping entry, so a fixer can permute entries without
//! touching their text.

use granit_parser::{
    Event, Parser, Placement, ScalarStyle, Scanner, Span, SpannedEventReceiver,
    StrInput, StructureStyle, TokenType,
};

use crate::directives::directive_scope;
use crate::rules::support::line_syntax::split_lines_preserve_endings;
use crate::yaml_dom::core_schema_suffix;

pub(crate) struct Key {
    pub(crate) text: String,
    pub(crate) style: ScalarStyle,
}

pub(crate) struct Entry {
    /// `None` for a collection or alias key.
    pub(crate) key: Option<Key>,
    pub(crate) key_line: usize,
    pub(crate) key_col: usize,
    /// First line, including leading comments.
    pub(crate) start: usize,
    /// Last line, including trailing comments.
    pub(crate) end: usize,
    pub(crate) ends_keep: bool,
    pub(crate) anchors: Vec<String>,
    pub(crate) aliases: Vec<String>,
}

pub(crate) struct Mapping {
    pub(crate) block: bool,
    pub(crate) keyed_or_tagged: bool,
    pub(crate) loose_comment: bool,
    pub(crate) entries: Vec<Entry>,
}

pub(crate) struct Layout<'a> {
    pub(crate) lines: Vec<(&'a str, &'a str)>,
    /// Innermost first, so rendering in order sorts children before parents.
    pub(crate) mappings: Vec<Mapping>,
    /// Lines of directive comments a moved line would detach, each flagged when it is
    /// an own-line `disable-line` (which targets the line below).
    pub(crate) directives: Vec<(usize, bool)>,
}

type End = Option<(usize, usize, bool)>;
const BALANCED: &str = "granit closes every collection it opens";

impl<'a> Layout<'a> {
    pub(crate) fn parse(buffer: &'a str) -> Option<Self> {
        let mut events = Events(Vec::new());
        Parser::new_from_str(buffer).load(&mut events, true).ok()?;
        let mut layout = Self {
            lines: split_lines_preserve_endings(buffer)
                .map(|(_, content, ending)| (content, ending))
                .collect(),
            mappings: Vec::new(),
            directives: Vec::new(),
        };
        let mut iter = events.0.into_iter().peekable();
        while iter.peek().is_some() {
            layout.node(&mut iter, false);
        }
        let mut names = Vec::new();
        for token in Scanner::new(StrInput::new(buffer)).map_while(Result::ok) {
            let (span, token) = token.into_parts();
            let line = span.start.line() - 1;
            match token {
                TokenType::Anchor(name) => names.push((line, name, true)),
                TokenType::Alias(name) => names.push((line, name, false)),
                TokenType::Comment(comment) => {
                    let own_line = comment.placement() != Placement::Right;
                    match directive_scope(comment.text()) {
                        Some(true) if !own_line => {}
                        Some(line_scoped) => {
                            layout.directives.push((line, line_scoped));
                        }
                        None => {}
                    }
                }
                _ => {}
            }
        }
        for mapping in layout.mappings.iter_mut().filter(|m| m.block) {
            layout_block(&layout.lines, mapping);
            for (line, name, anchor) in &names {
                if let Some(entry) = mapping
                    .entries
                    .iter_mut()
                    .find(|e| (e.key_line..=e.end).contains(line))
                {
                    let list = if *anchor {
                        &mut entry.anchors
                    } else {
                        &mut entry.aliases
                    };
                    list.push(name.to_string());
                }
            }
        }
        Some(layout)
    }

    fn node(
        &mut self,
        iter: &mut std::iter::Peekable<std::vec::IntoIter<(Event<'a>, Span)>>,
        in_key: bool,
    ) -> End {
        let (event, span) = iter.next().expect(BALANCED);
        let at = |keep| Some((span.end.line() - 1, span.end.col(), keep));
        match event {
            Event::Scalar(_, ScalarStyle::Literal | ScalarStyle::Folded, ..) => {
                at(self.keep_header(span))
            }
            Event::SequenceStart(style, ..) => {
                let mut end = None;
                while !matches!(iter.peek(), Some((Event::SequenceEnd, _)) | None) {
                    end = end.max(self.node(iter, in_key));
                }
                let (_, close) = iter.next().expect(BALANCED);
                flow_end(style, end, close)
            }
            Event::MappingStart(style, _, tag) => {
                let mut mapping = Mapping {
                    block: style == StructureStyle::Block,
                    keyed_or_tagged: in_key
                        || tag.is_some_and(|tag| {
                            core_schema_suffix(&tag).as_deref() != Some("map")
                        }),
                    loose_comment: false,
                    entries: Vec::new(),
                };
                let mut end = None;
                while !matches!(iter.peek(), Some((Event::MappingEnd, _)) | None) {
                    let key = match iter.peek() {
                        Some((Event::Scalar(text, style, ..), _)) => Some(Key {
                            text: text.to_string(),
                            style: *style,
                        }),
                        _ => None,
                    };
                    let key_start = iter.peek().expect(BALANCED).1.start;
                    let entry_end =
                        Some((key_start.line() - 1, key_start.col(), false))
                            .max(self.node(iter, true))
                            .max(self.node(iter, in_key));
                    end = end.max(entry_end);
                    let (line, col, ends_keep) = entry_end.unwrap_or_default();
                    mapping.entries.push(Entry {
                        key,
                        key_line: key_start.line() - 1,
                        key_col: key_start.col(),
                        start: key_start.line() - 1,
                        end: self.content_end(key_start.line() - 1, line, col),
                        ends_keep,
                        anchors: Vec::new(),
                        aliases: Vec::new(),
                    });
                }
                let (_, close) = iter.next().expect(BALANCED);
                self.mappings.push(mapping);
                flow_end(style, end, close)
            }
            // An empty scalar sits at the next token, which may be lines further on.
            Event::Scalar(..) if span.start.index() == span.end.index() => None,
            _ => at(false),
        }
    }

    /// The last line holding entry content, given a span end that may sit at the start
    /// of the next line (block scalars end where the next token begins).
    fn content_end(&self, key_line: usize, line: usize, col: usize) -> usize {
        let mut last = line.min(self.lines.len() - 1);
        if last == line
            && last > key_line
            && is_blank(&self.lines[line].0[..byte_at(self.lines[line].0, col)])
        {
            last -= 1;
        }
        while last > key_line && is_blank(self.lines[last].0) {
            last -= 1;
        }
        last
    }

    /// Whether the block scalar ending at `span` has a keep (`+`) chomping header, found
    /// on the last non-blank line before its content.
    fn keep_header(&self, span: Span) -> bool {
        let header = self.lines[..span.start.line() - 1]
            .iter()
            .rev()
            .find(|(line, _)| !is_blank(line))
            .map_or("", |(line, _)| line);
        header
            .match_indices(['|', '>'])
            .find_map(|(idx, _)| {
                let rest = &header[idx + 1..];
                let after = rest.trim_start_matches(|c: char| {
                    c == '+' || c == '-' || c.is_ascii_digit()
                });
                let tail = after.trim_start();
                (tail.is_empty() || (tail.starts_with('#') && tail.len() < after.len()))
                    .then(|| rest[..rest.len() - after.len()].contains('+'))
            })
            .unwrap_or(false)
    }
}

fn flow_end(style: StructureStyle, end: End, close: Span) -> End {
    if style == StructureStyle::Flow {
        end.max(Some((close.end.line() - 1, close.end.col(), false)))
    } else {
        end
    }
}

pub(crate) fn is_blank(line: &str) -> bool {
    line.trim_matches([' ', '\t']).is_empty()
}

/// Byte offset of char column `col`, past a leading BOM that granit does not count.
pub(crate) fn byte_at(line: &str, col: usize) -> usize {
    let bom = if line.starts_with('\u{feff}') {
        '\u{feff}'.len_utf8()
    } else {
        0
    };
    line[bom..]
        .char_indices()
        .nth(col)
        .map_or(line.len(), |(idx, _)| bom + idx)
}

enum Line {
    Blank,
    Comment(usize),
    Other,
}

fn classify(line: &str) -> Line {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if is_blank(line) {
        Line::Blank
    } else if line[indent..].starts_with('#') {
        Line::Comment(indent)
    } else {
        Line::Other
    }
}

/// Extend each entry over the comments attached to it: a run at the key column directly
/// above the key leads it, a run indented deeper directly after its content trails it.
/// Anything else between two entries marks the mapping `loose_comment`.
fn layout_block(lines: &[(&str, &str)], mapping: &mut Mapping) {
    let count = mapping.entries.len();
    for index in 0..count {
        let col = mapping.entries[index].key_col;
        let floor = if index == 0 {
            0
        } else {
            mapping.entries[index - 1].end + 1
        };
        let mut end = mapping.entries[index].end;
        let limit = mapping
            .entries
            .get(index + 1)
            .map_or(lines.len(), |e| e.key_line);
        while end + 1 < limit
            && matches!(classify(lines[end + 1].0), Line::Comment(c) if c > col)
        {
            end += 1;
        }
        let entry = &mut mapping.entries[index];
        entry.end = end;
        let indented = lines[entry.key_line].0.chars().take(col).all(|c| c == ' ');
        while indented
            && entry.start > floor
            && matches!(classify(lines[entry.start - 1].0), Line::Comment(c) if c == col)
        {
            entry.start -= 1;
        }
        if index > 0 {
            let gap = mapping.entries[index - 1].end + 1..mapping.entries[index].start;
            mapping.loose_comment |= lines[gap].iter().any(|(line, _)| !is_blank(line));
        }
    }
}

struct Events<'a>(Vec<(Event<'a>, Span)>);

impl<'a> SpannedEventReceiver<'a> for Events<'a> {
    fn on_event(&mut self, event: Event<'a>, span: Span) {
        if event.is_node() || matches!(event, Event::SequenceEnd | Event::MappingEnd) {
            self.0.push((event, span));
        }
    }
}

impl Layout<'_> {
    /// The buffer with each listed mapping's entries dealt into its slots: `order[slot]`
    /// is the entry that moves there. Each slot keeps its key-line prefix (indent or
    /// `- `), the blank lines after it, and every line keeps its own terminator.
    pub(crate) fn render(&self, sorts: &[(usize, Vec<usize>)]) -> String {
        let mut lines: Vec<String> = self
            .lines
            .iter()
            .map(|(line, _)| (*line).to_owned())
            .collect();
        for (index, order) in sorts {
            let entries = &self.mappings[*index].entries;
            let (first, last) = (entries[0].start, entries[entries.len() - 1].end);
            let mut out = Vec::with_capacity(last + 1 - first);
            for (slot, &from) in order.iter().enumerate() {
                let (source, target) = (&entries[from], &entries[slot]);
                for line in source.start..=source.end {
                    if line == source.key_line {
                        let prefix = &lines[target.key_line];
                        let body = &lines[line];
                        out.push(format!(
                            "{}{}",
                            &prefix[..byte_at(prefix, target.key_col)],
                            &body[byte_at(body, source.key_col)..]
                        ));
                    } else {
                        out.push(lines[line].clone());
                    }
                }
                if let Some(next) = entries.get(slot + 1) {
                    out.extend_from_slice(&lines[target.end + 1..next.start]);
                }
            }
            lines.splice(first..=last, out);
        }
        let mut out = String::new();
        for (line, (_, ending)) in lines.iter().zip(&self.lines) {
            out.push_str(line);
            out.push_str(ending);
        }
        out
    }
}
