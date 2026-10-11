//! Synthetic YAML AST plus rendering used by the proptest generator.

#[derive(Debug, Clone)]
pub enum Scalar {
    Plain(String),
    SingleQuoted(String),
    DoubleQuoted(String),
}

#[derive(Debug, Clone)]
pub enum Node {
    Scalar(Scalar),
    FlowSeq(Vec<Node>, FlowStyle),
    FlowMap(Vec<(Scalar, Node)>, FlowStyle),
    BlockScalar(BlockScalarSpec),
    MultilineQuoted(MultilineQuotedSpec),
    MultilinePlain(MultilinePlainSpec),
    BlockMap(Vec<BlockEntry>),
    BlockSeq(Vec<SeqItem>),
    /// A flow sequence with each item after the first on its own line.
    MultilineFlowSeq(MultilineFlowSpec),
}

/// Continuation lines start `1 + extra` columns past the parent's indentation, so they
/// stay deeper than it; the closer may sit at it.
#[derive(Debug, Clone)]
pub struct MultilineFlowSpec {
    pub items: Vec<(u8, Scalar)>,
    pub closer: u8,
}

#[derive(Debug, Clone)]
pub struct SeqItem {
    pub dash_spaces: u8,
    /// How far past the dash an own-line body sits.
    pub width: u8,
    pub body: SeqBody,
}

#[derive(Debug, Clone)]
pub enum SeqBody {
    Inline(Node),
    BlockScalar(BlockScalarSpec),
    MultilineQuoted(MultilineQuotedSpec),
    MultilinePlain(MultilinePlainSpec),
    /// `- !!map`, `- &m` or a bare `-`, with the mapping on the lines below.
    TaggedMap(&'static str, Vec<BlockEntry>),
    /// The first entry on the dash line and the rest aligned under it: more than one
    /// entry makes the dash spacing the mapping's indentation.
    CompactMap(Vec<BlockEntry>),
    /// `- - a` with further items aligned under the inner dash.
    CompactSeq(Vec<(u8, Node)>),
}

#[derive(Debug, Clone)]
pub struct MultilinePlainSpec {
    pub first: String,
    pub continuations: Vec<MultilineLine>,
    /// Continuation lines sit `1 + extra` columns past the parent's indentation.
    pub extra: u8,
}

#[derive(Debug, Clone)]
pub struct BlockScalarSpec {
    pub properties: &'static str,
    pub style: char,
    pub chomp: Option<char>,
    pub explicit_indent: Option<u8>,
    pub header_spaces: u8,
    /// The body's offset from the parent's indentation when no indicator fixes it.
    pub offset: u8,
    /// A whitespace-only line before the first content line, this many columns short of
    /// the body (wider would be a parse error without an indicator).
    pub leading_short: Option<u8>,
    pub body: Vec<BlockBodyLine>,
    /// A comment line after the body, this many columns short of it.
    pub trailing_comment: Option<u8>,
}

#[derive(Debug, Clone)]
pub enum BlockBodyLine {
    Content {
        text: String,
        trailing_ws: u8,
    },
    Blank,
    /// Whitespace only, as wide as the body plus this (clamped at zero).
    Spaces(i8),
}

#[derive(Debug, Clone)]
pub struct MultilineQuotedSpec {
    pub style: MultilineQuoteStyle,
    pub lines: Vec<MultilineLine>,
    /// Continuation lines sit `2 + extra` columns past the parent's indentation: granit
    /// rejects a quoted continuation one column in, which the spec allows.
    pub extra: u8,
}

#[derive(Debug, Clone, Copy)]
pub enum MultilineQuoteStyle {
    Single,
    Double,
}

#[derive(Debug, Clone)]
pub enum MultilineLine {
    Content(String),
    Blank,
}

#[derive(Debug, Clone, Copy)]
pub struct FlowStyle {
    pub inner_padding: u8,
    pub spaces_before_comma: u8,
    pub spaces_after_comma: u8,
    pub spaces_before_colon: u8,
    pub spaces_after_colon: u8,
}

#[derive(Debug, Clone)]
pub struct InlineComment {
    pub whitespace_before_hash: String,
    pub spaces_after_hash: u8,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct BlockEntry {
    pub leading_comment: Option<String>,
    pub key: String,
    pub colon: ColonGap,
    pub value: Node,
    pub trailing_inline_comment: Option<InlineComment>,
    pub layout: Layout,
}

#[derive(Debug, Clone, Copy)]
pub struct Layout {
    /// Columns from the key to a nested mapping or sequence.
    pub width: u8,
    /// A nested sequence starts at the key's column instead.
    pub flush: bool,
    /// The leading comment's column relative to the key, clamped at zero.
    pub comment_shift: i8,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            width: 2,
            flush: false,
            comment_shift: 0,
        }
    }
}

