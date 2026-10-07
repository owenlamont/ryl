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
}

#[derive(Debug, Clone)]
pub struct SeqItem {
    pub dash_spaces: u8,
    pub body: SeqBody,
}

#[derive(Debug, Clone)]
pub enum SeqBody {
    Inline(Node),
    BlockScalar(BlockScalarSpec),
    MultilineQuoted(MultilineQuotedSpec),
    MultilinePlain(MultilinePlainSpec),
    /// `- !!map` with the mapping on the lines below.
    TaggedMap(Vec<BlockEntry>),
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
}

#[derive(Debug, Clone)]
pub struct BlockScalarSpec {
    pub properties: &'static str,
    pub style: char,
    pub chomp: Option<char>,
    pub explicit_indent: Option<u8>,
    pub body: Vec<BlockBodyLine>,
}

#[derive(Debug, Clone)]
pub enum BlockBodyLine {
    Content { text: String, trailing_ws: u8 },
    Blank,
}

#[derive(Debug, Clone)]
pub struct MultilineQuotedSpec {
    pub style: MultilineQuoteStyle,
    pub lines: Vec<MultilineLine>,
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
    fn render(&self, buffer: &mut String) {
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
            | Self::BlockSeq(_) => {
                unreachable!("multi-line nodes must be rendered via BlockEntry");
            }
        }
    }
}

impl BlockScalarSpec {
    /// Renders the body `explicit_indent` (or 2) columns past `base`, the parent's
    /// indentation.
    fn render(&self, buffer: &mut String, line_term: &str, base: usize) {
        buffer.push_str(self.properties);
        buffer.push(self.style);
        if let Some(n) = self.explicit_indent {
            buffer.push((b'0' + n) as char);
        }
        if let Some(c) = self.chomp {
            buffer.push(c);
        }
        let indent = base + self.body_indent();
        for line in &self.body {
            buffer.push_str(line_term);
            if let BlockBodyLine::Content { text, trailing_ws } = line {
                for _ in 0..indent {
                    buffer.push(' ');
                }
                buffer.push_str(text);
                for _ in 0..*trailing_ws {
                    buffer.push(' ');
                }
            }
        }
    }

    fn body_indent(&self) -> usize {
        self.explicit_indent.map_or(2, usize::from)
    }
}

impl MultilineQuotedSpec {
    fn render(&self, buffer: &mut String, line_term: &str, indent: &str) {
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
                    buffer.push_str(indent);
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
    fn render(&self, buffer: &mut String, line_term: &str, indent: &str) {
        buffer.push_str(&self.first);
        for line in &self.continuations {
            buffer.push_str(line_term);
            if let MultilineLine::Content(text) = line {
                buffer.push_str(indent);
                buffer.push_str("  ");
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
            buffer.push_str(&format!("{indent}# {comment}{line_term}"));
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
        if let (Some(spaces), true) = (self.colon.explicit, single_line) {
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
        let child = format!("{indent}  ");
        match &self.value {
            Node::BlockMap(entries) => {
                for entry in entries {
                    buffer.push_str(line_term);
                    entry.render_at(buffer, line_term, &child);
                }
                return;
            }
            Node::BlockSeq(items) => {
                for item in items {
                    buffer.push_str(line_term);
                    item.render(buffer, line_term, &child);
                }
                return;
            }
            _ => {}
        }
        self.colon.render_after(buffer);
        match &self.value {
            Node::BlockScalar(spec) => spec.render(buffer, line_term, indent.len()),
            Node::MultilineQuoted(spec) => spec.render(buffer, line_term, indent),
            Node::MultilinePlain(spec) => spec.render(buffer, line_term, indent),
            value => {
                value.render(buffer);
                self.render_trailing_comment(buffer);
            }
        }
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
        push_spaces(buffer, self.dash_spaces);
        let deeper = format!("{indent}  ");
        let content = format!("{indent} {}", " ".repeat(usize::from(self.dash_spaces)));
        match &self.body {
            SeqBody::Inline(node) => node.render(buffer),
            SeqBody::BlockScalar(spec) => spec.render(buffer, line_term, indent.len()),
            SeqBody::MultilineQuoted(spec) => spec.render(buffer, line_term, &deeper),
            SeqBody::MultilinePlain(spec) => spec.render(buffer, line_term, &deeper),
            SeqBody::TaggedMap(entries) => {
                buffer.push_str("!!map");
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
