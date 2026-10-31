use ryl_yaml_parser::{
    Marker, Placement, ScalarStyle, Scanner, Span, StrInput, TokenKind,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind<'text> {
    StreamStart,
    StreamEnd,
    DocumentStart,
    DocumentEnd,
    BlockSequenceStart,
    BlockMappingStart,
    BlockEnd,
    FlowSequenceStart,
    FlowSequenceEnd,
    FlowMappingStart,
    FlowMappingEnd,
    FlowEntry,
    BlockEntry,
    Key,
    Value,
    Alias,
    Anchor,
    Tag,
    Scalar {
        style: ScalarStyle,
        empty: bool,
    },
    Comment {
        text: &'text str,
        placement: Placement,
    },
    Other,
}

const WIDE: u32 = u32::MAX;
const EXTRA: u32 = 0x80;

/// 16 bytes per token: start and end byte, start line above an 8-bit kind code, and start
/// column. A token's character index is its byte less the UTF-8 continuation bytes before
/// it, `delta`, which only a token flagged `EXTRA` changes; such a token, spanning lines,
/// holding a multibyte character or carrying an indent, also stores its end line, end
/// column, indent and end delta. Anything else stores its whole span in `wide`.
#[derive(Default)]
pub(crate) struct Tokens {
    records: Vec<[u32; 4]>,
    extras: Vec<[u32; 4]>,
    wide: Vec<Span>,
    delta: u32,
}

impl Tokens {
    #[inline]
    pub(super) fn push(&mut self, span: &Span, kind: TokenKind<'_>) {
        let index = match kind {
            TokenKind::StreamStart => 0,
            TokenKind::StreamEnd => 1,
            TokenKind::DocumentStart => 2,
            TokenKind::DocumentEnd => 3,
            TokenKind::BlockSequenceStart => 4,
            TokenKind::BlockMappingStart => 5,
            TokenKind::BlockEnd => 6,
            TokenKind::FlowSequenceStart => 7,
            TokenKind::FlowSequenceEnd => 8,
            TokenKind::FlowMappingStart => 9,
            TokenKind::FlowMappingEnd => 10,
            TokenKind::BlockEntry => 11,
            TokenKind::Key => 12,
            TokenKind::Value => 13,
            TokenKind::Alias => 14,
            TokenKind::Anchor => 15,
            TokenKind::Tag => 16,
            TokenKind::FlowEntry => 17,
            TokenKind::Scalar { style, value } => {
                19 + 2 * match style {
                    ScalarStyle::Plain => 0,
                    ScalarStyle::SingleQuoted => 1,
                    ScalarStyle::DoubleQuoted => 2,
                    ScalarStyle::Literal => 3,
                    ScalarStyle::Folded => 4,
                } + u32::from(value.is_empty())
            }
            TokenKind::Comment { placement, .. } => {
                29 + u32::try_from(
                    PLACEMENTS
                        .iter()
                        .position(|&value| value == placement)
                        .expect("all scanner placements are represented"),
                )
                .expect("placement index fits u32")
            }
            _ => 18,
        };
        let (start, end) = (span.start, span.end);
        let (a, b) = (
            start.byte_offset().unwrap_or(usize::MAX),
            end.byte_offset().unwrap_or(usize::MAX),
        );
        let (delta, to) = (char_delta(start), char_delta(end));
        let widest = [a, b, delta, to, start.col(), end.line(), end.col()]
            .into_iter()
            .fold(span.indent.unwrap_or(0), usize::max);
        let narrow = |value: usize| u32::try_from(value).unwrap_or(WIDE);
        if (widest >= WIDE as usize)
            | (start.line() >= 1 << 24)
            | span.tag_start.is_some()
            | (narrow(delta) != self.delta)
        {
            self.delta = narrow(to);
            self.records.push([WIDE, narrow(self.wide.len()), index, 0]);
            self.wide.push(*span);
            return;
        }
        let line = narrow(start.line());
        let mut record = [narrow(a), narrow(b), line << 8 | index, narrow(start.col())];
        let extra = [
            narrow(end.line()),
            narrow(end.col()),
            span.indent.map_or(WIDE, narrow),
            narrow(to),
        ];
        let [a, b, code, column] = record;
        if extra
            != [
                code >> 8,
                column.wrapping_add(b.wrapping_sub(a)),
                WIDE,
                self.delta,
            ]
        {
            record[2] |= EXTRA;
            self.extras.push(extra);
            self.delta = extra[3];
        }
        self.records.push(record);
    }

