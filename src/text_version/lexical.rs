use ryl_yaml_parser::{Placement, ScalarStyle, Scanner, Span, StrInput, TokenKind};

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

#[derive(Default)]
pub(crate) struct Tokens {
    records: Vec<(super::spans::StoredSpan, u32)>,
    spans: super::spans::Spans,
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
        self.records.push((self.spans.store(span), index));
    }

    /// Byte ranges and kinds; comment text is left empty.
    pub(crate) fn bytes(
        &self,
    ) -> impl Iterator<Item = (usize, usize, Kind<'static>)> + '_ {
        self.records.iter().map(|&(span, kind)| {
            let (start, end) = self.spans.bytes(span);
            (start, end, decode(kind))
        })
    }

    pub(crate) fn iter<'a>(
        &'a self,
        source: &'a str,
    ) -> impl Iterator<Item = (Span, Kind<'a>)> {
        self.records.iter().map(move |&(span, kind)| {
            let span = self.spans.get(span);
            (span, decode(kind).with_source(source, &span))
        })
    }
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

fn decode(kind: u32) -> Kind<'static> {
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
