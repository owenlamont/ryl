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
        let kind = Kind::new(kind);
        let index = match kind {
            Kind::StreamStart => 0,
            Kind::StreamEnd => 1,
            Kind::DocumentStart => 2,
            Kind::DocumentEnd => 3,
            Kind::BlockSequenceStart => 4,
            Kind::BlockMappingStart => 5,
            Kind::BlockEnd => 6,
            Kind::FlowSequenceStart => 7,
            Kind::FlowSequenceEnd => 8,
            Kind::FlowMappingStart => 9,
            Kind::FlowMappingEnd => 10,
            Kind::BlockEntry => 11,
            Kind::Key => 12,
            Kind::Value => 13,
            Kind::Alias => 14,
            Kind::Anchor => 15,
            Kind::Tag => 16,
            Kind::Other => 17,
            Kind::Scalar { style, empty } => {
                18 + 2 * match style {
                    ScalarStyle::Plain => 0,
                    ScalarStyle::SingleQuoted => 1,
                    ScalarStyle::DoubleQuoted => 2,
                    ScalarStyle::Literal => 3,
                    ScalarStyle::Folded => 4,
                } + u32::from(empty)
            }
            Kind::Comment { placement, .. } => {
                28 + u32::try_from(
                    PLACEMENTS
                        .iter()
                        .position(|&value| value == placement)
                        .expect("all scanner placements are represented"),
                )
                .expect("placement index fits u32")
            }
        };
        self.records.push((self.spans.store(span), index));
    }

    pub(crate) fn iter<'a>(
        &'a self,
        source: &'a str,
    ) -> impl Iterator<Item = (Span, Kind<'a>)> {
        self.records.iter().map(move |&(span, kind)| {
            let span = self.spans.get(span);
            let kind = if kind < 18 {
                COMMON[kind as usize]
            } else if kind < 28 {
                Kind::Scalar {
                    style: STYLES[((kind - 18) / 2) as usize],
                    empty: kind % 2 != 0,
                }
            } else {
                Kind::Comment {
                    text: "",
                    placement: PLACEMENTS[(kind - 28) as usize],
                }
            };
            (span, kind.with_source(source, &span))
        })
    }
}

impl<'text> Kind<'text> {
    #[inline]
    fn new(kind: TokenKind<'_>) -> Self {
        match kind {
            TokenKind::StreamStart => Kind::StreamStart,
            TokenKind::StreamEnd => Kind::StreamEnd,
            TokenKind::DocumentStart => Kind::DocumentStart,
            TokenKind::DocumentEnd => Kind::DocumentEnd,
            TokenKind::BlockSequenceStart => Kind::BlockSequenceStart,
            TokenKind::BlockMappingStart => Kind::BlockMappingStart,
            TokenKind::BlockEnd => Kind::BlockEnd,
            TokenKind::FlowSequenceStart => Kind::FlowSequenceStart,
            TokenKind::FlowSequenceEnd => Kind::FlowSequenceEnd,
            TokenKind::FlowMappingStart => Kind::FlowMappingStart,
            TokenKind::FlowMappingEnd => Kind::FlowMappingEnd,
            TokenKind::BlockEntry => Kind::BlockEntry,
            TokenKind::Key => Kind::Key,
            TokenKind::Value => Kind::Value,
            TokenKind::Alias => Kind::Alias,
            TokenKind::Anchor => Kind::Anchor,
            TokenKind::Tag => Kind::Tag,
            TokenKind::Scalar { style, value } => Kind::Scalar {
                style,
                empty: value.is_empty(),
            },
            TokenKind::Comment { placement, .. } => Kind::Comment {
                placement,
                text: "",
            },
            _ => Kind::Other,
        }
    }
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

pub(super) fn scan(text: &str) -> Tokens {
    let mut tokens = Tokens::default();
    for token in Scanner::new(StrInput::new(text)).map_while(Result::ok) {
        tokens.push(&token.span(), token.token_type().kind());
    }
    tokens
}

pub(crate) fn stream(text: &str) -> impl Iterator<Item = (Span, Kind<'_>)> {
    Scanner::new(StrInput::new(text))
        .map_while(Result::ok)
        .map(|token| {
            (
                token.span(),
                Kind::new(token.token_type().kind()).with_source(text, &token.span()),
            )
        })
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