/// Spacing around a block entry's `:` beyond the canonical `key: value`.
#[derive(Debug, Clone, Copy, Default)]
pub struct ColonGap {
    pub extra_before: u8,
    pub extra_after: u8,
    /// A tab in place of the first space after `:`.
    pub tab: bool,
    /// Spaces after an explicit `?`, which then puts a single-line value on a `:` line.
    pub explicit: Option<u8>,
}

impl ColonGap {
    fn render_after(self, buffer: &mut String) {
        buffer.push(if self.tab { '\t' } else { ' ' });
        push_spaces(buffer, self.extra_after);
    }
}

#[derive(Debug, Clone, Copy)]
pub enum NewlineStyle {
    Lf,
    Crlf,
    /// A bare `\r`: a YAML 1.2 line break the fixers honour everywhere, so the
    /// safe-fix matrix exercises `\r`-delimited documents.
    Cr,
}

#[derive(Debug, Clone)]
pub struct Document {
    pub version_directive: Option<(u32, u32)>,
    pub entries: Vec<BlockEntry>,
    pub newline: NewlineStyle,
    pub has_final_newline: bool,
}

fn push_spaces(buffer: &mut String, count: u8) {
    for _ in 0..count {
        buffer.push(' ');
    }
}

fn shifted(column: usize, shift: i8) -> usize {
    column.saturating_add_signed(isize::from(shift))
}

impl Scalar {
    fn is_explicitly_quoted(&self) -> bool {
        matches!(self, Self::SingleQuoted(_) | Self::DoubleQuoted(_))
    }

    pub fn render(&self, buffer: &mut String) {
        match self {
            Self::Plain(text) => buffer.push_str(text),
            Self::SingleQuoted(text) => {
                buffer.push('\'');
                for ch in text.chars() {
                    if ch == '\'' {
                        buffer.push_str("''");
                    } else {
                        buffer.push(ch);
                    }
                }
                buffer.push('\'');
            }
            Self::DoubleQuoted(text) => {
                buffer.push('"');
                for ch in text.chars() {
                    match ch {
                        '"' => buffer.push_str("\\\""),
                        '\\' => buffer.push_str("\\\\"),
                        '\n' => buffer.push_str("\\n"),
                        '\t' => buffer.push_str("\\t"),
                        _ => buffer.push(ch),
                    }
                }
                buffer.push('"');
            }
        }
    }
}

impl Node {
    pub fn render(&self, buffer: &mut String) {
        match self {
            Self::Scalar(scalar) => scalar.render(buffer),
            Self::FlowSeq(items, style) => {
                buffer.push('[');
                push_spaces(buffer, style.inner_padding);
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        push_spaces(buffer, style.spaces_before_comma);
                        buffer.push(',');
                        push_spaces(buffer, style.spaces_after_comma);
                    }
                    item.render(buffer);
                }
                push_spaces(buffer, style.inner_padding);
                buffer.push(']');
            }
            Self::FlowMap(pairs, style) => {
                buffer.push('{');
                push_spaces(buffer, style.inner_padding);
                for (index, (key, value)) in pairs.iter().enumerate() {
                    if index > 0 {
                        push_spaces(buffer, style.spaces_before_comma);
                        buffer.push(',');
                        push_spaces(buffer, style.spaces_after_comma);
                    }
                    key.render(buffer);
                    push_spaces(buffer, style.spaces_before_colon);
                    buffer.push(':');
                    let adjacent =
                        key.is_explicitly_quoted() && style.spaces_before_colon == 0;
                    push_spaces(
                        buffer,
                        style.spaces_after_colon.max(u8::from(!adjacent)),
                    );
                    value.render(buffer);
                }
                push_spaces(buffer, style.inner_padding);
                buffer.push('}');
            }
            Self::BlockScalar(_)
            | Self::MultilineQuoted(_)
            | Self::MultilinePlain(_)
            | Self::BlockMap(_)
            | Self::BlockSeq(_)
            | Self::MultilineFlowSeq(_) => {
                unreachable!("multi-line nodes must be rendered via BlockEntry");
            }
        }
    }
}

