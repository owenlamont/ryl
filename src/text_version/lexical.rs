use granit_parser::{Placement, ScalarStyle, Scanner, Span, StrInput, TokenType};

pub(crate) enum Kind {
    StreamStart,
    StreamEnd,
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
    Scalar { style: ScalarStyle, empty: bool },
    Comment { text: String, placement: Placement },
    Other,
}

pub(super) fn scan(text: &str) -> Vec<(Span, Kind)> {
    Scanner::new(StrInput::new(text))
        .map_while(Result::ok)
        .map(|token| {
            let (span, kind) = token.into_parts();
            let kind = match kind {
                TokenType::StreamStart => Kind::StreamStart,
                TokenType::StreamEnd => Kind::StreamEnd,
                TokenType::BlockSequenceStart => Kind::BlockSequenceStart,
                TokenType::BlockMappingStart => Kind::BlockMappingStart,
                TokenType::BlockEnd => Kind::BlockEnd,
                TokenType::FlowSequenceStart => Kind::FlowSequenceStart,
                TokenType::FlowSequenceEnd => Kind::FlowSequenceEnd,
                TokenType::FlowMappingStart => Kind::FlowMappingStart,
                TokenType::FlowMappingEnd => Kind::FlowMappingEnd,
                TokenType::BlockEntry => Kind::BlockEntry,
                TokenType::Key => Kind::Key,
                TokenType::Value => Kind::Value,
                TokenType::Alias(_) => Kind::Alias,
                TokenType::Anchor(_) => Kind::Anchor,
                TokenType::Tag(..) => Kind::Tag,
                TokenType::Scalar(style, value) => Kind::Scalar {
                    style,
                    empty: value.is_empty(),
                },
                TokenType::Comment(comment) => Kind::Comment {
                    placement: comment.placement(),
                    text: comment.into_text().into_owned(),
                },
                _ => Kind::Other,
            };
            (span, kind)
        })
        .collect()
}
