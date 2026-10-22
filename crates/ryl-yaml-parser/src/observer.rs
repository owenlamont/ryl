use crate::{ScalarStyle, Span};

/// Lexical evidence borrowed for a token observer callback.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind<'token> {
    /// Beginning of the token stream.
    StreamStart,
    /// Successful scanner completion.
    StreamEnd,
    /// A `%YAML` directive.
    VersionDirective,
    /// A `%TAG` directive.
    TagDirective,
    /// An explicit `---` marker.
    DocumentStart,
    /// An explicit `...` marker.
    DocumentEnd,
    /// Beginning of a block sequence.
    BlockSequenceStart,
    /// Beginning of a block mapping.
    BlockMappingStart,
    /// End of a block collection.
    BlockEnd,
    /// An opening `[`.
    FlowSequenceStart,
    /// A closing `]`.
    FlowSequenceEnd,
    /// An opening `{`.
    FlowMappingStart,
    /// A closing `}`.
    FlowMappingEnd,
    /// A block sequence entry.
    BlockEntry,
    /// A flow collection separator.
    FlowEntry,
    /// A mapping key, including synthetic simple keys.
    Key,
    /// A mapping value indicator.
    Value,
    /// An alias reference.
    Alias,
    /// An anchor definition.
    Anchor,
    /// A node tag.
    Tag,
    /// A reserved directive.
    ReservedDirective,
    /// A scalar whose decoded value borrows scanner storage during the callback.
    Scalar {
        /// Source scalar style.
        style: ScalarStyle,
        /// Decoded scalar value.
        value: &'token str,
    },
    #[cfg(feature = "comments")]
    /// Raw comment payload, excluding the hash and line ending.
    Comment {
        /// Source text immediately after the hash.
        text: &'token str,
        /// Scanner placement hint before parser event ordering.
        placement: crate::Placement,
    },
}

/// Receives scanner emission order after synthetic-key insertion is settled.
pub trait TokenObserver {
    /// Called once when a settled token leaves the scanner queue.
    fn on_token(&mut self, span: &Span, kind: TokenKind<'_>);
}

impl TokenObserver for () {
    #[inline]
    fn on_token(&mut self, _: &Span, _: TokenKind<'_>) {}
}

impl<F: FnMut(&Span, TokenKind<'_>)> TokenObserver for F {
    fn on_token(&mut self, span: &Span, kind: TokenKind<'_>) {
        self(span, kind);
    }
}