    /// Byte ranges and kinds; comment text is left empty.
    pub(crate) fn bytes(
        &self,
    ) -> impl Iterator<Item = (usize, usize, Kind<'static>)> + '_ {
        self.records.iter().map(|&[start, end, code, _]| {
            let (start, end) = if start == WIDE {
                let span = self.wide[end as usize];
                let byte = |marker: Marker| marker.byte_offset().unwrap_or(usize::MAX);
                (byte(span.start), byte(span.end))
            } else {
                (start as usize, end as usize)
            };
            (start, end, decode(code))
        })
    }

    pub(crate) fn iter<'a>(
        &'a self,
        source: &'a str,
    ) -> impl Iterator<Item = (Span, Kind<'a>)> {
        let mut extras = self.extras.iter();
        let mut delta = 0;
        self.records.iter().map(move |&[start, end, code, column]| {
            if start == WIDE {
                let span = self.wide[end as usize];
                delta = char_delta(span.end);
                return (span, decode(code).with_source(source, &span));
            }
            let (start, end, line) =
                (start as usize, end as usize, (code >> 8) as usize);
            let from = start - delta;
            let (end_line, end_column, indent) = if code & EXTRA == 0 {
                (line, column as usize + end - start, None)
            } else {
                let &[line, column, indent, to] =
                    extras.next().expect("flagged extra is stored");
                delta = to as usize;
                let indent = (indent != WIDE).then_some(indent as usize);
                (line as usize, column as usize, indent)
            };
            let span = Span::new(
                Marker::new(from, line, column as usize).with_byte_offset(Some(start)),
                Marker::new(end - delta, end_line, end_column)
                    .with_byte_offset(Some(end)),
            )
            .with_indent(indent);
            (span, decode(code).with_source(source, &span))
        })
    }
}

/// Continuation bytes before `marker`, the gap between its byte offset and character index.
fn char_delta(marker: Marker) -> usize {
    marker
        .byte_offset()
        .unwrap_or(usize::MAX)
        .wrapping_sub(marker.index())
}

impl<'text> Kind<'text> {
    fn with_source(mut self, source: &'text str, span: &Span) -> Self {
        if let Self::Comment { text, .. } = &mut self {
            let start = span
                .start
                .byte_offset()
                .expect("source token has byte offset")
                + 1;
            let end = span
                .end
                .byte_offset()
                .expect("source token has byte offset");
            *text = &source[start..end];
        }
        self
    }
}

fn decode(code: u32) -> Kind<'static> {
    let kind = code & 0x7f;
    if kind < 19 {
        COMMON[kind as usize]
    } else if kind < 29 {
        Kind::Scalar {
            style: STYLES[((kind - 19) / 2) as usize],
            empty: !(kind - 19).is_multiple_of(2),
        }
    } else {
        Kind::Comment {
            text: "",
            placement: PLACEMENTS[(kind - 29) as usize],
        }
    }
}

pub(super) fn scan(text: &str) -> Tokens {
    let mut tokens = Tokens::default();
    for token in Scanner::new(StrInput::new(text)).map_while(Result::ok) {
        tokens.push(&token.span(), token.token_type().kind());
    }
    tokens
}

static COMMON: &[Kind<'static>] = &[
    Kind::StreamStart,
    Kind::StreamEnd,
    Kind::DocumentStart,
    Kind::DocumentEnd,
    Kind::BlockSequenceStart,
    Kind::BlockMappingStart,
    Kind::BlockEnd,
    Kind::FlowSequenceStart,
    Kind::FlowSequenceEnd,
    Kind::FlowMappingStart,
    Kind::FlowMappingEnd,
    Kind::BlockEntry,
    Kind::Key,
    Kind::Value,
    Kind::Alias,
    Kind::Anchor,
    Kind::Tag,
    Kind::FlowEntry,
    Kind::Other,
];

const STYLES: &[ScalarStyle] = &[
    ScalarStyle::Plain,
    ScalarStyle::SingleQuoted,
    ScalarStyle::DoubleQuoted,
    ScalarStyle::Literal,
    ScalarStyle::Folded,
];

const PLACEMENTS: &[Placement] = &[
    Placement::Above,
    Placement::Right,
    Placement::Free,
    Placement::Last,
];