impl BlockScalarSpec {
    /// Renders the body `explicit_indent` (or `offset`) columns past `base`, the
    /// parent's indentation.
    fn render(&self, buffer: &mut String, line_term: &str, base: usize) {
        buffer.push_str(self.properties);
        buffer.push(self.style);
        if let Some(n) = self.explicit_indent {
            buffer.push((b'0' + n) as char);
        }
        if let Some(c) = self.chomp {
            buffer.push(c);
        }
        push_spaces(buffer, self.header_spaces);
        let indent = base + usize::from(self.explicit_indent.unwrap_or(self.offset));
        if let Some(short) = self.leading_short {
            buffer.push_str(line_term);
            buffer.push_str(&" ".repeat(indent.saturating_sub(usize::from(short))));
        }
        for line in &self.body {
            buffer.push_str(line_term);
            match line {
                BlockBodyLine::Content { text, trailing_ws } => {
                    buffer.push_str(&" ".repeat(indent));
                    buffer.push_str(text);
                    push_spaces(buffer, *trailing_ws);
                }
                BlockBodyLine::Blank => {}
                BlockBodyLine::Spaces(shift) => {
                    buffer.push_str(&" ".repeat(shifted(indent, *shift)));
                }
            }
        }
        if let Some(short) = self.trailing_comment {
            buffer.push_str(line_term);
            buffer
                .push_str(&" ".repeat(indent - 1 - usize::from(short).min(indent - 1)));
            buffer.push_str("# after");
        }
    }
}

impl MultilineFlowSpec {
    fn render(&self, buffer: &mut String, line_term: &str, base: usize) {
        buffer.push('[');
        for (index, (extra, item)) in self.items.iter().enumerate() {
            if index > 0 {
                buffer.push(',');
                buffer.push_str(line_term);
                buffer.push_str(&" ".repeat(base + 1 + usize::from(*extra)));
            }
            item.render(buffer);
        }
        buffer.push_str(line_term);
        buffer.push_str(&" ".repeat(base + usize::from(self.closer)));
        buffer.push(']');
    }
}

impl MultilineQuotedSpec {
    fn render(&self, buffer: &mut String, line_term: &str, base: usize) {
        let indent = " ".repeat(base + 2 + usize::from(self.extra));
        let quote = match self.style {
            MultilineQuoteStyle::Single => '\'',
            MultilineQuoteStyle::Double => '"',
        };
        buffer.push(quote);
        for (index, line) in self.lines.iter().enumerate() {
            if index > 0 {
                buffer.push_str(line_term);
            }
            if let MultilineLine::Content(text) = line {
                if index > 0 {
                    buffer.push_str(&indent);
                }
                for ch in text.chars() {
                    match (self.style, ch) {
                        (MultilineQuoteStyle::Single, '\'') => buffer.push_str("''"),
                        (MultilineQuoteStyle::Double, '"') => buffer.push_str("\\\""),
                        (MultilineQuoteStyle::Double, '\\') => buffer.push_str("\\\\"),
                        _ => buffer.push(ch),
                    }
                }
            }
        }
        buffer.push(quote);
    }
}

impl MultilinePlainSpec {
    fn render(&self, buffer: &mut String, line_term: &str, base: usize) {
        buffer.push_str(&self.first);
        for line in &self.continuations {
            buffer.push_str(line_term);
            if let MultilineLine::Content(text) = line {
                buffer.push_str(&" ".repeat(base + 1 + usize::from(self.extra)));
                buffer.push_str(text);
            }
        }
    }
}

impl Document {
    pub fn render(&self) -> String {
        let mut buffer = String::new();
        let line_terminator = match self.newline {
            NewlineStyle::Lf => "\n",
            NewlineStyle::Crlf => "\r\n",
            NewlineStyle::Cr => "\r",
        };
        if let Some((major, minor)) = self.version_directive {
            buffer.push_str(&format!("%YAML {major}.{minor}{line_terminator}---"));
            buffer.push_str(line_terminator);
        }
        for (index, entry) in self.entries.iter().enumerate() {
            if index > 0 {
                buffer.push_str(line_terminator);
            }
            entry.render(&mut buffer, line_terminator);
        }
        if self.has_final_newline {
            buffer.push_str(line_terminator);
        }
        buffer
    }
}

impl BlockEntry {
    fn render(&self, buffer: &mut String, line_term: &str) {
        self.render_at(buffer, line_term, "");
    }

    fn render_at(&self, buffer: &mut String, line_term: &str, indent: &str) {
        if let Some(comment) = &self.leading_comment {
            let column = shifted(indent.len(), self.layout.comment_shift);
            buffer.push_str(&format!("{}# {comment}{line_term}", " ".repeat(column)));
        }
        buffer.push_str(indent);
        self.render_body(buffer, line_term, indent);
    }

