use ryl_yaml_parser::{Placement, ScalarStyle, Scanner, Span, StrInput, TokenType};

pub(crate) enum Kind {
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
        text: Box<str>,
        placement: Placement,
    },
    Other,
}

pub(crate) struct Tokens(Box<[(super::spans::StoredSpan, Kind)]>);

impl Tokens {
    pub(crate) fn iter(&self) -> impl Iterator<Item = (Span, &Kind)> {
        self.iter_from(0)
    }

    pub(crate) fn iter_from(
        &self,
        start: usize,
    ) -> impl Iterator<Item = (Span, &Kind)> {
        self.0[start..]
            .iter()
            .map(|(span, kind)| (span.get(), kind))
    }
}

pub(super) fn scan(text: &str) -> Tokens {
    Tokens(
        stream(text)
            .map(|(span, kind)| (super::spans::StoredSpan::new(span), kind))
            .collect(),
    )
}

pub(crate) fn stream(text: &str) -> impl Iterator<Item = (Span, Kind)> {
    Scanner::new(StrInput::new(text))
        .map_while(Result::ok)
        .map(|token| {
            let (span, kind) = token.into_parts();
            let kind = match kind {
                TokenType::StreamStart => Kind::StreamStart,
                TokenType::StreamEnd => Kind::StreamEnd,
                TokenType::DocumentStart => Kind::DocumentStart,
                TokenType::DocumentEnd => Kind::DocumentEnd,
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
                    text: comment.into_text().into_owned().into_boxed_str(),
                },
                _ => Kind::Other,
            };
            (span, kind)
        })
}
