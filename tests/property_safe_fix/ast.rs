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
}

#[derive(Debug, Clone)]
pub struct MultilinePlainSpec {
    pub first: String,
    pub continuations: Vec<MultilineLine>,
}

#[derive(Debug, Clone)]
pub struct BlockScalarSpec {
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
    pub space_after_colon: bool,
}

#[derive(Debug, Clone)]
pub struct InlineComment {
    pub spaces_after_hash: u8,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct BlockEntry {
    pub key: String,
    pub value: Node,
    pub trailing_inline_comment: Option<InlineComment>,
}

#[derive(Debug, Clone, Copy)]
pub enum NewlineStyle {
    Lf,
    Crlf,
    /// A bare `\r` — a YAML 1.2 line break the fixers now honour everywhere
    ///, so the safe-fix matrix exercises `\r`-delimited documents.
    Cr,
}

#[derive(Debug, Clone)]
pub struct Document {
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

    fn render(&self, buffer: &mut String) {
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
                    buffer.push(':');
                    if style.space_after_colon || !key.is_explicitly_quoted() {
                        buffer.push(' ');
                    }
                    value.render(buffer);
                }
                push_spaces(buffer, style.inner_padding);
                buffer.push('}');
            }
            Self::BlockScalar(_)
            | Self::MultilineQuoted(_)
            | Self::MultilinePlain(_) => {
                unreachable!("multi-line nodes must be rendered via BlockEntry");
            }
        }
    }
}

impl BlockScalarSpec {
    fn render(&self, buffer: &mut String, line_term: &str) {
        buffer.push(self.style);
        if let Some(n) = self.explicit_indent {
            buffer.push((b'0' + n) as char);
        }
        if let Some(c) = self.chomp {
            buffer.push(c);
        }
        let indent = self.body_indent();
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
    fn render(&self, buffer: &mut String, line_term: &str) {
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
    fn render(&self, buffer: &mut String, line_term: &str) {
        buffer.push_str(&self.first);
        for line in &self.continuations {
            buffer.push_str(line_term);
            if let MultilineLine::Content(text) = line {
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
        buffer.push_str(&self.key);
        buffer.push_str(": ");
        let allow_trailing_comment = match &self.value {
            Node::Scalar(_) | Node::FlowSeq(_, _) | Node::FlowMap(_, _) => {
                self.value.render(buffer);
                true
            }
            Node::BlockScalar(spec) => {
                spec.render(buffer, line_term);
                false
            }
            Node::MultilineQuoted(spec) => {
                spec.render(buffer, line_term);
                false
            }
            Node::MultilinePlain(spec) => {
                spec.render(buffer, line_term);
                false
            }
        };
        if allow_trailing_comment && let Some(comment) = &self.trailing_inline_comment {
            buffer.push_str("  #");
            push_spaces(buffer, comment.spaces_after_hash);
            buffer.push_str(&comment.text);
        }
    }
}