    /// Everything from the key on, with `indent` before each later line.
    fn render_body(&self, buffer: &mut String, line_term: &str, indent: &str) {
        let single_line = matches!(
            self.value,
            Node::Scalar(_) | Node::FlowSeq(..) | Node::FlowMap(..)
        );
        let collection = matches!(self.value, Node::BlockMap(_) | Node::BlockSeq(_));
        let explicit = self.colon.explicit.filter(|_| single_line || collection);
        if let Some(spaces) = explicit {
            buffer.push('?');
            push_spaces(buffer, spaces);
            buffer.push_str(&self.key);
            buffer.push_str(line_term);
            buffer.push_str(indent);
        } else {
            buffer.push_str(&self.key);
            push_spaces(buffer, self.colon.extra_before);
        }
        buffer.push(':');
        if explicit.is_some() && collection {
            self.render_compact_value(buffer, line_term, indent);
            return;
        }
        let width = usize::from(self.layout.width);
        let child = format!("{indent}{}", " ".repeat(width));
        match &self.value {
            Node::BlockMap(entries) => {
                for entry in entries {
                    buffer.push_str(line_term);
                    entry.render_at(buffer, line_term, &child);
                }
                return;
            }
            Node::BlockSeq(items) => {
                let child = if self.layout.flush { indent } else { &child };
                for item in items {
                    buffer.push_str(line_term);
                    item.render(buffer, line_term, child);
                }
                return;
            }
            _ => {}
        }
        self.colon.render_after(buffer);
        let base = indent.len();
        match &self.value {
            Node::BlockScalar(spec) => spec.render(buffer, line_term, base),
            Node::MultilineQuoted(spec) => spec.render(buffer, line_term, base),
            Node::MultilinePlain(spec) => spec.render(buffer, line_term, base),
            Node::MultilineFlowSeq(spec) => {
                spec.render(buffer, line_term, base);
                self.render_trailing_comment(buffer);
            }
            value => {
                value.render(buffer);
                self.render_trailing_comment(buffer);
            }
        }
    }

    /// An explicit key's collection value opening on the `:` line (`: - a`), the rest
    /// aligned under its first item.
    fn render_compact_value(&self, buffer: &mut String, line_term: &str, indent: &str) {
        let gap = 1 + self.colon.extra_after;
        push_spaces(buffer, gap);
        let content = format!("{indent} {}", " ".repeat(usize::from(gap)));
        let mut rendered = String::new();
        match &self.value {
            Node::BlockMap(entries) => {
                for (index, entry) in entries.iter().enumerate() {
                    if index == 0 {
                        entry.render_body(&mut rendered, line_term, &content);
                    } else {
                        rendered.push_str(line_term);
                        entry.render_at(&mut rendered, line_term, &content);
                    }
                }
            }
            Node::BlockSeq(items) => {
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        rendered.push_str(line_term);
                    }
                    item.render(&mut rendered, line_term, &content);
                }
                rendered.drain(..content.len());
            }
            _ => unreachable!("only block collections render compact"),
        }
        buffer.push_str(&rendered);
    }

    fn render_trailing_comment(&self, buffer: &mut String) {
        if let Some(comment) = &self.trailing_inline_comment {
            buffer.push_str(&comment.whitespace_before_hash);
            buffer.push('#');
            push_spaces(buffer, comment.spaces_after_hash);
            buffer.push_str(&comment.text);
        }
    }
}

impl SeqItem {
    fn render(&self, buffer: &mut String, line_term: &str, indent: &str) {
        buffer.push_str(indent);
        buffer.push('-');
        if !matches!(self.body, SeqBody::TaggedMap("", _)) {
            push_spaces(buffer, self.dash_spaces);
        }
        let deeper = format!("{indent}{}", " ".repeat(usize::from(self.width)));
        let content = format!("{indent} {}", " ".repeat(usize::from(self.dash_spaces)));
        let base = indent.len();
        match &self.body {
            SeqBody::Inline(node) => node.render(buffer),
            SeqBody::BlockScalar(spec) => spec.render(buffer, line_term, base),
            SeqBody::MultilineQuoted(spec) => spec.render(buffer, line_term, base),
            SeqBody::MultilinePlain(spec) => spec.render(buffer, line_term, base),
            SeqBody::TaggedMap(property, entries) => {
                buffer.push_str(property);
                for entry in entries {
                    buffer.push_str(line_term);
                    entry.render_at(buffer, line_term, &deeper);
                }
            }
            SeqBody::CompactMap(entries) => {
                for (index, entry) in entries.iter().enumerate() {
                    if index == 0 {
                        entry.render_body(buffer, line_term, &content);
                    } else {
                        buffer.push_str(line_term);
                        entry.render_at(buffer, line_term, &content);
                    }
                }
            }
            SeqBody::CompactSeq(items) => {
                for (index, (spaces, node)) in items.iter().enumerate() {
                    if index > 0 {
                        buffer.push_str(line_term);
                        buffer.push_str(&content);
                    }
                    buffer.push('-');
                    push_spaces(buffer, *spaces);
                    node.render(buffer);
                }
            }
        }
    }
}
