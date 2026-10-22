//! Home to the YAML Scanner.
//!
//! The scanner is the lowest-level parsing utility. It is the lexer / tokenizer, reading input a
//! character at a time and emitting tokens that can later be interpreted by the [`crate::parser`]
//! to check for more context and validity.
//!
//! Due to the grammar of YAML, the scanner has to have some context and is not error-free.

#![allow(clippy::cast_possible_wrap)]
#![allow(clippy::cast_sign_loss)]

use core::char;
use std::{
    borrow::{Cow, ToOwned},
    collections::VecDeque,
    string::String,
    vec::Vec,
};

use crate::{
    Options,
    char_traits::{
        find_non_printable, is_anchor_char, is_blank_or_breakz, is_bom, is_break,
        is_breakz, is_flow, is_printable, is_tag_char, is_uri_char, is_yaml_non_space,
        is_z,
    },
    error::{ErrorKind, ScanError},
    input::{BorrowedInput, SkipTabs, WhitespaceResult},
};

/// The source style used for a YAML scalar.
#[derive(Clone, Copy, PartialEq, Debug, Eq, Hash, PartialOrd, Ord)]
pub enum ScalarStyle {
    /// A YAML plain scalar.
    Plain,
    /// A YAML single quoted scalar.
    SingleQuoted,
    /// A YAML double quoted scalar.
    DoubleQuoted,

    /// A YAML literal block (`|` block).
    ///
    /// See [8.1.2](https://yaml.org/spec/1.2.2/#812-literal-style).
    /// In literal blocks, any indented character is content, including white space characters.
    /// There is no way to escape characters, nor to break a long line.
    Literal,
    /// A YAML folded block (`>` block).
    ///
    /// See [8.1.3](https://yaml.org/spec/1.2.2/#813-folded-style).
    /// In folded blocks, any indented character is content, including white space characters.
    /// There is no way to escape characters. Content is subject to line folding, allowing breaking
    /// long lines.
    Folded,
}

/// Offset information for a [`Marker`].
///
/// YAML inputs can come from either a full `&str` (stable backing storage) or a streaming
/// character source. For stable inputs, we can track both a character index and a byte offset.
/// For streaming inputs, byte offsets are not generally useful (and may not correspond to any
/// meaningful underlying file/source), so they are optional.
#[derive(Clone, Copy, Debug, Default)]
struct MarkerOffsets {
    /// The index (in characters) in the source.
    chars: usize,
    /// The offset (in bytes) in the source, if available.
    bytes: Option<usize>,
}

impl PartialEq for MarkerOffsets {
    fn eq(&self, other: &Self) -> bool {
        // Byte offsets are an optional diagnostic enhancement and may differ between input
        // backends (e.g., `&str` vs streaming). Equality is therefore based on the character
        // position only.
        self.chars == other.chars
    }
}

impl Eq for MarkerOffsets {}

/// A location in a YAML document.
#[derive(Clone, Copy, PartialEq, Debug, Eq, Default)]
pub struct Marker {
    /// Offsets in the source.
    offsets: MarkerOffsets,
    /// The line (1-indexed).
    line: usize,
    /// The column (0-indexed).
    col: usize,
}

impl Marker {
    /// Create a new [`Marker`] at the given position.
    #[must_use]
    pub fn new(index: usize, line: usize, col: usize) -> Marker {
        Marker {
            offsets: MarkerOffsets {
                chars: index,
                bytes: None,
            },
            line,
            col,
        }
    }

    /// Return a copy of the marker with the given optional byte offset.
    #[must_use]
    pub fn with_byte_offset(mut self, byte_offset: Option<usize>) -> Marker {
        self.offsets.bytes = byte_offset;
        self
    }

    /// Return the index (in characters) of the marker in the source.
    #[must_use]
    pub fn index(&self) -> usize {
        self.offsets.chars
    }

    /// Return the byte offset of the marker in the source, if available.
    #[must_use]
    pub fn byte_offset(&self) -> Option<usize> {
        self.offsets.bytes
    }

    /// Return the line of the marker in the source.
    #[must_use]
    pub fn line(&self) -> usize {
        self.line
    }

    /// Return the column of the marker in the source.
    #[must_use]
    pub fn col(&self) -> usize {
        self.col
    }
}

/// A range of locations in a YAML document.
///
/// Construct spans with [`Self::new`], [`Self::empty`], or [`Self::default`], then use
/// [`Self::with_indent`] and [`Self::with_tag_start`] to attach optional metadata. Fields remain
/// readable and writable, but the type is non-exhaustive so future releases can add metadata.
/// Destructuring patterns must include `..`.
///
/// ```rust
/// use ryl_yaml_parser::{Marker, Span};
///
/// let start = Marker::new(0, 1, 0);
/// let end = Marker::new(3, 1, 3);
/// let span = Span::new(start, end)
///     .with_indent(Some(0))
///     .with_tag_start(Some(start));
/// let Span { start: actual_start, end: actual_end, .. } = span;
/// assert_eq!(actual_start, start);
/// assert_eq!(actual_end, end);
/// assert_eq!(span.indent, Some(0));
/// ```
///
/// Direct struct construction outside this crate is not supported:
///
/// ```compile_fail,E0639
/// use ryl_yaml_parser::{Marker, Span};
///
/// let marker = Marker::new(0, 1, 0);
/// let span = Span { start: marker, end: marker, indent: None, tag_start: None };
/// ```
///
/// Struct-update syntax is likewise unavailable; use the builders or assign individual fields:
///
/// ```compile_fail,E0639
/// use ryl_yaml_parser::{Marker, Span};
///
/// let span = Span { indent: Some(2), ..Span::empty(Marker::new(0, 1, 0)) };
/// ```
#[non_exhaustive]
#[derive(Clone, Copy, PartialEq, Debug, Eq, Default)]
pub struct Span {
    /// The start (inclusive) of the range.
    pub start: Marker,
    /// The end (exclusive) of the range.
    pub end: Marker,

    /// Optional indentation hint associated with this span.
    ///
    /// This is only meaningful for certain parser-emitted events (notably: block mapping keys).
    /// When indentation is not meaningful or cannot be provided, it must be `None`.
    pub indent: Option<usize>,

    /// Optional source marker for the explicit tag token attached to this node.
    ///
    /// This is only meaningful for parser-emitted node events that carry a resolved tag, such as
    /// [`Event::Scalar`](crate::Event::Scalar),
    /// [`Event::SequenceStart`](crate::Event::SequenceStart), or
    /// [`Event::MappingStart`](crate::Event::MappingStart). The normal [`Span::start`] and
    /// [`Span::end`] continue to cover the node value or collection; `tag_start` points to the
    /// tag token when that token appears at a different source location.
    pub tag_start: Option<Marker>,
}

impl Span {
    /// Create a new [`Span`] for the given range.
    #[must_use]
    pub fn new(start: Marker, end: Marker) -> Span {
        Span {
            start,
            end,
            indent: None,
            tag_start: None,
        }
    }

    /// Create an empty [`Span`] at a given location.
    ///
    /// An empty span doesn't contain any characters, but its position may still be meaningful.
    /// For example, for an indented sequence [`SequenceEnd`] has a location but an empty span.
    ///
    /// [`SequenceEnd`]: crate::Event::SequenceEnd
    #[must_use]
    pub fn empty(mark: Marker) -> Span {
        Span {
            start: mark,
            end: mark,
            indent: None,
            tag_start: None,
        }
    }

    /// Return a copy of this [`Span`] with the given indentation hint.
    #[must_use]
    pub fn with_indent(mut self, indent: Option<usize>) -> Span {
        self.indent = indent;
        self
    }

    /// Return a copy of this [`Span`] with the given explicit tag-token start marker.
    #[must_use]
    pub fn with_tag_start(mut self, tag_start: Option<Marker>) -> Span {
        self.tag_start = tag_start;
        self
    }

    /// Return the source marker of the explicit tag token attached to this node, if any.
    ///
    /// The regular span still covers the node value or collection. This accessor is useful for
    /// diagnostics that should point at the tag itself, especially when a tagged block collection
    /// begins on a later line than the tag token.
    #[must_use]
    pub fn tag_start(&self) -> Option<Marker> {
        self.tag_start
    }

    /// Return the length of the span (in characters).
    ///
    /// # Panics
    /// Panics in debug builds if the end marker precedes the start marker.
    #[track_caller]
    #[must_use]
    pub fn len(&self) -> usize {
        self.end.index() - self.start.index()
    }

    /// Return whether the [`Span`] has a length of zero.
    ///
    /// # Panics
    /// Panics in debug builds if the end marker precedes the start marker.
    #[track_caller]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Return the byte range of the span, if available.
    #[must_use]
    pub fn byte_range(&self) -> Option<core::ops::Range<usize>> {
        let start = self.start.byte_offset()?;
        let end = self.end.byte_offset()?;
        Some(start..end)
    }

    /// Return the source text covered by this span, if byte offsets are available
    /// and the range is valid for the provided input.
    #[must_use]
    pub fn slice<'source>(&self, source: &'source str) -> Option<&'source str> {
        source.get(self.byte_range()?)
    }
}

/// A positional hint for a YAML source comment.
///
/// The parser currently recognizes these placements:
///
/// ```yaml
/// # Above
/// key: value # Right
///
/// # Free
///
/// next: value
///
/// # Last
/// ```
#[non_exhaustive]
#[derive(Clone, Copy, PartialEq, Debug, Eq, Default)]
pub enum Placement {
    /// An own-line comment immediately before another YAML token.
    ///
    /// This usually means the comment visually describes the following node.
    /// Consecutive own-line comments without blank lines between them are also considered
    /// `Above`, so a comment block can attach to the next YAML element as a group.
    Above,
    /// A same-line comment after YAML content or syntax. Examples include `key: value # Right`
    /// and `- # Right` for an empty sequence entry.
    Right,
    /// A standalone own-line comment that is separated from nearby YAML tokens.
    ///
    /// This is the fallback for comments that are neither same-line comments, immediately above a
    /// following token, nor the final comment in the stream. Consumers should treat `Free` as not
    /// having an obvious neighboring node.
    #[default]
    Free,
    /// An own-line comment at the end of the input stream.
    ///
    /// A `Last` comment may be followed by blank lines, but no further YAML token appears before
    /// `StreamEnd`.
    Last,
}

/// YAML comment metadata captured from the source.
///
/// Comments are presentation metadata, not YAML data. This type carries the raw comment payload and
/// a best-effort [`Placement`] hint. The companion [`Token`] carries the comment's source span.
#[derive(Clone, PartialEq, Debug, Eq)]
pub struct Comment<'input> {
    /// Raw comment payload exactly after `#`, excluding only the line break.
    ///
    /// Leading spaces are preserved, including a single space immediately after `#` when present.
    text: Cow<'input, str>,
    /// Best-effort placement of this comment relative to nearby YAML content.
    placement: Placement,
}

impl<'input> Comment<'input> {
    /// Create captured YAML comment metadata from a raw payload.
    ///
    /// The placement defaults to [`Placement::Free`]. Use [`Comment::with_placement`] when the
    /// caller already knows a more specific placement.
    #[must_use]
    pub fn new(text: impl Into<Cow<'input, str>>) -> Self {
        Self {
            text: text.into(),
            placement: Placement::Free,
        }
    }

    /// Return this comment with the given placement.
    #[must_use]
    pub fn with_placement(mut self, placement: Placement) -> Self {
        self.placement = placement;
        self
    }

    /// Return the raw comment payload exactly after `#`, excluding only the line break.
    #[must_use]
    pub fn text(&self) -> &str {
        self.text.as_ref()
    }

    /// Return the best-effort placement of this comment relative to nearby YAML content.
    #[must_use]
    pub const fn placement(&self) -> Placement {
        self.placement
    }

    /// Consume the comment and return its raw payload without cloning.
    #[must_use]
    pub fn into_text(self) -> Cow<'input, str> {
        self.text
    }

    /// Return the comment payload with surrounding whitespace removed.
    ///
    /// This helper is ergonomic only. The raw [`Self::text`] payload remains unchanged.
    #[must_use]
    pub fn trimmed_text(&self) -> &str {
        self.text.trim()
    }
}

impl AsRef<str> for Comment<'_> {
    fn as_ref(&self) -> &str {
        self.text.as_ref()
    }
}

/// The contents of a scanner token.
#[non_exhaustive]
#[derive(Clone, PartialEq, Debug, Eq)]
pub enum TokenType<'input> {
    /// The start of the stream. Sent first, before even [`TokenType::DocumentStart`].
    StreamStart,
    /// The end of the stream, EOF.
    StreamEnd,
    /// A YAML version directive.
    VersionDirective(
        /// Major version number.
        u32,
        /// Minor version number.
        u32,
    ),
    /// A YAML tag directive (e.g.: `!!str`, `!foo!bar`, ...).
    TagDirective(
        /// Tag directive handle, such as `!` or `!app!`.
        Cow<'input, str>,
        /// Tag URI prefix associated with the handle.
        Cow<'input, str>,
    ),
    /// The start of a YAML document (`---`).
    DocumentStart,
    /// The end of a YAML document (`...`).
    DocumentEnd,
    /// The start of a sequence block.
    ///
    /// Sequence blocks are arrays starting with a `-`.
    BlockSequenceStart,
    /// The start of a block mapping.
    ///
    /// Block mappings are key-value collections written with `key: value` entries.
    BlockMappingStart,
    /// End of the corresponding `BlockSequenceStart` or `BlockMappingStart`.
    BlockEnd,
    /// Start of an inline sequence (`[ a, b ]`).
    FlowSequenceStart,
    /// End of an inline sequence.
    FlowSequenceEnd,
    /// Start of an inline mapping (`{ a: b, c: d }`).
    FlowMappingStart,
    /// End of an inline mapping.
    FlowMappingEnd,
    /// An entry in a block sequence (see [`TokenType::BlockSequenceStart`]).
    BlockEntry,
    /// An entry in a flow sequence (see [`TokenType::FlowSequenceStart`]).
    FlowEntry,
    /// A key in a mapping.
    Key,
    /// A value in a mapping.
    Value,
    /// A reference to a previously defined anchor.
    Alias(Cow<'input, str>),
    /// A YAML anchor definition introduced by `&`.
    Anchor(Cow<'input, str>),
    /// A YAML tag (starting with bangs `!`).
    Tag(
        /// The handle of the tag.
        Cow<'input, str>,
        /// The suffix of the tag.
        Cow<'input, str>,
    ),
    /// A regular YAML scalar.
    Scalar(ScalarStyle, Cow<'input, str>),
    /// A YAML source comment.
    ///
    /// The token payload carries the raw text exactly after `#` and an initial [`Placement`] hint.
    /// The companion [`Token`] span covers the whole source comment, including `#` and excluding the
    /// line break.
    Comment(
        /// Captured comment metadata.
        Comment<'input>,
    ),
    /// A reserved YAML directive.
    ReservedDirective(
        /// Directive name.
        String,
        /// Directive parameters, split on YAML whitespace.
        Vec<String>,
    ),
}

/// A scanner token.
#[derive(Clone, PartialEq, Debug, Eq)]
pub struct Token<'input>(Span, TokenType<'input>);

impl<'input> Token<'input> {
    /// Create a scanner token from its source span and payload.
    #[must_use]
    pub const fn new(span: Span, token_type: TokenType<'input>) -> Self {
        Self(span, token_type)
    }

    /// Return the source span covered by this token.
    #[must_use]
    pub const fn span(&self) -> Span {
        self.0
    }

    /// Return the payload emitted by the scanner.
    #[must_use]
    pub const fn token_type(&self) -> &TokenType<'input> {
        &self.1
    }

    /// Consume the token and return its source span and payload.
    #[must_use]
    pub fn into_parts(self) -> (Span, TokenType<'input>) {
        (self.0, self.1)
    }
}

/// Token payload used in the scanner's internal queue.
///
/// Public [`Token`] values are reconstructed when the scanner emits them.
#[derive(Clone, PartialEq, Debug, Eq)]
pub(crate) enum QueuedTokenType<'input> {
    StreamStart,
    StreamEnd,
    VersionDirective(u32, u32),
    TagDirective(Cow<'input, str>, Cow<'input, str>),
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
    FlowEntry,
    Key,
    Value,
    Alias(Cow<'input, str>),
    Anchor(Cow<'input, str>),
    Tag(Cow<'input, str>, Cow<'input, str>),
    Scalar(ScalarStyle, Cow<'input, str>),
    Comment(Comment<'input>),
    ReservedDirective(String, Vec<String>),
}

impl<'input> QueuedTokenType<'input> {
    #[inline]
    pub(crate) fn kind(&self) -> crate::TokenKind<'_> {
        match self {
            Self::StreamStart => crate::TokenKind::StreamStart,
            Self::StreamEnd => crate::TokenKind::StreamEnd,
            Self::VersionDirective(..) => crate::TokenKind::VersionDirective,
            Self::TagDirective(..) => crate::TokenKind::TagDirective,
            Self::DocumentStart => crate::TokenKind::DocumentStart,
            Self::DocumentEnd => crate::TokenKind::DocumentEnd,
            Self::BlockSequenceStart => crate::TokenKind::BlockSequenceStart,
            Self::BlockMappingStart => crate::TokenKind::BlockMappingStart,
            Self::BlockEnd => crate::TokenKind::BlockEnd,
            Self::FlowSequenceStart => crate::TokenKind::FlowSequenceStart,
            Self::FlowSequenceEnd => crate::TokenKind::FlowSequenceEnd,
            Self::FlowMappingStart => crate::TokenKind::FlowMappingStart,
            Self::FlowMappingEnd => crate::TokenKind::FlowMappingEnd,
            Self::BlockEntry => crate::TokenKind::BlockEntry,
            Self::FlowEntry => crate::TokenKind::FlowEntry,
            Self::Key => crate::TokenKind::Key,
            Self::Value => crate::TokenKind::Value,
            Self::Alias(..) => crate::TokenKind::Alias,
            Self::Anchor(..) => crate::TokenKind::Anchor,
            Self::Tag(..) => crate::TokenKind::Tag,
            Self::ReservedDirective(..) => crate::TokenKind::ReservedDirective,
            Self::Scalar(style, value) => crate::TokenKind::Scalar {
                style: *style,
                value: value.as_ref(),
            },
            Self::Comment(comment) => crate::TokenKind::Comment {
                text: comment.text(),
                placement: comment.placement(),
            },
        }
    }

    fn into_public(self) -> TokenType<'input> {
        match self {
            Self::StreamStart => TokenType::StreamStart,
            Self::StreamEnd => TokenType::StreamEnd,
            Self::VersionDirective(major, minor) => {
                TokenType::VersionDirective(major, minor)
            }
            Self::TagDirective(handle, prefix) => {
                TokenType::TagDirective(handle, prefix)
            }
            Self::DocumentStart => TokenType::DocumentStart,
            Self::DocumentEnd => TokenType::DocumentEnd,
            Self::BlockSequenceStart => TokenType::BlockSequenceStart,
            Self::BlockMappingStart => TokenType::BlockMappingStart,
            Self::BlockEnd => TokenType::BlockEnd,
            Self::FlowSequenceStart => TokenType::FlowSequenceStart,
            Self::FlowSequenceEnd => TokenType::FlowSequenceEnd,
            Self::FlowMappingStart => TokenType::FlowMappingStart,
            Self::FlowMappingEnd => TokenType::FlowMappingEnd,
            Self::BlockEntry => TokenType::BlockEntry,
            Self::FlowEntry => TokenType::FlowEntry,
            Self::Key => TokenType::Key,
            Self::Value => TokenType::Value,
            Self::Alias(name) => TokenType::Alias(name),
            Self::Anchor(name) => TokenType::Anchor(name),
            Self::Tag(handle, suffix) => TokenType::Tag(handle, suffix),
            Self::Scalar(style, value) => TokenType::Scalar(style, value),
            Self::Comment(comment) => TokenType::Comment(comment),
            Self::ReservedDirective(name, params) => {
                TokenType::ReservedDirective(name, params)
            }
        }
    }
}

impl<'input> From<TokenType<'input>> for QueuedTokenType<'input> {
    fn from(token: TokenType<'input>) -> Self {
        match token {
            TokenType::StreamStart => Self::StreamStart,
            TokenType::StreamEnd => Self::StreamEnd,
            TokenType::VersionDirective(major, minor) => {
                Self::VersionDirective(major, minor)
            }
            TokenType::TagDirective(handle, prefix) => {
                Self::TagDirective(handle, prefix)
            }
            TokenType::DocumentStart => Self::DocumentStart,
            TokenType::DocumentEnd => Self::DocumentEnd,
            TokenType::BlockSequenceStart => Self::BlockSequenceStart,
            TokenType::BlockMappingStart => Self::BlockMappingStart,
            TokenType::BlockEnd => Self::BlockEnd,
            TokenType::FlowSequenceStart => Self::FlowSequenceStart,
            TokenType::FlowSequenceEnd => Self::FlowSequenceEnd,
            TokenType::FlowMappingStart => Self::FlowMappingStart,
            TokenType::FlowMappingEnd => Self::FlowMappingEnd,
            TokenType::BlockEntry => Self::BlockEntry,
            TokenType::FlowEntry => Self::FlowEntry,
            TokenType::Key => Self::Key,
            TokenType::Value => Self::Value,
            TokenType::Alias(name) => Self::Alias(name),
            TokenType::Anchor(name) => Self::Anchor(name),
            TokenType::Tag(handle, suffix) => Self::Tag(handle, suffix),
            TokenType::Scalar(style, value) => Self::Scalar(style, value),
            TokenType::Comment(comment) => Self::Comment(comment),
            TokenType::ReservedDirective(name, params) => {
                Self::ReservedDirective(name, params)
            }
        }
    }
}

/// A compact token stored by the scanner before it is emitted publicly.
#[derive(Clone, PartialEq, Debug, Eq)]
pub(crate) struct QueuedToken<'input>(
    pub(crate) Span,
    pub(crate) QueuedTokenType<'input>,
);

impl<'input> QueuedToken<'input> {
    fn into_public(self) -> Token<'input> {
        Token(self.0, self.1.into_public())
    }
}

impl<'input> From<Token<'input>> for QueuedToken<'input> {
    fn from(token: Token<'input>) -> Self {
        Self(token.0, token.1.into())
    }
}

/// A scalar that was parsed and may correspond to a simple key.
///
/// Upon scanning the following YAML:
/// ```yaml
/// a: b
/// ```
/// We do not know that `a` is a key for a map until we have reached the following `:`. For this
/// YAML, we would store `a` as a scalar token in the [`Scanner`], but not emit it yet. It would be
/// kept inside the scanner until more context is fetched and we are able to know whether it is a
/// plain scalar or a key.
///
/// For example, see the following two YAML documents:
/// ```yaml
/// ---
/// a: b # Here, `a` is a key.
/// ...
/// ---
/// a # Here, `a` is a plain scalar.
/// ...
/// ```
/// An instance of [`SimpleKey`] is created in the [`Scanner`] when such ambiguity occurs.
///
/// In both documents, scanning `a` would lead to the creation of a [`SimpleKey`] with
/// [`Self::possible`] set to `true`. The token for `a` would be pushed in the [`Scanner`] but not
/// yet emitted. Instead, more context would be fetched (through [`Scanner::fetch_more_tokens`]).
///
/// In the first document, upon reaching the `:`, the [`SimpleKey`] would be inspected and our
/// scalar `a` since it is a possible key, would be "turned" into a key. This is done by prepending
/// a [`TokenType::Key`] to our scalar token in the [`Scanner`]. This way, the
/// [`crate::parser::Parser`] would read the [`TokenType::Key`] token before the
/// [`TokenType::Scalar`] token.
///
/// In the second document however, reaching EOF would mark the [`SimpleKey`] as no longer possible,
/// and no [`TokenType::Key`] would be emitted by the scanner.
#[derive(Clone, Copy, PartialEq, Debug, Eq)]
struct SimpleKey {
    /// Whether the token this [`SimpleKey`] refers to may still be a key.
    ///
    /// Sometimes, when we have more context, we notice that what we thought could be a key no
    /// longer can be. In that case, [`Self::possible`] is set to `false`.
    ///
    /// For instance, let us consider the following invalid YAML:
    /// ```yaml
    /// key
    ///   : value
    /// ```
    /// Upon reading the `\n` after `key`, the [`SimpleKey`] that was created for `key` is no longer
    /// possible and [`Self::possible`] is set to `false`.
    possible: bool,
    /// Whether the token this [`SimpleKey`] refers to is required to be a key.
    ///
    /// With more context, we may know for sure that the token must be a key. If later input makes
    /// that impossible, the scanner must report an error instead of silently treating the token as a
    /// plain scalar.
    ///
    /// This happens for simple keys at the current block indentation where the surrounding
    /// collection requires the next token to be a mapping key.
    required: bool,
    /// The index of the token referred to by the [`SimpleKey`].
    ///
    /// This is the index in the scanner, which takes into account both the tokens that have been
    /// emitted and those about to be emitted. See [`Scanner::tokens_parsed`] and
    /// [`Scanner::tokens`] for more details.
    token_number: usize,
    /// The position at which the token the [`SimpleKey`] refers to is.
    mark: Marker,
}

impl SimpleKey {
    /// Create a new [`SimpleKey`] at the given `Marker` and with the given flow level.
    fn new(mark: Marker) -> SimpleKey {
        SimpleKey {
            possible: false,
            required: false,
            token_number: 0,
            mark,
        }
    }
}

/// An indentation level on the stack of indentations.
#[derive(Clone, Debug, Default)]
struct Indent {
    /// The former indentation level.
    indent: isize,
    /// Whether, upon closing, this indents generates a `BlockEnd` token.
    ///
    /// There are levels of indentation which do not start a block. Examples of this would be:
    /// ```yaml
    /// -
    ///   foo # ok
    /// -
    /// bar # ko, bar needs to be indented further than the `-`.
    /// - [
    ///  baz, # ok
    /// quux # accepted by default; invalid when `Options::strict_indentation` is true.
    /// ] # accepted by default; invalid when `Options::strict_indentation` is true.
    /// ```
    ///
    /// The indentation level created by the `-` is for a single entry in the sequence. Emitting a
    /// `BlockEnd` when this indentation block ends would generate one `BlockEnd` per entry in the
    /// sequence, although we must have exactly one to end the sequence.
    needs_block_end: bool,
}

/// The knowledge we have about an implicit mapping.
///
/// Implicit mappings occur in flow sequences where the opening `{` for a mapping in a flow
/// sequence is omitted:
/// ```yaml
/// [ a: b, c: d ]
/// # Equivalent to
/// [ { a: b }, { c: d } ]
/// # Equivalent to
/// - a: b
/// - c: d
/// ```
///
/// The state must be carefully tracked for each nested flow sequence since we must emit a
/// [`FlowMappingStart`] event when encountering `a` and `c` in our previous example without a
/// character hinting us. Similarly, we must emit a [`FlowMappingEnd`] event when we reach the `,`
/// or the `]`. If the state is not properly tracked, we may omit to emit these events or emit them
/// out-of-order.
///
/// [`FlowMappingStart`]: TokenType::FlowMappingStart
/// [`FlowMappingEnd`]: TokenType::FlowMappingEnd
#[derive(Debug, PartialEq)]
enum ImplicitMappingState {
    /// It is possible there is an implicit mapping.
    ///
    /// This state is the one when we have just encountered the opening `[`. We need more context
    /// to know whether an implicit mapping follows.
    Possible,
    /// We are inside the implicit mapping.
    ///
    /// Note that this state is not set immediately (we need to have encountered the `:` to know).
    Inside(usize),
}

/// The YAML scanner.
///
/// This corresponds to the low-level interface when reading YAML. The scanner emits tokens as they
/// are read (akin to a lexer), but it also holds sufficient context to be able to disambiguate
/// some of the constructs. It has understanding of indentation and whitespace and is able to
/// generate error messages for some invalid YAML constructs.
///
/// It is however not a full parser and needs [`crate::parser::Parser`] to fully detect invalid
/// YAML documents.
///
/// `Scanner` is a fallible iterator over [`Token`] values. A scanning failure is emitted as one
/// [`Err`] item, after which the iterator is exhausted.
#[derive(Debug)]
#[allow(clippy::struct_excessive_bools)]
pub struct Scanner<'input, T> {
    /// The input source.
    ///
    /// This must implement [`Input`].
    input: T,
    /// Resource limits used while scanning.
    options: Options,
    /// The position of the cursor within the reader.
    mark: Marker,
    /// Buffer for tokens to be returned.
    ///
    /// This buffer can hold some temporary tokens that are not yet ready to be returned. For
    /// instance, if we just read a scalar, it can be a value or a key if an implicit mapping
    /// follows. In this case, the token stays in the `VecDeque` but cannot be returned from
    /// [`Self::next`] until we have more context.
    tokens: VecDeque<QueuedToken<'input>>,
    /// Whether a terminal error has been emitted by the iterator.
    failed: bool,
    /// Error found after one or more already-scanned comment tokens.
    deferred_error: Option<ScanError>,
    /// Whether the input may contain `#` comment indicators.
    ///
    /// This remains a source-content hint when comment emission is disabled. In that mode it is
    /// conservatively set to `true`, because the `false` fast path may only be used when the
    /// source is known not to contain comments.
    comments_possible: bool,

    /// Whether we have already emitted the `StreamStart` token.
    stream_start_produced: bool,
    /// Whether we have already emitted the `StreamEnd` token.
    stream_end_produced: bool,
    /// Whether the scanner is still in the prefix of the next document.
    ///
    /// A BOM may appear in a document prefix, before directives/comments/content. Once a document
    /// start marker or any content token is scanned, another BOM is document content and must be
    /// rejected unless it appears inside a quoted scalar.
    document_prefix_allowed: bool,
    /// In some flow contexts, the value of a mapping is allowed to be adjacent to the `:`. When it
    /// is, the index at which the `:` may be must be stored in `adjacent_value_allowed_at`.
    adjacent_value_allowed_at: usize,
    /// Whether a simple key could potentially start at the current position.
    ///
    /// Simple keys are the opposite of complex keys which are keys starting with `?`.
    simple_key_allowed: bool,
    /// A stack of potential simple keys.
    ///
    /// Refer to the documentation of [`SimpleKey`] for a more in-depth explanation of what they
    /// are.
    simple_keys: smallvec::SmallVec<[SimpleKey; 8]>,
    /// Index of the oldest still-possible simple key, or `None` if there are none.
    ///
    /// Only the current flow level can save a key, so possible keys are ordered by source
    /// position and token number. Keeping their first index avoids rescanning inactive levels
    /// and lets expiry stop as soon as the oldest remaining candidate is still valid.
    first_simple_key: Option<usize>,
    /// The current indentation level.
    indent: isize,
    /// List of all block indentation levels we are in (except the current one).
    indents: smallvec::SmallVec<[Indent; 8]>,
    /// Level of nesting of flow collections.
    flow_level: usize,
    /// Number of open block collections that will each generate a `BlockEnd` token.
    block_level: usize,
    /// The number of tokens that have been returned from the scanner.
    ///
    /// This excludes the tokens from [`Self::tokens`].
    tokens_parsed: usize,
    /// Whether a token is ready to be taken from [`Self::tokens`].
    token_available: bool,
    /// Whether all characters encountered since the last newline were whitespace.
    leading_whitespace: bool,
    /// Whether we started a flow mapping at each flow nesting level.
    ///
    /// This is used to detect implicit flow mapping starts such as:
    /// ```yaml
    /// [ : foo ] # { null: "foo" }
    /// ```
    flow_mapping_started: smallvec::SmallVec<[bool; 8]>,
    /// An array of states, representing whether flow sequences have implicit mappings.
    ///
    /// When a flow mapping is possible (when encountering the first `[` or a `,` in a sequence),
    /// the state is set to [`Possible`].
    /// When we encounter the `:`, we know we are in an implicit mapping and can set the state to
    /// [`Inside`].
    ///
    /// There is one entry in this [`Vec`] for each nested flow sequence that we are in.
    /// The entries are created with the opening `[` and popped with the closing `]`.
    ///
    /// [`Possible`]: ImplicitMappingState::Possible
    /// [`Inside`]: ImplicitMappingState::Inside
    implicit_flow_mapping_states: smallvec::SmallVec<[ImplicitMappingState; 8]>,
    /// If a plain scalar was terminated by a `#` comment on its line, we set this
    /// to detect an illegal multiline continuation on the following line.
    interrupted_plain_by_comment: Option<Marker>,
    /// Whether the scanner is still validating whitespace after an explicit `?` key indicator.
    ///
    /// This stays set across streamed comments, and across node properties in strict mode, to
    /// distinguish tab separation from the space-only indentation required by a key node or
    /// compact block collection.
    explicit_key_tab_check_pending: bool,
    /// A stack of markers for opening brackets `[` and `{`.
    flow_markers: smallvec::SmallVec<[(Marker, char); 8]>,
    buf_leading_break: String,
    buf_trailing_breaks: String,
    buf_whitespaces: String,
}

impl<'input, T: BorrowedInput<'input>> Iterator for Scanner<'input, T> {
    type Item = Result<Token<'input>, ScanError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        match self.next_token() {
            Ok(Some(tok)) => Some(Ok(tok)),
            Ok(None) => None,
            Err(error) => {
                self.failed = true;
                Some(Err(error))
            }
        }
    }
}

impl<'input, T: BorrowedInput<'input>> core::iter::FusedIterator
    for Scanner<'input, T>
{
}

/// A convenience alias for scanner functions that may fail without returning a value.
type ScanResult = Result<(), ScanError>;

/// Result of skipping inter-token whitespace and comments.
///
/// These facts are intentionally distinct: an ignored comment still changes some scanner
/// decisions even though it did not produce a token that can be returned to the caller.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct SkipToNextTokenOutcome {
    saw_comment: bool,
    queued_comment: bool,
}

/// A flow scalar's contents: a borrowed source range until a transformation forces a copy.
#[derive(Debug, Default)]
struct FlowScalarBuf {
    /// The contents once they can no longer be borrowed from the source.
    owned: Option<String>,
    /// `start..end` is the committed verbatim range.
    start: usize,
    end: usize,
    /// `pending_ws_start..pending_ws_end` is a run of blanks that were seen but not yet
    /// committed (they must be dropped if followed by a line break).
    pending_ws_start: Option<usize>,
    pending_ws_end: usize,
}

impl FlowScalarBuf {
    #[inline]
    fn new_borrowed(start: usize) -> Self {
        Self {
            owned: None,
            start,
            end: start,
            pending_ws_start: None,
            pending_ws_end: start,
        }
    }

    #[inline]
    fn new_owned() -> Self {
        Self {
            owned: Some(String::new()),
            ..Self::default()
        }
    }

    #[inline]
    fn commit_pending_ws(&mut self) {
        if self.pending_ws_start.take().is_some() {
            self.end = self.pending_ws_end;
        }
    }

    #[inline]
    fn note_pending_ws(&mut self, ws_start: usize, ws_end: usize) {
        self.pending_ws_start.get_or_insert(ws_start);
        self.pending_ws_end = ws_end;
    }

    #[inline]
    fn discard_pending_ws(&mut self) {
        self.pending_ws_start = None;
        self.pending_ws_end = self.end;
    }
}

impl<'input, T: BorrowedInput<'input>> Scanner<'input, T> {
    #[inline]
    fn promote_flow_scalar_buf_to_owned<'b>(
        &self,
        start_mark: &Marker,
        buf: &'b mut FlowScalarBuf,
    ) -> Result<&'b mut String, ScanError> {
        if buf.owned.is_none() {
            let slice =
                self.input.slice_bytes(buf.start, buf.end).ok_or_else(|| {
                    ScanError::from_kind(
                        *start_mark,
                        ErrorKind::InputOffsetsWithoutSlice,
                    )
                })?;
            buf.owned = Some(slice.to_owned());
            buf.pending_ws_start = None;
        }
        Ok(buf
            .owned
            .as_mut()
            .expect("a borrowed buffer was just replaced by an owned one"))
    }

    /// The column a tab must reach to be separation rather than indentation.
    fn tab_indent_limit(&self) -> isize {
        if self.options.strict_indentation && self.flow_level > 0 {
            self.flow_block_indent() + 1
        } else {
            self.indent
        }
    }

    /// Consume the blanks and line breaks after a flow scalar's content run.
    ///
    /// Trailing blanks are appended immediately and truncated if a line break follows. The
    /// first break of a run is consumed into `break_scratch` and discarded; later breaks are
    /// appended. Returns whether a break was folded and whether later breaks were preserved.
    fn consume_flow_scalar_whitespace(
        &mut self,
        buf: &mut FlowScalarBuf,
        leading_blanks: &mut bool,
        break_scratch: &mut String,
        start_mark: &Marker,
    ) -> Result<(bool, bool), ScanError> {
        let mut trailing_ws_start = None;
        let mut pending_ws_start = None;
        let mut has_leading_break = false;
        let mut has_trailing_breaks = false;
        while self.input.next_is_blank() || self.input.next_is_break() {
            if self.input.next_is_blank() && *leading_blanks {
                if self.input.peek() == '\t'
                    && (self.mark.col as isize) < self.tab_indent_limit()
                {
                    return Err(self.scan_error(ErrorKind::TabInIndentation));
                }
                self.skip_blank();
            } else if self.input.next_is_blank() {
                self.push_flow_scalar_blank(
                    buf,
                    &mut trailing_ws_start,
                    &mut pending_ws_start,
                );
            } else if *leading_blanks {
                self.input.lookahead(2);
                let string = self.promote_flow_scalar_buf_to_owned(start_mark, buf)?;
                self.read_break(string);
                has_trailing_breaks = true;
            } else {
                self.input.lookahead(2);
                if let Some(pos) = trailing_ws_start.take()
                    && let Some(string) = &mut buf.owned
                {
                    string.truncate(pos);
                }
                // Trailing blanks before a break are discarded, so the scalar is transformed.
                if pending_ws_start.take().is_some() {
                    self.promote_flow_scalar_buf_to_owned(start_mark, buf)?;
                    buf.discard_pending_ws();
                } else {
                    buf.commit_pending_ws();
                }
                break_scratch.clear();
                self.read_break(break_scratch);
                has_leading_break = true;
                *leading_blanks = true;
            }
            self.input.lookahead(1);
        }
        Ok((has_leading_break, has_trailing_breaks))
    }

    /// Record a trailing blank: append it to an owned buffer, or extend a borrowed buffer's
    /// pending whitespace run.
    fn push_flow_scalar_blank(
        &mut self,
        buf: &mut FlowScalarBuf,
        trailing_ws_start: &mut Option<usize>,
        pending_ws_start: &mut Option<usize>,
    ) {
        match &mut buf.owned {
            Some(string) => {
                trailing_ws_start.get_or_insert(string.len());
                string.push(self.input.peek());
            }
            None => {
                if pending_ws_start.is_none() {
                    *pending_ws_start = self.input.byte_offset();
                }
            }
        }
        self.skip_blank();
        if let (Some(ws_start), Some(ws_end)) =
            (*pending_ws_start, self.input.byte_offset())
        {
            buf.note_pending_ws(ws_start, ws_end);
        }
    }
    /// Try to borrow a slice from the underlying input.
    ///
    /// This method uses the [`BorrowedInput`] trait to safely obtain a slice with the `'input`
    /// lifetime. For inputs that support zero-copy slicing (like `StrInput`), this returns
    /// `Some(&'input str)`. For streaming inputs, this returns `None`.
    #[inline]
    fn try_borrow_slice(&self, start: usize, end: usize) -> Option<&'input str> {
        self.input.slice_borrowed(start, end)
    }

    /// Scan a tag handle for a `%TAG` directive as a `Cow<str>`.
    ///
    /// For `StrInput`, this will borrow from the input when possible. For other inputs, or if
    /// borrowing is not possible, it falls back to allocating.
    fn scan_tag_handle_directive_cow(
        &mut self,
        mark: &Marker,
        budget: &mut usize,
    ) -> Result<Cow<'input, str>, ScanError> {
        let Some(start) = self.input.byte_offset() else {
            return Ok(Cow::Owned(
                self.scan_tag_handle_directive_owned(mark, budget)?,
            ));
        };

        if self.input.look_ch() != '!' {
            return Err(ScanError::from_kind(*mark, ErrorKind::ExpectedTagBang));
        }

        // Consume the leading '!'.
        self.spend_directive_bytes(budget, 1, *mark)?;
        self.skip_non_blank();

        // Consume ns-word-char (ASCII alphanumeric, '_' or '-') characters.
        // This mirrors `StrInput::fetch_while_is_alpha` but avoids allocation.
        self.input.lookahead(1);
        while self.input.next_is_alpha() {
            self.spend_directive_bytes(budget, 1, *mark)?;
            self.skip_non_blank();
            self.input.lookahead(1);
        }

        // Optional trailing '!'.
        if self.input.peek() == '!' {
            self.spend_directive_bytes(budget, 1, *mark)?;
            self.skip_non_blank();
        }

        let handle = self.source_since(start, mark)?;
        if !handle.ends_with('!') {
            return Err(ScanError::from_kind(
                *mark,
                ErrorKind::ExpectedTagDirectiveBang,
            ));
        }
        Ok(handle)
    }

    /// The source text from byte `start` to the cursor, borrowed when the input allows it.
    fn source_since(
        &self,
        start: usize,
        mark: &Marker,
    ) -> Result<Cow<'input, str>, ScanError> {
        let end = self.input.byte_offset();
        if let Some(slice) = end.and_then(|end| self.try_borrow_slice(start, end)) {
            return Ok(Cow::Borrowed(slice));
        }
        end.and_then(|end| self.input.slice_bytes(start, end))
            .map(|slice| Cow::Owned(slice.to_owned()))
            .ok_or_else(|| {
                ScanError::from_kind(*mark, ErrorKind::InputSlicingUnavailable)
            })
    }

    /// Owned `%TAG`-handle scanner for streaming inputs, with allocation bounded by `budget`.
    fn scan_tag_handle_directive_owned(
        &mut self,
        mark: &Marker,
        budget: &mut usize,
    ) -> Result<String, ScanError> {
        let mut string = String::new();
        if self.input.look_ch() != '!' {
            return Err(ScanError::from_kind(*mark, ErrorKind::ExpectedTagBang));
        }

        self.spend_directive_bytes(budget, 1, *mark)?;
        string.push('!');
        self.skip_non_blank();

        self.input.lookahead(1);
        while self.input.next_is_alpha() {
            self.spend_directive_bytes(budget, 1, *mark)?;
            string.push(self.input.peek());
            self.skip_non_blank();
            self.input.lookahead(1);
        }

        if self.input.peek() == '!' {
            self.spend_directive_bytes(budget, 1, *mark)?;
            string.push('!');
            self.skip_non_blank();
        } else if string != "!" {
            return Err(ScanError::from_kind(
                *mark,
                ErrorKind::ExpectedTagDirectiveBang,
            ));
        }

        Ok(string)
    }

    /// Scan a tag prefix for a `%TAG` directive as a `Cow<str>`.
    ///
    /// This borrows from `StrInput` only when no URI escape sequences are encountered. If a `%`
    /// escape is present, the prefix must be decoded and therefore allocated.
    fn scan_tag_prefix_directive_cow(
        &mut self,
        start_mark: &Marker,
        budget: &mut usize,
    ) -> Result<Cow<'input, str>, ScanError> {
        let Some(start) = self.input.byte_offset() else {
            return Ok(Cow::Owned(
                self.scan_tag_prefix_directive_owned(start_mark, budget)?,
            ));
        };

        // The prefix must start with either '!' (local) or a valid global tag char.
        if self.input.look_ch() == '!' {
            self.spend_directive_bytes(budget, 1, *start_mark)?;
            self.skip_non_blank();
        } else if !is_tag_char(self.input.peek()) {
            return Err(ScanError::from_kind(
                *start_mark,
                ErrorKind::InvalidGlobalTagCharacter,
            ));
        } else if self.input.peek() == '%' {
            // Needs decoding. Fall back to allocating path below.
        } else {
            self.spend_directive_bytes(budget, 1, *start_mark)?;
            self.skip_non_blank();
        }

        // Consume URI chars while we can stay in the borrowed path.
        while is_uri_char(self.input.look_ch()) {
            if self.input.peek() == '%' {
                break;
            }
            self.spend_directive_bytes(budget, 1, *start_mark)?;
            self.skip_non_blank();
        }

        // If we encountered an escape sequence, we must decode, therefore allocate.
        if self.input.peek() == '%' {
            let current = self
                .input
                .byte_offset()
                .expect("byte_offset() must remain available once enabled");
            let mut out = if let Some(slice) = self.input.slice_bytes(start, current) {
                slice.to_owned()
            } else {
                String::new()
            };

            while is_uri_char(self.input.look_ch()) {
                if self.input.peek() == '%' {
                    out.push(
                        self.scan_uri_escapes_with_budget(start_mark, Some(budget))?,
                    );
                } else {
                    self.spend_directive_bytes(budget, 1, *start_mark)?;
                    out.push(self.input.peek());
                    self.skip_non_blank();
                }
            }
            return Ok(Cow::Owned(out));
        }

        self.source_since(start, start_mark)
    }

    /// Owned `%TAG`-prefix scanner for streaming inputs, with allocation bounded by `budget`.
    fn scan_tag_prefix_directive_owned(
        &mut self,
        start_mark: &Marker,
        budget: &mut usize,
    ) -> Result<String, ScanError> {
        let mut string = String::new();

        if self.input.look_ch() == '!' {
            self.spend_directive_bytes(budget, 1, *start_mark)?;
            string.push('!');
            self.skip_non_blank();
        } else if !is_tag_char(self.input.peek()) {
            return Err(ScanError::from_kind(
                *start_mark,
                ErrorKind::InvalidGlobalTagCharacter,
            ));
        } else if self.input.peek() == '%' {
            string.push(self.scan_uri_escapes_with_budget(start_mark, Some(budget))?);
        } else {
            self.spend_directive_bytes(budget, 1, *start_mark)?;
            string.push(self.input.peek());
            self.skip_non_blank();
        }

        while is_uri_char(self.input.look_ch()) {
            if self.input.peek() == '%' {
                string
                    .push(self.scan_uri_escapes_with_budget(start_mark, Some(budget))?);
            } else {
                self.spend_directive_bytes(budget, 1, *start_mark)?;
                string.push(self.input.peek());
                self.skip_non_blank();
            }
        }

        Ok(string)
    }
    /// Create a scanner over the given input source.
    #[must_use]
    pub fn new(input: T) -> Self {
        Self::with_options(input, Options::default())
    }

    /// Create a scanner over the given input source with configurable resource limits.
    ///
    /// [`Options::emit_comments`] controls comment emission.
    /// [`Options::max_buffered_comment_events`] has no effect when using a scanner directly,
    /// because comment-event buffering is performed by [`crate::Parser`].
    #[must_use]
    pub fn with_options(input: T, options: Options) -> Self {
        let initial_byte_offset = input.byte_offset();
        // Avoid the full-string `may_contain_comments` pre-scan when comments will not be emitted.
        // `true` also keeps comment validation on the scanner's comment-aware path.
        let comments_possible = !options.emit_comments || input.may_contain_comments();
        Scanner {
            input,
            options,
            mark: Marker::new(0, 1, 0).with_byte_offset(initial_byte_offset),
            tokens: VecDeque::with_capacity(64),
            failed: false,
            deferred_error: None,
            comments_possible,

            stream_start_produced: false,
            stream_end_produced: false,
            document_prefix_allowed: true,
            adjacent_value_allowed_at: 0,
            simple_key_allowed: true,
            simple_keys: smallvec::SmallVec::new(),
            first_simple_key: None,
            indent: -1,
            indents: smallvec::SmallVec::new(),
            flow_level: 0,
            block_level: 0,
            tokens_parsed: 0,
            token_available: false,
            leading_whitespace: true,
            flow_mapping_started: smallvec::SmallVec::new(),
            implicit_flow_mapping_states: smallvec::SmallVec::new(),
            flow_markers: smallvec::SmallVec::new(),
            interrupted_plain_by_comment: None,
            explicit_key_tab_check_pending: false,

            buf_leading_break: String::with_capacity(128),
            buf_trailing_breaks: String::with_capacity(128),
            buf_whitespaces: String::with_capacity(128),
        }
    }

    #[cold]
    fn scan_error(&self, kind: ErrorKind) -> ScanError {
        ScanError::from_kind(self.mark, kind)
    }

    #[inline]
    fn ensure_current_char_is_printable(&self) -> ScanResult {
        let character = self.input.peek();
        if self.input.next_is_z() || is_printable(character) {
            Ok(())
        } else {
            Err(self.scan_error(ErrorKind::UnexpectedCharacter { character }))
        }
    }

    #[cold]
    fn simple_key_expected(mark: Marker) -> ScanError {
        ScanError::from_kind(mark, ErrorKind::SimpleKeyExpected)
    }

    #[cold]
    fn unclosed_bracket(mark: Marker, bracket: char) -> ScanError {
        ScanError::from_kind(mark, ErrorKind::UnclosedFlowCollection { open: bracket })
    }

    /// Consume the next character. It is assumed the next character is a blank.
    #[inline]
    fn skip_blank(&mut self) {
        self.input.skip();

        self.mark.offsets.chars += 1;
        self.mark.col += 1;
        self.mark.offsets.bytes = self.input.byte_offset();
    }

    /// Consume the next character. It is assumed the next character is not a blank.
    #[inline]
    fn skip_non_blank(&mut self) {
        self.input.skip();

        self.mark.offsets.chars += 1;
        self.mark.col += 1;
        self.mark.offsets.bytes = self.input.byte_offset();
        self.leading_whitespace = false;
    }

    /// Consume a byte order mark from a document prefix.
    ///
    /// The source index advances, but the logical column remains unchanged so directives and
    /// document markers immediately following the BOM are still recognized as line-start tokens.
    #[inline]
    fn skip_bom(&mut self) {
        self.input.skip();

        self.mark.offsets.chars += 1;
        self.mark.offsets.bytes = self.input.byte_offset();
    }

    /// Consume one character that belongs to a comment.
    ///
    /// Unlike [`Self::skip_non_blank`], this deliberately does not change
    /// `leading_whitespace`. Comments are presentation content, so consuming one for either
    /// tokenization or skipping should only advance position bookkeeping.
    #[inline]
    fn skip_comment_char(&mut self) {
        self.input.skip();

        self.mark.offsets.chars += 1;
        self.mark.col += 1;
        self.mark.offsets.bytes = self.input.byte_offset();
    }

    /// Consume the next characters. It is assumed none of the next characters are blanks.
    #[inline]
    fn skip_n_non_blank(&mut self, count: usize) {
        for _ in 0..count {
            self.input.skip();
            self.mark.offsets.chars += 1;
            self.mark.col += 1;
        }
        self.mark.offsets.bytes = self.input.byte_offset();
        self.leading_whitespace = false;
    }

    /// Consume the next character. It is assumed the next character is a newline.
    #[inline]
    fn skip_nl(&mut self) {
        self.input.skip();

        self.mark.offsets.chars += 1;
        self.mark.col = 0;
        self.mark.line += 1;
        self.mark.offsets.bytes = self.input.byte_offset();
        self.leading_whitespace = true;
    }

    /// Consume a line break (either CR, LF, or CRLF), if any. Do nothing if there is none.
    #[inline]
    fn skip_linebreak(&mut self) {
        if self.input.next_2_are('\r', '\n') {
            // While technically not a blank, this does not matter as `self.leading_whitespace`
            // will be reset by `skip_nl`.
            self.skip_blank();
            self.skip_nl();
        } else if self.input.next_is_break() {
            self.skip_nl();
        }
    }

    fn scan_comment_queued_token(&mut self) -> Result<QueuedToken<'input>, ScanError> {
        let start_mark = self.mark;
        debug_assert_eq!(self.input.peek(), '#');
        let placement = if self.leading_whitespace {
            Placement::Free
        } else {
            Placement::Right
        };

        self.skip_comment_char();

        let text = if let Some(start) = self.input.byte_offset() {
            // Stable byte offsets are available; slice the payload once at the end.
            let n = self.input.skip_while_non_breakz();
            self.mark.offsets.chars += n;
            self.mark.col += n;
            let byte_offset = self.input.byte_offset();
            self.mark.offsets.bytes = byte_offset;
            let end =
                byte_offset.expect("byte_offset must remain available once enabled");

            if let Some(slice) = self.try_borrow_slice(start, end) {
                Cow::Borrowed(slice)
            } else if let Some(slice) = self.input.slice_bytes(start, end) {
                // Defensive fallback for third-party inputs that expose offsets but cannot borrow.
                Cow::Owned(slice.to_owned())
            } else {
                return Err(ScanError::from_kind(
                    start_mark,
                    ErrorKind::InputOffsetsWithoutSlice,
                ));
            }
        } else {
            // Streaming input without stable offsets; collect into an owned string.
            let mut owned = String::new();
            while {
                let character = self.input.look_ch();
                !is_breakz(character) && is_printable(character)
            } {
                owned.push(self.input.peek());
                self.skip_comment_char();
            }
            Cow::Owned(owned)
        };

        self.ensure_current_char_is_printable()?;

        let end_mark = self.mark;
        let span = Span::new(start_mark, end_mark);
        Ok(Token(span, TokenType::Comment(Comment { text, placement })).into())
    }

    fn push_comment_token(&mut self) -> ScanResult {
        let token = self.scan_comment_queued_token()?;
        self.tokens.push_back(token);
        Ok(())
    }

    /// Consume the comment at the current position and return whether it was queued as a token.
    fn consume_comment(&mut self) -> Result<bool, ScanError> {
        if self.options.emit_comments {
            self.push_comment_token()?;
            return Ok(true);
        }
        self.skip_comment()?;
        Ok(false)
    }

    fn skip_comment(&mut self) -> ScanResult {
        debug_assert_eq!(self.input.peek(), '#');

        self.skip_comment_char();
        let n = self.input.skip_while_non_breakz();
        self.mark.offsets.chars += n;
        self.mark.col += n;
        self.mark.offsets.bytes = self.input.byte_offset();
        self.ensure_current_char_is_printable()
    }

    /// Return whether the [`TokenType::StreamStart`] event has been emitted.
    #[inline]
    #[must_use]
    pub fn stream_started(&self) -> bool {
        self.stream_start_produced
    }

    /// Return whether the [`TokenType::StreamEnd`] event has been emitted.
    #[inline]
    #[must_use]
    pub fn stream_ended(&self) -> bool {
        self.stream_end_produced
    }

    /// Return the current position in the input stream.
    #[inline]
    #[must_use]
    pub fn mark(&self) -> Marker {
        self.mark
    }

    /// Return whether this scanner may emit comment tokens.
    #[inline]
    pub(crate) fn comments_possible(&self) -> bool {
        self.options.emit_comments && self.comments_possible
    }

    // Read and consume a line break (either `\r`, `\n` or `\r\n`).
    //
    // A `\n` is pushed into `s`.
    //
    // # Panics (in debug)
    // If the next characters do not correspond to a line break.
    #[track_caller]
    #[inline]
    fn read_break(&mut self, s: &mut String) {
        self.skip_break();
        s.push('\n');
    }

    // Read and consume a line break (either `\r`, `\n` or `\r\n`).
    //
    // # Panics (in debug)
    // If the next characters do not correspond to a line break.
    #[track_caller]
    #[inline]
    fn skip_break(&mut self) {
        let c = self.input.peek();
        let nc = self.input.peek_nth(1);
        debug_assert!(is_break(c));
        if c == '\r' && nc == '\n' {
            self.skip_blank();
        }
        self.skip_nl();
    }

    /// Insert a token at the given position.
    ///
    /// # Panics
    /// Panics if `pos` is past the end of the token queue.
    #[track_caller]
    fn insert_token(&mut self, pos: usize, tok: Token<'input>) {
        let old_len = self.tokens.len();
        assert!(pos <= old_len);
        self.tokens.insert(pos, tok.into());
    }

    /// The queue position of a possible simple key's token, which stays queued until the key
    /// is resolved.
    fn simple_key_token_index(&self, sk: &SimpleKey) -> usize {
        sk.token_number
            .checked_sub(self.tokens_parsed)
            .expect("a possible simple key's token is still queued")
    }

    #[inline]
    fn allow_simple_key(&mut self) {
        self.simple_key_allowed = true;
    }

    #[inline]
    fn disallow_simple_key(&mut self) {
        self.simple_key_allowed = false;
    }

    /// Scan enough input to append one next token to the internal token queue.
    ///
    /// # Errors
    /// Returns `ScanError` when the scanner does not find the next expected token.
    fn fetch_next_token(&mut self) -> ScanResult {
        let result = self.fetch_next_token_impl();
        if let Some(kind) = self.input.take_source_error() {
            return Err(ScanError::from_kind(self.mark, kind));
        }
        result
    }

    fn fetch_next_token_impl(&mut self) -> ScanResult {
        self.input.lookahead(1);

        if !self.stream_start_produced {
            self.fetch_stream_start();
            return Ok(());
        }
        loop {
            let outcome = self.skip_to_next_token()?;
            if outcome.queued_comment {
                return Ok(());
            }
            if outcome.saw_comment && !self.tokens.is_empty() {
                return Ok(());
            }
            if !outcome.saw_comment {
                break;
            }
            // No token is waiting, so an ignored leading comment is not enough to satisfy this
            // fetch. Continue until a token is produced or another scanner condition stops us.
        }

        self.stale_simple_keys()?;

        let mark = self.mark;
        self.unroll_indent(mark.col as isize);

        self.input.lookahead(4);

        if self.input.next_is_z() {
            self.fetch_stream_end()?;
            return Ok(());
        }

        self.ensure_current_char_is_printable()?;

        if self.mark.col == 0 {
            if self.input.next_char_is('%') {
                return self.fetch_directive();
            } else if self.input.next_is_document_start() {
                return self.fetch_document_indicator(TokenType::DocumentStart);
            } else if self.input.next_is_document_end() {
                self.fetch_document_indicator(TokenType::DocumentEnd)?;
                self.skip_ws_to_eol(SkipTabs::Yes)?;
                if !self.input.next_is_breakz() {
                    return Err(self.scan_error(ErrorKind::InvalidDocumentEnd));
                }
                return Ok(());
            }
        }

        if self.document_prefix_allowed {
            self.document_prefix_allowed = false;
        }

        // Block indentation was handled by `unroll_indent`. Flow entries and delimiters
        // may be under-indented only when strict YAML indentation is disabled.
        if self.options.strict_indentation
            && self.flow_level > 0
            && (self.mark.col as isize) <= self.flow_block_indent()
        {
            return Err(self.scan_error(ErrorKind::InvalidIndentation));
        }

        let c = self.input.peek();
        let nc = self.input.peek_nth(1);
        match c {
            '[' => self.fetch_flow_collection_start(TokenType::FlowSequenceStart),
            '{' => self.fetch_flow_collection_start(TokenType::FlowMappingStart),
            ']' => self.fetch_flow_collection_end(TokenType::FlowSequenceEnd, '[', ']'),
            '}' => self.fetch_flow_collection_end(TokenType::FlowMappingEnd, '{', '}'),
            ',' => self.fetch_flow_entry(),
            '-' if is_blank_or_breakz(nc) => self.fetch_block_entry(),
            '?' if is_blank_or_breakz(nc) => self.fetch_key(),
            ':' if is_blank_or_breakz(nc) => self.fetch_value(),
            ':' if self.flow_level > 0
                && (is_flow(nc)
                    || self.mark.index() == self.adjacent_value_allowed_at) =>
            {
                self.fetch_flow_value()
            }
            // Is it an alias?
            '*' => self.fetch_anchor(true),
            // Is it an anchor?
            '&' => self.fetch_anchor(false),
            '!' => self.fetch_tag(),
            // Is it a literal scalar?
            '|' if self.flow_level == 0 => self.fetch_block_scalar(true),
            // Is it a folded scalar?
            '>' if self.flow_level == 0 => self.fetch_block_scalar(false),
            '\'' => self.fetch_flow_scalar(true),
            '"' => self.fetch_flow_scalar(false),
            // plain scalar
            '-' if !is_blank_or_breakz(nc) => self.fetch_plain_scalar(),
            ':' | '?' if !is_blank_or_breakz(nc) && self.flow_level == 0 => {
                self.fetch_plain_scalar()
            }
            c if is_bom(c) => Err(self.scan_error(ErrorKind::BomInsideDocument)),
            '|' | '>' | '%' | '@' | '`' => {
                Err(self.scan_error(ErrorKind::UnexpectedCharacter { character: c }))
            }
            _ => self.fetch_plain_scalar(),
        }
    }

    /// Return the next compact queued token, scanning more input when needed.
    ///
    /// # Errors
    /// Returns `ScanError` when scanning fails to find an expected next token.
    pub(crate) fn next_queued_token(
        &mut self,
    ) -> Result<Option<QueuedToken<'input>>, ScanError> {
        // An error is deferred only behind the one comment queued with it, which was returned.
        if let Some(error) = self.deferred_error.take() {
            return error.into_result();
        }

        if self.stream_end_produced {
            return Ok(None);
        }

        if !self.token_available
            && let Err(error) = self.fetch_more_tokens()
        {
            if matches!(
                self.tokens.front().map(|token| &token.1),
                Some(QueuedTokenType::Comment(_))
            ) {
                self.deferred_error = Some(error);
            } else {
                return Err(error);
            }
        }
        let t = self
            .tokens
            .pop_front()
            .expect("fetch_more_tokens succeeded without producing a token");
        self.token_available = false;
        self.tokens_parsed += 1;

        let is_stream_end = matches!(t.1, QueuedTokenType::StreamEnd);
        if is_stream_end {
            self.stream_end_produced = true;
        }
        Ok(Some(t))
    }

    /// Return the next queued token, scanning more input when needed.
    ///
    /// # Errors
    /// Returns `ScanError` when scanning fails to find an expected next token.
    fn next_token(&mut self) -> Result<Option<Token<'input>>, ScanError> {
        Ok(self.next_queued_token()?.map(QueuedToken::into_public))
    }

    /// Only the oldest possible simple key can refer to the next token to emit.
    fn oldest_simple_key_is_next_token(&self) -> bool {
        self.first_simple_key.is_some_and(|index| {
            self.simple_keys[index].token_number == self.tokens_parsed
        })
    }

    /// Scan more input until a token is ready to be returned.
    ///
    /// # Errors
    /// Returns `ScanError` when scanning fails.
    fn fetch_more_tokens(&mut self) -> ScanResult {
        let mut need_more;
        loop {
            if self.tokens.is_empty() {
                need_more = true;
            } else {
                // Stale potential keys that we know won't be keys.
                self.stale_simple_keys()?;
                need_more = !matches!(
                    self.tokens.front().map(|token| &token.1),
                    Some(QueuedTokenType::Comment(_))
                ) && self.oldest_simple_key_is_next_token();
            }

            // Stop fetching immediately after document end/start markers
            // to allow the parser to emit the event before reading more content.
            if let Some(token) = self.tokens.back()
                && matches!(
                    token.1,
                    QueuedTokenType::DocumentEnd | QueuedTokenType::DocumentStart
                )
            {
                break;
            }

            if !need_more {
                break;
            }
            self.fetch_next_token()?;
        }
        self.token_available = true;

        Ok(())
    }

    /// Mark simple keys that can no longer be keys as such.
    ///
    /// This function sets `possible` to `false` to each key that, now we have more context, we
    /// know will not be keys.
    ///
    /// # Errors
    /// This function returns an error if one of the keys becoming impossible was required to be a
    /// key.
    fn stale_simple_keys(&mut self) -> ScanResult {
        while let Some(index) = self.first_simple_key {
            let sk = &mut self.simple_keys[index];
            debug_assert!(sk.possible);
            let is_line_stale = self.flow_level == 0 && sk.mark.line < self.mark.line;
            // The length cap applies in flow contexts too; otherwise token buffering can grow
            // without bound while the scanner waits to see whether a later ':' resolves the key.
            let is_length_stale = self.mark.index().saturating_sub(sk.mark.index())
                > self.options.simple_key_max_lookahead;

            if !is_line_stale && !is_length_stale {
                // Inner candidates start no earlier, so they cannot have expired either.
                break;
            }
            if sk.required {
                return Err(Self::simple_key_expected(sk.mark));
            }
            sk.possible = false;
            // Skip inactive slots only when advancing past an expired candidate. Saving a key
            // at an earlier level requires popping its descendants, so this work is amortized.
            self.first_simple_key = self.simple_keys[index + 1..]
                .iter()
                .position(|key| key.possible)
                .map(|offset| index + 1 + offset);
        }
        Ok(())
    }

    /// Consume a single logical line break and apply simple-key rules.
    #[inline]
    fn consume_linebreak(&mut self) {
        self.input.lookahead(2);
        self.skip_linebreak();
        if self.flow_level == 0 {
            self.allow_simple_key();
        }
    }

    /// Skip a tab, which may separate flow entries but cannot supply required indentation.
    fn skip_tab(&mut self) -> ScanResult {
        if !(self.is_within_block()
            && self.leading_whitespace
            && (self.mark.col as isize) < self.tab_indent_limit())
        {
            self.skip_blank();
            return Ok(());
        }
        let (chars_consumed, _) = self.input.skip_ws_to_eol_blanks(SkipTabs::Yes);
        self.mark.col += chars_consumed;
        self.mark.offsets.chars += chars_consumed;
        self.mark.offsets.bytes = self.input.byte_offset();

        // A comment is left for the caller's comment branch to record.
        if self.input.look_ch() == '#' {
            return Ok(());
        }
        if !self.input.next_is_breakz() {
            return Err(self.scan_error(ErrorKind::TabInBlockIndentation));
        }
        if matches!(self.input.look_ch(), '\n' | '\r') {
            self.consume_linebreak();
        }
        Ok(())
    }

    /// Skip over whitespace (`\t`, ` `, `\n`, `\r`) until the next non-comment token.
    ///
    /// The function returns after one comment, whether it was queued or ignored, so callers can
    /// publish completed syntax before scanning later input that may fail.
    ///
    /// When comment emission is enabled, encountered comments are queued as
    /// [`TokenType::Comment`] tokens so the parser can emit presentation events.
    ///
    /// # Errors
    /// This function returns an error if a tab is encountered where there should not be
    /// one.
    fn skip_to_next_token(&mut self) -> Result<SkipToNextTokenOutcome, ScanError> {
        let mut outcome = SkipToNextTokenOutcome::default();
        loop {
            let ch = self.input.look_ch();
            if self.explicit_key_tab_check_pending {
                match ch {
                    '\t' => {
                        self.skip_explicit_key_tab()?;
                        continue;
                    }
                    ' ' | '\n' | '\r' | '#' => {}
                    '&' | '!' if self.options.strict_indentation => {}
                    _ => self.explicit_key_tab_check_pending = false,
                }
            }

            match ch {
                '\t' => self.skip_tab()?,

                ' ' => self.skip_blank(),

                '\n' | '\r' => self.consume_linebreak(),

                c if is_bom(c)
                    && self.document_prefix_allowed
                    && self.flow_level == 0
                    && self.mark.col == 0 =>
                {
                    self.skip_bom();
                }

                '#' => {
                    outcome.saw_comment = true;
                    outcome.queued_comment |= self.consume_comment()?;

                    // Micro-opt: comment-only lines are common; consume the following line break here.
                    if matches!(self.input.look_ch(), '\n' | '\r') {
                        self.consume_linebreak();
                    }
                    return Ok(outcome);
                }

                _ => break,
            }
        }

        // If a plain scalar was interrupted by a comment, and the next line could
        // continue the scalar in block context, this is invalid.
        if let Some(err_mark) = self.interrupted_plain_by_comment.take() {
            // BS4K should only trigger when the continuation would start on the immediate next
            // line (no intervening empty/comment-only lines). A blank line resets the folding
            // opportunity and thus should not error.
            let is_immediate_next_line = self.mark.line == err_mark.line + 1;

            // Optimization: do the cheap checks first; only then request extra lookahead / do deeper checks.
            if self.flow_level == 0
                && is_immediate_next_line
                && (self.mark.col as isize) > self.indent
            {
                // Ensure enough lookahead for:
                // - the checks below (peek/peek_nth)
                // - document indicator detection which needs 4 chars.
                self.input.lookahead(4);

                if !self.input.next_is_z()
                    && !self.input.next_is_document_indicator()
                    && self.input.next_can_be_plain_scalar(false)
                {
                    return Err(ScanError::from_kind(
                        err_mark,
                        ErrorKind::CommentInterceptedScalar,
                    ));
                }
            }
        }

        Ok(outcome)
    }

    /// Consume a tab in the separation following a block explicit-key indicator.
    fn skip_explicit_key_tab(&mut self) -> ScanResult {
        if self.leading_whitespace && (self.mark.col as isize) <= self.indent {
            let tab_mark = self.mark;
            let (chars_consumed, _) = self.input.skip_ws_to_eol_blanks(SkipTabs::Yes);
            self.mark.col += chars_consumed;
            self.mark.offsets.chars += chars_consumed;
            self.mark.offsets.bytes = self.input.byte_offset();
            // Blank and comment-only lines do not require key-node indentation.
            if self.input.look_ch() != '#' && !self.input.next_is_breakz() {
                return Err(ScanError::from_kind(tab_mark, ErrorKind::TabNotAllowed));
            }
        } else {
            self.skip_blank();
        }
        // Inline flow/scalar nodes accept tab separation. Compact block collections require
        // spaces, so no simple key or block entry may start until the next line.
        self.disallow_simple_key();
        Ok(())
    }

    /// Skip the separation after a block explicit key indicator, which the dispatcher only
    /// recognizes before a blank, a line break or a NUL. Returns whether it stopped after a
    /// comment.
    fn skip_yaml_whitespace(&mut self) -> Result<bool, ScanError> {
        let mut need_whitespace = true;
        loop {
            match self.input.look_ch() {
                ' ' => self.skip_blank(),
                '\t' => self.skip_explicit_key_tab()?,
                '\n' | '\r' => {
                    self.input.lookahead(2);
                    self.skip_linebreak();
                    self.allow_simple_key();
                }
                // A comment needs separation, which the dispatcher already saw.
                '#' => {
                    self.consume_comment()?;
                    return Ok(true);
                }
                _ => break,
            }
            need_whitespace = false;
        }
        // A literal NUL also follows `?` in the dispatcher, but only the end of input separates.
        if need_whitespace && !self.input.next_is_z() {
            Err(self.scan_error(ErrorKind::ExpectedWhitespace))
        } else {
            Ok(false)
        }
    }

    /// Skip YAML whitespace up to the end of the current line.
    fn skip_ws_to_eol(
        &mut self,
        skip_tabs: SkipTabs,
    ) -> Result<WhitespaceResult, ScanError> {
        if !self.comments_possible {
            let (chars_consumed, result) = self.input.skip_ws_to_eol(skip_tabs);
            self.mark.col += chars_consumed;
            self.mark.offsets.chars += chars_consumed;
            self.mark.offsets.bytes = self.input.byte_offset();
            return result.map_err(|kind| self.scan_error(kind));
        }

        let (chars_consumed, whitespace) = self.input.skip_ws_to_eol_blanks(skip_tabs);
        self.mark.col += chars_consumed;
        self.mark.offsets.chars += chars_consumed;
        self.mark.offsets.bytes = self.input.byte_offset();

        if self.input.look_ch() != '#' {
            return Ok(whitespace);
        }

        if !whitespace.found_tabs() && !whitespace.has_valid_yaml_ws() {
            return Err(self.scan_error(ErrorKind::CommentNotSeparated));
        }

        self.consume_comment()?;
        Ok(whitespace)
    }

    fn fetch_stream_start(&mut self) {
        let mark = self.mark;
        self.indent = -1;
        self.stream_start_produced = true;
        self.allow_simple_key();
        self.tokens
            .push_back(Token(Span::empty(mark), TokenType::StreamStart).into());
        self.simple_keys.push(SimpleKey::new(Marker::new(0, 0, 0)));
    }

    fn fetch_stream_end(&mut self) -> ScanResult {
        // Close blocks at the physical EOF position. No line break was consumed here;
        // moving the marker would also corrupt a pending fallible-input error's location.
        if let Some((mark, bracket)) = self.flow_markers.pop() {
            return Err(Self::unclosed_bracket(mark, bracket));
        }

        // If the stream ended, we won't have more context. We can stall all the simple keys we
        // had. If one was required, however, that was an error and we must propagate it.
        for sk in &mut self.simple_keys {
            if sk.required && sk.possible {
                return Err(Self::simple_key_expected(sk.mark));
            }
            sk.possible = false;
        }
        self.first_simple_key = None;

        self.unroll_indent(-1);
        self.clear_simple_key();
        self.disallow_simple_key();

        self.tokens
            .push_back(Token(Span::empty(self.mark), TokenType::StreamEnd).into());
        Ok(())
    }

    fn fetch_directive(&mut self) -> ScanResult {
        self.unroll_indent(-1);
        self.clear_simple_key();

        self.disallow_simple_key();

        let token_index = self.tokens.len();
        let tok = self.scan_directive()?;
        self.insert_token(token_index, tok);

        Ok(())
    }

    /// Scan a reserved directive's parameters, and whether a comment follows them.
    fn scan_reserved_directive_params(
        &mut self,
        start_mark: Marker,
        budget: &mut usize,
    ) -> Result<(Vec<String>, bool), ScanError> {
        let mut params = Vec::new();
        while self.input.next_is_blank() {
            let n_blanks = self.input.skip_while_blank();
            self.mark.offsets.chars += n_blanks;
            self.mark.col += n_blanks;
            self.mark.offsets.bytes = self.input.byte_offset();

            if self.input.peek() == '#' {
                return Ok((params, true));
            }
            if is_blank_or_breakz(self.input.peek()) {
                continue;
            }
            self.spend_directive_bytes(budget, n_blanks, start_mark)?;
            if params.len() >= self.options.max_reserved_directive_params {
                return Err(ScanError::from_kind(
                    start_mark,
                    ErrorKind::TooManyReservedDirectiveParams {
                        limit: self.options.max_reserved_directive_params,
                    },
                ));
            }

            let mut param = String::new();
            let Some(n_chars) = self.fetch_directive_word(&mut param, budget) else {
                return Err(self.directive_byte_limit(start_mark));
            };
            self.mark.offsets.chars += n_chars;
            self.mark.col += n_chars;
            self.mark.offsets.bytes = self.input.byte_offset();
            params.push(param);
        }
        Ok((params, false))
    }

    fn scan_directive(&mut self) -> Result<Token<'input>, ScanError> {
        let start_mark = self.mark;
        self.skip_non_blank();

        // Directives precede every event, so a document could otherwise make the scanner retain
        // memory proportional to the input before any event, node or scalar budget can apply.
        let mut budget = self.options.max_directive_bytes;

        let name = self.scan_directive_name(&mut budget)?;
        let mut separated_comment = false;
        let tok = match name.as_ref() {
            "YAML" => self.scan_version_directive_value(&start_mark)?,
            "TAG" => self.scan_tag_directive_value(&start_mark, &mut budget)?,
            _ => {
                let params;
                (params, separated_comment) =
                    self.scan_reserved_directive_params(start_mark, &mut budget)?;
                Token(
                    Span::new(start_mark, self.mark),
                    TokenType::ReservedDirective(name, params),
                )
            }
        };

        if separated_comment {
            // The reserved-parameter loop already consumed the comment's separator.
            self.consume_comment()?;
        } else {
            self.skip_ws_to_eol(SkipTabs::Yes)?;
        }

        if self.input.next_is_breakz() {
            self.input.lookahead(2);
            self.skip_linebreak();
            Ok(tok)
        } else {
            Err(ScanError::from_kind(
                start_mark,
                ErrorKind::InvalidDirectiveTerminator,
            ))
        }
    }

    fn scan_version_directive_value(
        &mut self,
        mark: &Marker,
    ) -> Result<Token<'input>, ScanError> {
        let n_blanks = self.input.skip_while_blank();
        self.mark.offsets.chars += n_blanks;
        self.mark.col += n_blanks;
        self.mark.offsets.bytes = self.input.byte_offset();

        let major = self.scan_version_directive_number(mark)?;

        if self.input.peek() != '.' {
            return Err(ScanError::from_kind(
                *mark,
                ErrorKind::MissingYamlVersionSeparator,
            ));
        }
        self.skip_non_blank();

        let minor = self.scan_version_directive_number(mark)?;

        Ok(Token(
            Span::new(*mark, self.mark),
            TokenType::VersionDirective(major, minor),
        ))
    }

    /// Build the error reported when a directive outgrows [`Options::max_directive_bytes`].
    fn directive_byte_limit(&self, start_mark: Marker) -> ScanError {
        ScanError::from_kind(
            start_mark,
            ErrorKind::DirectiveByteLimitExceeded {
                limit: self.options.max_directive_bytes,
            },
        )
    }

    /// Spend source bytes from a directive-retention budget before growing owned storage.
    fn spend_directive_bytes(
        &self,
        budget: &mut usize,
        bytes: usize,
        start_mark: Marker,
    ) -> ScanResult {
        let Some(remaining) = budget.checked_sub(bytes) else {
            return Err(self.directive_byte_limit(start_mark));
        };
        *budget = remaining;
        Ok(())
    }

    /// Fetch a run of [`is_yaml_non_space`] characters into `out`, spending `budget` bytes.
    ///
    /// This mirrors [`crate::input::Input::fetch_while_is_yaml_non_space`] but stops as soon as
    /// the run would outgrow `budget`, returning [`None`]. Checking before each push is what keeps
    /// the allocation bounded: a limit applied after the fetch would already have grown `out` to
    /// the size of the run.
    fn fetch_directive_word(
        &mut self,
        out: &mut String,
        budget: &mut usize,
    ) -> Option<usize> {
        let mut n_chars = 0;

        loop {
            let c = self.input.look_ch();
            if !is_yaml_non_space(c) || is_z(c) {
                break;
            }

            let len = c.len_utf8();
            if len > *budget {
                return None;
            }
            *budget -= len;

            out.push(c);
            self.input.skip();
            n_chars += 1;
        }

        Some(n_chars)
    }

    fn scan_directive_name(&mut self, budget: &mut usize) -> Result<String, ScanError> {
        let start_mark = self.mark;
        let mut string = String::new();

        let Some(n_chars) = self.fetch_directive_word(&mut string, budget) else {
            return Err(self.directive_byte_limit(start_mark));
        };
        self.mark.offsets.chars += n_chars;
        self.mark.col += n_chars;
        self.mark.offsets.bytes = self.input.byte_offset();

        if string.is_empty() {
            return Err(ScanError::from_kind(
                start_mark,
                ErrorKind::MissingDirectiveName,
            ));
        }

        if !is_blank_or_breakz(self.input.peek()) {
            return Err(ScanError::from_kind(
                start_mark,
                ErrorKind::InvalidDirectiveName,
            ));
        }

        Ok(string)
    }

    fn scan_version_directive_number(
        &mut self,
        mark: &Marker,
    ) -> Result<u32, ScanError> {
        let mut val = 0u32;
        let mut length = 0usize;
        while let Some(digit) = self.input.look_ch().to_digit(10) {
            if length + 1 > 9 {
                return Err(ScanError::from_kind(*mark, ErrorKind::YamlVersionTooLong));
            }
            length += 1;
            val = val * 10 + digit;
            self.skip_non_blank();
        }

        if length == 0 {
            return Err(ScanError::from_kind(*mark, ErrorKind::MissingYamlVersion));
        }

        Ok(val)
    }

    fn scan_tag_directive_value(
        &mut self,
        mark: &Marker,
        budget: &mut usize,
    ) -> Result<Token<'input>, ScanError> {
        let n_blanks = self.input.skip_while_blank();
        self.mark.offsets.chars += n_blanks;
        self.mark.col += n_blanks;
        self.mark.offsets.bytes = self.input.byte_offset();
        self.spend_directive_bytes(budget, n_blanks, *mark)?;

        let handle = self.scan_tag_handle_directive_cow(mark, budget)?;

        let n_blanks = self.input.skip_while_blank();
        self.mark.offsets.chars += n_blanks;
        self.mark.col += n_blanks;
        self.mark.offsets.bytes = self.input.byte_offset();
        self.spend_directive_bytes(budget, n_blanks, *mark)?;

        let prefix = self.scan_tag_prefix_directive_cow(mark, budget)?;

        self.input.lookahead(1);

        if self.input.next_is_blank_or_breakz() {
            Ok(Token(
                Span::new(*mark, self.mark),
                TokenType::TagDirective(handle, prefix),
            ))
        } else {
            Err(ScanError::from_kind(
                *mark,
                ErrorKind::InvalidTagDirectiveTerminator,
            ))
        }
    }

    fn fetch_tag(&mut self) -> ScanResult {
        self.save_simple_key();
        self.disallow_simple_key();

        let tok = self.scan_tag()?;
        self.tokens.push_back(tok.into());
        Ok(())
    }

    fn scan_tag(&mut self) -> Result<Token<'input>, ScanError> {
        let start_mark = self.mark;

        // Check if the tag is in the canonical form (verbatim).
        self.input.lookahead(2);

        // If byte_offset is not available, use the original owned-only path.
        let Some(start) = self.input.byte_offset() else {
            return self.scan_tag_owned(&start_mark);
        };

        let (handle, suffix): (Cow<'input, str>, Cow<'input, str>) = if self
            .input
            .nth_char_is(1, '<')
        {
            // Verbatim tags always need owned strings (URI escapes).
            let suffix = self.scan_verbatim_tag(&start_mark)?;
            (Cow::Owned(String::new()), Cow::Owned(suffix))
        } else {
            // The tag has either the '!suffix' or the '!handle!suffix'
            let handle = self.scan_tag_handle_cow(start, &start_mark)?;
            // Check if it is, indeed, handle.
            if handle.len() >= 2 && handle.starts_with('!') && handle.ends_with('!') {
                // A tag handle starting with "!!" is a secondary tag handle.
                let suffix = self.scan_tag_shorthand_suffix_cow(&start_mark, true)?;
                (handle, suffix)
            } else {
                // Not a real handle, it's part of the suffix.
                // E.g., "!foo" -> handle="!", suffix="foo"
                // The "handle" we scanned is actually "!" + suffix_part1.
                // We need to also scan any remaining suffix characters.
                let remaining_suffix =
                    self.scan_tag_shorthand_suffix_cow(&start_mark, false)?;

                let suffix = self.combine_local_tag_suffix(
                    &start_mark,
                    handle,
                    remaining_suffix,
                );

                // A special case: the '!' tag.  Set the handle to '' and the
                // suffix to '!'.
                if suffix.is_empty() {
                    (Cow::Borrowed(""), Cow::Borrowed("!"))
                } else {
                    (Cow::Borrowed("!"), suffix)
                }
            }
        };

        if is_blank_or_breakz(self.input.look_ch())
            || (self.flow_level > 0 && matches!(self.input.peek(), ',' | ']' | '}'))
        {
            // YAML example 7.2 allows a tag to annotate an empty scalar when a separator or flow
            // delimiter follows.
            Ok(Token(
                Span::new(start_mark, self.mark),
                TokenType::Tag(handle, suffix),
            ))
        } else {
            Err(ScanError::from_kind(
                start_mark,
                ErrorKind::InvalidTagTerminator,
            ))
        }
    }

    fn combine_local_tag_suffix(
        &self,
        start_mark: &Marker,
        handle: Cow<'input, str>,
        remaining: Cow<'input, str>,
    ) -> Cow<'input, str> {
        if handle.len() == 1 {
            return remaining;
        }

        match (handle, remaining) {
            (Cow::Borrowed(handle), Cow::Borrowed(remaining)) => {
                if remaining.is_empty() {
                    return Cow::Borrowed(&handle[1..]);
                }

                let borrowed = start_mark
                    .byte_offset()
                    .and_then(|start| start.checked_add(1))
                    .zip(self.input.byte_offset())
                    .and_then(|(start, end)| self.try_borrow_slice(start, end));
                // An input may lend each part yet decline the combined range.
                borrowed.map_or_else(
                    || {
                        let mut combined =
                            String::with_capacity(handle.len() - 1 + remaining.len());
                        combined.push_str(&handle[1..]);
                        combined.push_str(remaining);
                        Cow::Owned(combined)
                    },
                    Cow::Borrowed,
                )
            }
            (Cow::Borrowed(handle), Cow::Owned(mut remaining)) => {
                remaining.reserve(handle.len() - 1);
                remaining.insert_str(0, &handle[1..]);
                Cow::Owned(remaining)
            }
            (Cow::Owned(mut handle), remaining) => {
                handle.remove(0);
                handle.push_str(&remaining);
                Cow::Owned(handle)
            }
        }
    }

    /// Original owned-only tag scanning path for inputs without `byte_offset` support.
    fn scan_tag_owned(
        &mut self,
        start_mark: &Marker,
    ) -> Result<Token<'input>, ScanError> {
        let mut handle = String::new();
        let mut suffix;

        if self.input.nth_char_is(1, '<') {
            suffix = self.scan_verbatim_tag(start_mark)?;
        } else {
            // The tag has either the '!suffix' or the '!handle!suffix'
            handle = self.scan_tag_handle();
            // Check if it is, indeed, handle.
            if handle.len() >= 2 && handle.starts_with('!') && handle.ends_with('!') {
                // A tag handle starting with "!!" is a secondary tag handle.
                let is_secondary_handle = handle == "!!";
                suffix = self.scan_tag_shorthand_suffix(
                    false,
                    is_secondary_handle,
                    "",
                    start_mark,
                )?;
            } else {
                suffix =
                    self.scan_tag_shorthand_suffix(false, false, &handle, start_mark)?;
                "!".clone_into(&mut handle);
                // A special case: the '!' tag.  Set the handle to '' and the
                // suffix to '!'.
                if suffix.is_empty() {
                    handle.clear();
                    "!".clone_into(&mut suffix);
                }
            }
        }

        if is_blank_or_breakz(self.input.look_ch())
            || (self.flow_level > 0 && matches!(self.input.peek(), ',' | ']' | '}'))
        {
            // YAML example 7.2 allows a tag to annotate an empty scalar when a separator or flow
            // delimiter follows.
            Ok(Token(
                Span::new(*start_mark, self.mark),
                TokenType::Tag(handle.into(), suffix.into()),
            ))
        } else {
            Err(ScanError::from_kind(
                *start_mark,
                ErrorKind::InvalidTagTerminator,
            ))
        }
    }

    /// Scan a tag handle as a `Cow<str>`, borrowing when possible.
    ///
    /// Tag handles are of the form `!`, `!!`, or `!name!` where name is ASCII alphanumeric.
    /// Since they contain no escape sequences, they can always be borrowed from `StrInput`.
    fn scan_tag_handle_cow(
        &mut self,
        start: usize,
        mark: &Marker,
    ) -> Result<Cow<'input, str>, ScanError> {
        // Consume the leading '!'.
        self.skip_non_blank();

        // Consume ns-word-char (ASCII alphanumeric, '_' or '-') characters.
        self.input.lookahead(1);
        while self.input.next_is_alpha() {
            self.skip_non_blank();
            self.input.lookahead(1);
        }

        // Optional trailing '!'.
        if self.input.peek() == '!' {
            self.skip_non_blank();
        }

        self.source_since(start, mark)
    }

    /// Append the remaining tag characters to `out`, decoding URI escapes.
    fn scan_tag_chars_into(
        &mut self,
        mut out: String,
        mark: &Marker,
    ) -> Result<String, ScanError> {
        while is_tag_char(self.input.look_ch()) {
            if self.input.peek() == '%' {
                out.push(self.scan_uri_escapes(mark)?);
            } else {
                out.push(self.input.peek());
                self.skip_non_blank();
            }
        }
        Ok(out)
    }

    /// Scan a tag shorthand suffix as a `Cow<str>`, borrowing when possible.
    ///
    /// The suffix can be borrowed only if no `%` URI escape sequences are present.
    fn scan_tag_shorthand_suffix_cow(
        &mut self,
        mark: &Marker,
        require_non_empty: bool,
    ) -> Result<Cow<'input, str>, ScanError> {
        // The handle just scanned ended here with an offset.
        let start = self
            .input
            .byte_offset()
            .expect("byte_offset() must remain available once enabled");

        // Scan tag characters, checking for URI escapes.
        while is_tag_char(self.input.look_ch()) {
            if self.input.peek() == '%' {
                // URI escape found - must decode, so fall back to owned path.
                let current = self
                    .input
                    .byte_offset()
                    .expect("byte_offset() must remain available once enabled");
                let out = self
                    .input
                    .slice_bytes(start, current)
                    .unwrap_or_default()
                    .to_owned();
                return Ok(Cow::Owned(self.scan_tag_chars_into(out, mark)?));
            }
            self.skip_non_blank();
        }

        // An input may stop reporting offsets part-way through a tag.
        let Some(end) = self.input.byte_offset() else {
            return Ok(Cow::Owned(
                self.scan_tag_shorthand_suffix(false, false, "", mark)?,
            ));
        };

        if require_non_empty && start == end {
            return Err(ScanError::from_kind(*mark, ErrorKind::MissingTagUri));
        }

        if let Some(slice) = self.try_borrow_slice(start, end) {
            Ok(Cow::Borrowed(slice))
        } else {
            let slice = self.input.slice_bytes(start, end).ok_or_else(|| {
                ScanError::from_kind(*mark, ErrorKind::InputSlicingUnavailable)
            })?;
            Ok(Cow::Owned(slice.to_owned()))
        }
    }

    /// Scan a tag token's handle, starting at its `!`.
    fn scan_tag_handle(&mut self) -> String {
        let mut string = String::new();
        self.input.lookahead(1);
        string.push(self.input.peek());
        self.skip_non_blank();

        let n_chars = self.input.fetch_while_is_alpha(&mut string);
        self.mark.offsets.chars += n_chars;
        self.mark.col += n_chars;
        self.mark.offsets.bytes = self.input.byte_offset();

        // Check if the trailing character is '!' and copy it.
        // Without a trailing '!', the scanned word is the start of a local tag's suffix.
        if self.input.peek() == '!' {
            string.push(self.input.peek());
            self.skip_non_blank();
        }
        string
    }

    /// Scan for a verbatim tag.
    ///
    /// The prefixing `!<` must _not_ have been skipped.
    fn scan_verbatim_tag(&mut self, start_mark: &Marker) -> Result<String, ScanError> {
        // Eat `!<`
        self.skip_non_blank();
        self.skip_non_blank();

        let mut string = String::new();
        while is_uri_char(self.input.look_ch()) {
            if self.input.peek() == '%' {
                string.push(self.scan_uri_escapes(start_mark)?);
            } else {
                string.push(self.input.peek());
                self.skip_non_blank();
            }
        }

        if string.is_empty() {
            return Err(ScanError::from_kind(*start_mark, ErrorKind::MissingTagUri));
        }

        if self.input.peek() != '>' {
            return Err(ScanError::from_kind(
                *start_mark,
                ErrorKind::UnclosedVerbatimTag,
            ));
        }
        self.skip_non_blank();

        Ok(string)
    }

    fn scan_tag_shorthand_suffix(
        &mut self,
        _directive: bool,
        _is_secondary: bool,
        head: &str,
        mark: &Marker,
    ) -> Result<String, ScanError> {
        let mut length = head.len();
        let mut string = String::new();

        // Copy the head if needed.
        // Note that we don't copy the leading '!' character.
        if length > 1 {
            string.extend(head.chars().skip(1));
        }

        while is_tag_char(self.input.look_ch()) {
            // Check if it is a URI-escape sequence.
            if self.input.peek() == '%' {
                string.push(self.scan_uri_escapes(mark)?);
            } else {
                string.push(self.input.peek());
                self.skip_non_blank();
            }

            length += 1;
        }

        if length == 0 {
            return Err(ScanError::from_kind(*mark, ErrorKind::MissingTagUri));
        }

        Ok(string)
    }

    fn scan_uri_escapes(&mut self, mark: &Marker) -> Result<char, ScanError> {
        self.scan_uri_escapes_with_budget(mark, None)
    }

    fn scan_uri_escapes_with_budget(
        &mut self,
        mark: &Marker,
        mut directive_budget: Option<&mut usize>,
    ) -> Result<char, ScanError> {
        let mut width = 0usize;
        let mut bytes = [0u8; 4];
        let mut bytes_len = 0usize;
        loop {
            self.input.lookahead(3);

            let c = self.input.peek_nth(1);
            let nc = self.input.peek_nth(2);

            let (true, Some(high), Some(low)) =
                (self.input.peek() == '%', c.to_digit(16), nc.to_digit(16))
            else {
                return Err(ScanError::from_kind(*mark, ErrorKind::InvalidTagEscape));
            };

            if let Some(budget) = directive_budget.as_deref_mut() {
                self.spend_directive_bytes(budget, 3, *mark)?;
            }

            let byte = u8::try_from((high << 4) + low)
                .expect("two hex nibbles always fit in a byte");
            if width == 0 {
                width = match byte {
                    _ if byte & 0x80 == 0x00 => 1,
                    _ if byte & 0xE0 == 0xC0 => 2,
                    _ if byte & 0xF0 == 0xE0 => 3,
                    _ if byte & 0xF8 == 0xF0 => 4,
                    _ => {
                        return Err(ScanError::from_kind(
                            *mark,
                            ErrorKind::InvalidTagUtf8LeadingByte,
                        ));
                    }
                };
            } else if byte & 0xc0 != 0x80 {
                return Err(ScanError::from_kind(
                    *mark,
                    ErrorKind::InvalidTagUtf8TrailingByte,
                ));
            }

            bytes[bytes_len] = byte;
            bytes_len += 1;

            self.skip_n_non_blank(3);

            width -= 1;
            if width == 0 {
                break;
            }
        }

        let s = core::str::from_utf8(&bytes[..bytes_len])
            .map_err(|_| ScanError::from_kind(*mark, ErrorKind::InvalidTagUtf8))?;

        Ok(s.chars()
            .next()
            .expect("a validated URI escape cannot decode to an empty string"))
    }

    fn fetch_anchor(&mut self, alias: bool) -> ScanResult {
        self.save_simple_key();
        self.disallow_simple_key();

        let tok = self.scan_anchor(alias)?;

        self.tokens.push_back(tok.into());

        Ok(())
    }

    fn scan_anchor(&mut self, alias: bool) -> Result<Token<'input>, ScanError> {
        let start_mark = self.mark;

        // Skip `&` / `*`.
        self.skip_non_blank();

        // Borrow from input when possible.
        if let Some(start) = self.input.byte_offset() {
            while is_anchor_char(self.input.look_ch()) {
                self.skip_non_blank();
            }

            let end = self
                .input
                .byte_offset()
                .expect("byte_offset() must remain available once enabled");

            if start == end {
                return Err(ScanError::from_kind(
                    start_mark,
                    ErrorKind::MissingAnchorOrAliasName,
                ));
            }

            let cow = if let Some(slice) = self.try_borrow_slice(start, end) {
                Cow::Borrowed(slice)
            } else if let Some(slice) = self.input.slice_bytes(start, end) {
                Cow::Owned(slice.to_owned())
            } else {
                return Err(ScanError::from_kind(
                    start_mark,
                    ErrorKind::InputSlicingUnavailable,
                ));
            };

            let tok = if alias {
                TokenType::Alias(cow)
            } else {
                TokenType::Anchor(cow)
            };
            return Ok(Token(Span::new(start_mark, self.mark), tok));
        }

        let mut string = String::new();
        while is_anchor_char(self.input.look_ch()) {
            string.push(self.input.peek());
            self.skip_non_blank();
        }

        if string.is_empty() {
            return Err(ScanError::from_kind(
                start_mark,
                ErrorKind::MissingAnchorOrAliasName,
            ));
        }

        let tok = if alias {
            TokenType::Alias(string.into())
        } else {
            TokenType::Anchor(string.into())
        };
        Ok(Token(Span::new(start_mark, self.mark), tok))
    }

    fn fetch_flow_collection_start(&mut self, tok: TokenType<'input>) -> ScanResult {
        // The indicators '[' and '{' may start a simple key.
        self.save_simple_key();

        let start_mark = self.mark;
        let indicator = self.input.peek();
        self.flow_markers.push((start_mark, indicator));

        self.roll_one_col_indent();
        self.increase_flow_level()?;

        self.allow_simple_key();

        self.skip_non_blank();
        let end_mark = self.mark;

        if tok == TokenType::FlowMappingStart {
            self.flow_mapping_started.push(true);
        } else {
            self.flow_mapping_started.push(false);
            self.implicit_flow_mapping_states
                .push(ImplicitMappingState::Possible);
        }

        let token_index = self.tokens.len();
        self.skip_ws_to_eol(SkipTabs::Yes)?;

        self.insert_token(token_index, Token(Span::new(start_mark, end_mark), tok));
        Ok(())
    }

    fn fetch_flow_collection_end(
        &mut self,
        tok: TokenType<'input>,
        expected_open: char,
        actual_close: char,
    ) -> ScanResult {
        // A closing bracket without a corresponding opening is invalid YAML.
        if self.flow_level == 0 {
            return Err(self.scan_error(ErrorKind::MisplacedFlowCollectionEnd));
        }

        let (open_mark, open_ch) = self
            .flow_markers
            .pop()
            .expect("each open flow collection has a marker");

        if open_ch != expected_open {
            return Err(ScanError::from_kind(
                open_mark,
                ErrorKind::MismatchedFlowCollectionEnd {
                    open: open_ch,
                    close: actual_close,
                },
            ));
        }

        let flow_level = self.flow_level;

        self.clear_simple_key();

        if matches!(tok, TokenType::FlowSequenceEnd) {
            self.end_implicit_mapping(self.mark, flow_level);
            // We are out exiting the flow sequence, nesting goes down 1 level.
            self.implicit_flow_mapping_states.pop();
        }
        self.flow_mapping_started.pop();

        self.decrease_flow_level();

        self.disallow_simple_key();

        let start_mark = self.mark;
        self.skip_non_blank();
        let end_mark = self.mark;
        let token_index = self.tokens.len();
        self.skip_ws_to_eol(SkipTabs::Yes)?;

        // A flow collection within a flow mapping can be a key. In that case, the value may be
        // adjacent to the `:`.
        // ```yaml
        // - [ {a: b}:value ]
        // ```
        if self.flow_level > 0 {
            self.adjacent_value_allowed_at = self.mark.index();
        }

        self.insert_token(token_index, Token(Span::new(start_mark, end_mark), tok));
        Ok(())
    }

    /// Push the `FlowEntry` token and skip over the `,`.
    fn fetch_flow_entry(&mut self) -> ScanResult {
        self.remove_simple_key()?;
        self.allow_simple_key();

        self.end_implicit_mapping(self.mark, self.flow_level);
        if self.current_flow_collection_is_sequence() {
            self.set_current_flow_mapping_started(false);
        }

        let start_mark = self.mark;
        self.skip_non_blank();
        let end_mark = self.mark;
        let token_index = self.tokens.len();
        self.skip_ws_to_eol(SkipTabs::Yes)?;

        self.insert_token(
            token_index,
            Token(Span::new(start_mark, end_mark), TokenType::FlowEntry),
        );
        Ok(())
    }

    fn increase_flow_level(&mut self) -> ScanResult {
        if self.flow_level >= self.options.flow_nesting_limit {
            return Err(self.scan_error(ErrorKind::RecursionLimitExceeded));
        }

        self.simple_keys.push(SimpleKey::new(Marker::new(0, 0, 0)));
        self.flow_level += 1;
        Ok(())
    }

    fn decrease_flow_level(&mut self) {
        // The closing delimiter removes this level's possible key before popping it.
        debug_assert!(!self.simple_keys.last().unwrap().possible);
        debug_assert_ne!(self.first_simple_key, Some(self.flow_level));
        self.flow_level -= 1;
        self.simple_keys.pop().unwrap();
    }

    /// Push the `Block*` token(s) and skip over the `-`.
    ///
    /// Add an indentation level and push a `BlockSequenceStart` token if needed, then push a
    /// `BlockEntry` token.
    /// This function only skips over the `-` and does not fetch the entry value.
    fn fetch_block_entry(&mut self) -> ScanResult {
        if self.flow_level > 0 {
            // - * only allowed in block
            return Err(self.scan_error(ErrorKind::BlockEntryInFlowCollection));
        }
        // Check if we are allowed to start a new entry.
        if !self.simple_key_allowed {
            return Err(self.scan_error(ErrorKind::BlockSequenceEntryNotAllowed));
        }

        // Skip over the `-`.
        let mark = self.mark;
        self.skip_non_blank();

        // generate BLOCK-SEQUENCE-START if indented
        self.roll_indent(mark.col, None, TokenType::BlockSequenceStart, mark)?;
        let token_index = self.tokens.len();
        let found_tabs = self.skip_ws_to_eol(SkipTabs::Yes)?.found_tabs();
        self.input.lookahead(2);
        if found_tabs
            && self.input.next_char_is('-')
            && is_blank_or_breakz(self.input.peek_nth(1))
        {
            return Err(self.scan_error(ErrorKind::InvalidBlockEntryWhitespace));
        }

        self.input.lookahead(1);
        if self.input.next_is_break() || self.input.next_is_flow() {
            self.roll_one_col_indent();
        }

        self.clear_simple_key();
        self.allow_simple_key();

        self.insert_token(
            token_index,
            Token(Span::empty(self.mark), TokenType::BlockEntry),
        );

        Ok(())
    }

    fn fetch_document_indicator(&mut self, t: TokenType<'input>) -> ScanResult {
        if let Some((mark, bracket)) = self.flow_markers.pop() {
            return Err(ScanError::from_kind(
                mark,
                ErrorKind::UnclosedFlowCollection { open: bracket },
            ));
        }

        self.unroll_indent(-1);
        self.clear_simple_key();
        self.disallow_simple_key();

        let mark = self.mark;

        self.skip_n_non_blank(3);

        self.document_prefix_allowed = matches!(t, TokenType::DocumentEnd);
        self.tokens
            .push_back(Token(Span::new(mark, self.mark), t).into());
        Ok(())
    }

    fn fetch_block_scalar(&mut self, literal: bool) -> ScanResult {
        self.save_simple_key();
        self.allow_simple_key();
        let token_index = self.tokens.len();
        let tok = self.scan_block_scalar(literal)?;

        self.insert_token(token_index, tok);
        Ok(())
    }

    /// Scan a block scalar header's chomping and indentation indicators, in either order.
    fn scan_block_scalar_indicators(
        &mut self,
        start_mark: Marker,
    ) -> Result<(Chomping, usize), ScanError> {
        let mut chomping = Chomping::Clip;
        let mut increment = 0;
        loop {
            match self.input.look_ch() {
                '+' if chomping == Chomping::Clip => chomping = Chomping::Keep,
                '-' if chomping == Chomping::Clip => chomping = Chomping::Strip,
                '0' if increment == 0 => {
                    return Err(ScanError::from_kind(
                        start_mark,
                        ErrorKind::ZeroBlockScalarIndent,
                    ));
                }
                c @ '1'..='9' if increment == 0 => {
                    increment = c as usize - '0' as usize;
                }
                _ => return Ok((chomping, increment)),
            }
            self.skip_non_blank();
        }
    }

    #[allow(clippy::too_many_lines)]
    fn scan_block_scalar(&mut self, literal: bool) -> Result<Token<'input>, ScanError> {
        let start_mark = self.mark;
        let mut indent: usize = 0;
        let mut trailing_blank: bool;
        let mut leading_blank: bool = false;
        let style = if literal {
            ScalarStyle::Literal
        } else {
            ScalarStyle::Folded
        };

        let mut string = String::new();
        let mut leading_break = String::new();
        let mut trailing_breaks = String::new();
        let mut chomping_break = String::new();

        // skip '|' or '>'
        self.skip_non_blank();
        self.unroll_non_block_indents();

        let (chomping, increment) = self.scan_block_scalar_indicators(start_mark)?;

        self.skip_ws_to_eol(SkipTabs::Yes)?;

        // Check if we are at the end of the line.
        self.input.lookahead(1);
        self.ensure_current_char_is_printable()?;
        if !self.input.next_is_breakz() {
            return Err(ScanError::from_kind(
                start_mark,
                ErrorKind::InvalidBlockScalarHeader,
            ));
        }

        if self.input.next_is_break() {
            self.input.lookahead(2);
            self.read_break(&mut chomping_break);
        }

        self.ensure_current_char_is_printable()?;

        if self.input.look_ch() == '\t' {
            return Err(ScanError::from_kind(
                start_mark,
                ErrorKind::TabAtBlockScalarStart,
            ));
        }

        if increment > 0 {
            indent = if self.indent >= 0 {
                (self.indent + increment as isize) as usize
            } else {
                increment
            }
        }

        // Scan the leading line breaks and determine the indentation level if needed.
        if indent == 0 {
            self.skip_block_scalar_first_line_indent(&mut indent, &mut trailing_breaks);
        } else {
            self.skip_block_scalar_indent(indent, &mut trailing_breaks);
        }

        self.ensure_current_char_is_printable()?;

        // We have an end-of-stream with no content, e.g.:
        // ```yaml
        // - |+
        // ```
        if self.input.next_is_z() {
            let contents = match chomping {
                // We strip trailing line breaks. Nothing remains.
                Chomping::Strip => String::new(),
                // There was no newline after the chomping indicator.
                _ if self.mark.line == start_mark.line() => String::new(),
                // With no content lines, the header break is not scalar content.
                Chomping::Clip => String::new(),
                // An indented whitespace-only line at EOF is an empty content line.
                Chomping::Keep if trailing_breaks.is_empty() && self.mark.col > 0 => {
                    chomping_break
                }
                // Keep actual empty content lines, if any, but not the header break.
                Chomping::Keep => trailing_breaks,
            };

            return Ok(Token(
                Span::new(start_mark, self.mark),
                TokenType::Scalar(style, contents.into()),
            ));
        }

        if self.mark.col < indent && (self.mark.col as isize) > self.indent {
            if self.indent < 0 && self.mark.col == 0 {
                self.input.lookahead(4);
                if self.input.next_is_document_start()
                    || self.input.next_is_document_end()
                    || self.input.peek() == '#'
                {
                    // At the root level, an explicit indentation indicator can still yield an
                    // empty scalar when the next line is a document marker or comment.
                    // In this case, the scalar is terminated rather than under-indented.
                } else {
                    return Err(self.scan_error(ErrorKind::InvalidBlockScalarIndent));
                }
            } else {
                return Err(self.scan_error(ErrorKind::InvalidBlockScalarIndent));
            }
        }

        let start_mark = self.mark;
        while self.mark.col == indent && !self.input.next_is_z() {
            self.ensure_current_char_is_printable()?;

            if indent == 0 {
                self.input.lookahead(4);
                if self.input.next_is_document_indicator() {
                    break;
                }
            }

            // We are at the first content character of a content line.
            trailing_blank = self.input.next_is_blank();
            if !literal
                && !leading_break.is_empty()
                && !leading_blank
                && !trailing_blank
            {
                string.push_str(&trailing_breaks);
                if trailing_breaks.is_empty() {
                    string.push(' ');
                }
            } else {
                string.push_str(&leading_break);
                string.push_str(&trailing_breaks);
            }

            leading_break.clear();
            trailing_breaks.clear();

            leading_blank = self.input.next_is_blank();

            self.scan_block_scalar_content_line(&mut string);

            // break on EOF
            self.input.lookahead(2);
            if self.input.next_is_z() {
                break;
            }

            self.ensure_current_char_is_printable()?;

            self.read_break(&mut leading_break);

            // Eat the following indentation spaces and line breaks.
            self.skip_block_scalar_indent(indent, &mut trailing_breaks);
        }

        self.ensure_current_char_is_printable()?;

        // Chomp the tail.
        if chomping != Chomping::Strip {
            // If we had reached an eof but the last content line wasn't terminated by a line
            // break, check if the last line was indented at least as the rest of the scalar, then
            // we need to consider there is a newline. When that line already ended with a break
            // (`leading_break`), the indentation-only remainder at EOF is not a line (it has no
            // break, so it is no `l-empty`) and adds nothing.
            if leading_break.is_empty()
                && self.input.next_is_z()
                && self.mark.col >= indent.max(1)
            {
                string.push('\n');
            }
            string.push_str(&leading_break);
        }

        if chomping == Chomping::Keep {
            string.push_str(&trailing_breaks);
        }

        if let Some(character) = find_non_printable(&string) {
            return Err(ScanError::from_kind(
                start_mark,
                ErrorKind::UnexpectedCharacter { character },
            ));
        }

        let span = if string.trim().is_empty() {
            Span::new(start_mark, self.mark)
        } else {
            Span::new(start_mark, self.mark).with_indent(Some(indent))
        };

        Ok(Token(span, TokenType::Scalar(style, string.into())))
    }

    /// Retrieve the contents of the line, parsing it as a block scalar.
    ///
    /// The input appends content directly to `string`, leaving the line break or NUL unconsumed.
    /// Callers handle indentation and line breaks and refresh lookahead after this operation.
    fn scan_block_scalar_content_line(&mut self, string: &mut String) {
        let chars_consumed = self.input.fetch_block_scalar_line(string);
        self.mark.col += chars_consumed;
        self.mark.offsets.chars += chars_consumed;
        self.mark.offsets.bytes = self.input.byte_offset();
        // Preserve leading_whitespace, as the per-character skip_blank path did. Reading the
        // following line break resets it before the scanner uses it again.
    }

    /// Skip the block scalar indentation and empty lines.
    fn skip_block_scalar_indent(&mut self, indent: usize, breaks: &mut String) {
        loop {
            self.skip_block_scalar_indent_spaces(indent);
            // An empty line's break is part of the scalar; a content line returns control.
            if !self.input.next_is_break() {
                break;
            }
            self.read_break(breaks);
        }
    }

    /// Consume up to `indent` spaces of one line. Tabs cannot be used as indentation.
    fn skip_block_scalar_indent_spaces(&mut self, indent: usize) {
        if indent < self.input.bufmaxlen().saturating_sub(2) {
            self.input.lookahead(self.input.bufmaxlen());
            while self.mark.col < indent && self.input.peek() == ' ' {
                self.skip_blank();
            }
            return;
        }
        loop {
            self.input.lookahead(self.input.bufmaxlen());
            while !self.input.buf_is_empty()
                && self.mark.col < indent
                && self.input.peek() == ' '
            {
                self.skip_blank();
            }
            // Stop at the indent, or at content or EOF: a non-empty buffer whose next character
            // is not a space.
            if self.mark.col == indent
                || (!self.input.buf_is_empty() && self.input.peek() != ' ')
            {
                break;
            }
        }
        self.input.lookahead(2);
    }

    /// Determine the indentation level for a block scalar from the first line of its contents.
    ///
    /// The function skips over whitespace-only lines and sets `indent` to the longest
    /// whitespace line that was encountered.
    fn skip_block_scalar_first_line_indent(
        &mut self,
        indent: &mut usize,
        breaks: &mut String,
    ) {
        let mut max_indent = 0;
        loop {
            // Consume all spaces. Tabs cannot be used as indentation.
            while self.input.look_ch() == ' ' {
                self.skip_blank();
            }

            if self.mark.col > max_indent {
                max_indent = self.mark.col;
            }

            if self.input.next_is_break() {
                // If our current line is empty, skip over the break and continue looping.
                self.input.lookahead(2);
                self.read_break(breaks);
            } else {
                // Otherwise, we have a content line. Return control.
                break;
            }
        }

        // In case a YAML document looks like:
        // ```yaml
        // |
        // foo
        // bar
        // ```
        // We need to set the indent to 0 and not 1. In all other cases, the indent must be at
        // least 1. When in the above example, `self.indent` will be set to -1.
        *indent = max_indent.max((self.indent + 1) as usize);
    }

    fn fetch_flow_scalar(&mut self, single: bool) -> ScanResult {
        self.save_simple_key();
        self.disallow_simple_key();

        let token_index = self.tokens.len();
        let tok = self.scan_flow_scalar(single)?;

        // From spec: To ensure JSON compatibility, if a key inside a flow mapping is JSON-like,
        // YAML allows the following value to be specified adjacent to the “:”.
        if self.skip_to_next_token()?.saw_comment {
            self.adjacent_value_allowed_at = usize::MAX;
        } else {
            self.adjacent_value_allowed_at = self.mark.index();
        }

        self.insert_token(token_index, tok);
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    fn scan_flow_scalar(&mut self, single: bool) -> Result<Token<'input>, ScanError> {
        let start_mark = self.mark;

        // Output scalar contents.
        let mut buf = match self.input.byte_offset() {
            Some(off) => {
                FlowScalarBuf::new_borrowed(off + self.input.peek().len_utf8())
            }
            None => FlowScalarBuf::new_owned(),
        };

        // Scratch used to consume the *first* line break in a break run without emitting it.
        // (The first break folds to ' ' or to nothing depending on escaping rules.)
        let mut break_scratch = String::new();

        /* Eat the left quote. */
        self.skip_non_blank();

        loop {
            /* Check for a document indicator. */
            self.input.lookahead(4);

            if self.mark.col == 0 && self.input.next_is_document_indicator() {
                return Err(ScanError::from_kind(
                    start_mark,
                    ErrorKind::DocumentIndicatorInQuotedScalar,
                ));
            }

            if self.input.next_is_z() {
                return Err(ScanError::from_kind(
                    start_mark,
                    ErrorKind::UnclosedQuotedScalar,
                ));
            }

            self.ensure_current_char_is_printable()?;

            // Consume content before handling continuation-line indentation below.
            let mut leading_blanks = false;
            self.consume_flow_scalar_non_whitespace_chars(
                single,
                &mut buf,
                &mut leading_blanks,
                &start_mark,
            )?;

            match self.input.look_ch() {
                '\'' if single && !leading_blanks => break,
                '"' if !single && !leading_blanks => break,
                _ => {}
            }

            let (has_leading_break, has_trailing_breaks) = self
                .consume_flow_scalar_whitespace(
                    &mut buf,
                    &mut leading_blanks,
                    &mut break_scratch,
                    &start_mark,
                )?;

            // Escaped line breaks also set `leading_blanks`, even without a folded break.
            // In strict flow context the continuation and closing quote must both be indented.
            if leading_blanks
                && self.options.strict_indentation
                && self.flow_level > 0
                && (self.mark.col as isize) <= self.flow_block_indent()
            {
                return Err(self.scan_error(ErrorKind::InvalidIndentation));
            }

            // Continuation lines of a quoted scalar in block context must be indented more than
            // the enclosing block (s-flow-line-prefix(n), YAML 1.2.2 [69]). Compare against that
            // block's indentation, not the temporary one-column indent added after `:` / `-`:
            // `foo: "a\n b"` is valid (n = 1). In strict mode this also applies to closing quotes.
            if leading_blanks && has_leading_break && self.flow_level == 0 {
                let next_ch = self.input.peek();
                let is_closing_quote =
                    (single && next_ch == '\'') || (!single && next_ch == '"');
                if (!is_closing_quote || self.options.strict_indentation)
                    && (self.mark.col as isize) <= self.flow_block_indent()
                {
                    return Err(self.scan_error(ErrorKind::InvalidQuotedScalarIndent));
                }
            }

            // An escaped line break (`\` at the end of a line) sets only `leading_blanks`, so the
            // block-context check above skips it. s-double-escaped [112] still ends in
            // s-flow-line-prefix(n): in strict mode the continuation must be indented past the
            // enclosing block, including before a closing quote, as in flow context.
            if leading_blanks
                && !has_leading_break
                && self.options.strict_indentation
                && self.flow_level == 0
                && (self.mark.col as isize) <= self.flow_block_indent()
            {
                return Err(self.scan_error(ErrorKind::InvalidIndentation));
            }

            // A single folded line break becomes one space. Without a break the pending
            // whitespace is already emitted, and further breaks were preserved as read.
            if leading_blanks && has_leading_break && !has_trailing_breaks {
                self.promote_flow_scalar_buf_to_owned(&start_mark, &mut buf)?
                    .push(' ');
            }
        }

        // Eat the right quote.
        self.skip_non_blank();
        let end_mark = self.mark;

        // Ensure there is no invalid trailing content.
        self.skip_ws_to_eol(SkipTabs::Yes)?;
        self.ensure_current_char_is_printable()?;
        match self.input.peek() {
            // These can be encountered in flow sequences or mappings.
            ',' | '}' | ']' if self.flow_level > 0 => {}
            // An end-of-line / end-of-stream is fine. No trailing content.
            c if is_breakz(c) => {}
            // ':' can be encountered if our scalar is a key.
            // Outside of flow contexts, keys cannot span multiple lines
            ':' if self.flow_level == 0 && start_mark.line == self.mark.line => {}
            // Inside a flow context, this is allowed.
            ':' if self.flow_level > 0 => {}
            _ => {
                let kind = if single {
                    ErrorKind::InvalidTrailingSingleQuotedScalar
                } else {
                    ErrorKind::InvalidTrailingDoubleQuotedScalar
                };
                return Err(self.scan_error(kind));
            }
        }

        let style = if single {
            ScalarStyle::SingleQuoted
        } else {
            ScalarStyle::DoubleQuoted
        };

        let contents = if let Some(string) = buf.owned {
            Cow::Owned(string)
        } else {
            // A trailing whitespace run is part of the output when no break followed it.
            buf.commit_pending_ws();
            let (start, end) = (buf.start, buf.end);
            if let Some(slice) = self.try_borrow_slice(start, end) {
                Cow::Borrowed(slice)
            } else {
                let slice = self.input.slice_bytes(start, end).ok_or_else(|| {
                    ScanError::from_kind(
                        start_mark,
                        ErrorKind::InputOffsetsWithoutSlice,
                    )
                })?;
                Cow::Owned(slice.to_owned())
            }
        };

        Ok(Token(
            Span::new(start_mark, end_mark),
            TokenType::Scalar(style, contents),
        ))
    }

    /// Consume a printable ASCII run without per-character buffer and marker updates.
    fn consume_flow_scalar_ascii_chunk(
        &mut self,
        single: bool,
        buf: &mut FlowScalarBuf,
    ) -> bool {
        let chunk = self.input.take_quoted_scalar_ascii_chunk(single);
        let consumed = chunk.len();
        if consumed == 0 {
            return false;
        }

        match &mut buf.owned {
            Some(string) => string.push_str(chunk),
            None => buf.commit_pending_ws(),
        }

        // The input already consumed the run; all its characters are ASCII.
        self.mark.offsets.chars += consumed;
        self.mark.col += consumed;
        self.mark.offsets.bytes = self.input.byte_offset();
        self.leading_whitespace = false;
        buf.end = self.mark.offsets.bytes.unwrap_or(buf.end);
        true
    }

    /// Consume successive non-whitespace characters from a flow scalar.
    ///
    /// This function resolves escape sequences and stops upon encountering a whitespace, the end
    /// of the stream or the closing character for the scalar (`'` for single quoted scalars, `"`
    /// for double quoted scalars).
    ///
    /// # Errors
    /// Return an error if an invalid escape sequence is found.
    fn consume_flow_scalar_non_whitespace_chars(
        &mut self,
        single: bool,
        buf: &mut FlowScalarBuf,
        leading_blanks: &mut bool,
        start_mark: &Marker,
    ) -> Result<(), ScanError> {
        self.input.lookahead(2);
        loop {
            let character = self.input.peek();
            if is_blank_or_breakz(character) || !is_printable(character) {
                break;
            }
            match character {
                // Check for an escaped single quote.
                '\'' if self.input.peek_nth(1) == '\'' && single => {
                    buf.commit_pending_ws();
                    self.promote_flow_scalar_buf_to_owned(start_mark, buf)?
                        .push('\'');
                    self.skip_n_non_blank(2);
                }
                // Check for the right quote.
                '\'' if single => break,
                '"' if !single => break,
                // Check for an escaped line break.
                '\\' if !single && is_break(self.input.peek_nth(1)) => {
                    self.input.lookahead(3);
                    buf.commit_pending_ws();
                    self.promote_flow_scalar_buf_to_owned(start_mark, buf)?;
                    self.skip_non_blank();
                    self.skip_linebreak();
                    *leading_blanks = true;
                    break;
                }
                // Check for an escape sequence.
                '\\' if !single => {
                    buf.commit_pending_ws();
                    let string =
                        self.promote_flow_scalar_buf_to_owned(start_mark, buf)?;
                    string.push(self.resolve_flow_scalar_escape_sequence(start_mark)?);
                }
                c => {
                    if c.is_ascii() && self.consume_flow_scalar_ascii_chunk(single, buf)
                    {
                        self.input.lookahead(2);
                        continue;
                    }
                    match &mut buf.owned {
                        Some(string) => string.push(c),
                        None => buf.commit_pending_ws(),
                    }
                    self.skip_non_blank();

                    if let Some(new_end) = self.input.byte_offset() {
                        buf.end = new_end;
                    }
                }
            }
            self.input.lookahead(2);
        }
        Ok(())
    }

    /// Escape the sequence we encounter in a flow scalar.
    ///
    /// `self.input.peek()` must point to the `\` starting the escape sequence.
    ///
    /// # Errors
    /// Return an error if an invalid escape sequence is found.
    fn resolve_flow_scalar_escape_sequence(
        &mut self,
        start_mark: &Marker,
    ) -> Result<char, ScanError> {
        let code_length = match self.input.peek_nth(1) {
            'x' => 2,
            'u' => 4,
            'U' => 8,
            c => {
                let ch = match c {
                    '0' => '\0',
                    'a' => '\x07',
                    'b' => '\x08',
                    't' | '\t' => '\t',
                    'n' => '\n',
                    'v' => '\x0b',
                    'f' => '\x0c',
                    'r' => '\x0d',
                    'e' => '\x1b',
                    ' ' => '\x20',
                    '"' => '"',
                    '/' => '/',
                    '\\' => '\\',
                    'N' => '\u{85}',
                    '_' => '\u{A0}',
                    'L' => '\u{2028}',
                    'P' => '\u{2029}',
                    _ => {
                        return Err(ScanError::from_kind(
                            *start_mark,
                            ErrorKind::UnknownQuotedScalarEscape,
                        ));
                    }
                };
                self.skip_n_non_blank(2);
                return Ok(ch);
            }
        };
        self.skip_n_non_blank(2);

        let mut value = self.scan_hex_escape(
            code_length,
            start_mark,
            ErrorKind::InvalidQuotedScalarHexEscape,
        )?;
        match value {
            0xD800..=0xDBFF if code_length == 4 => {
                value = self.scan_low_surrogate(value, start_mark)?;
            }
            0xDC00..=0xDFFF if code_length == 4 => {
                return Err(ScanError::from_kind(
                    *start_mark,
                    ErrorKind::UnpairedLowSurrogate,
                ));
            }
            _ => {}
        }
        char::from_u32(value).ok_or_else(|| {
            ScanError::from_kind(*start_mark, ErrorKind::InvalidUnicodeEscape)
        })
    }

    /// Consume `len` hex digits, failing with `kind` on any other character.
    fn scan_hex_escape(
        &mut self,
        len: usize,
        start_mark: &Marker,
        kind: ErrorKind,
    ) -> Result<u32, ScanError> {
        let value = self.peek_hex_escape(len, start_mark, kind)?;
        self.skip_n_non_blank(len);
        Ok(value)
    }

    /// Read `len` hex digits without consuming them, failing with `kind` on any other character.
    fn peek_hex_escape(
        &mut self,
        len: usize,
        start_mark: &Marker,
        kind: ErrorKind,
    ) -> Result<u32, ScanError> {
        self.input.lookahead(len);
        let mut value = 0u32;
        for i in 0..len {
            let Some(digit) = self.input.peek_nth(i).to_digit(16) else {
                return Err(ScanError::from_kind(*start_mark, kind));
            };
            value = (value << 4) + digit;
        }
        Ok(value)
    }

    /// Combine a JSON-style high surrogate with the `\uXXXX` low surrogate that must follow it.
    fn scan_low_surrogate(
        &mut self,
        high: u32,
        start_mark: &Marker,
    ) -> Result<u32, ScanError> {
        self.input.lookahead(2);
        if !(self.input.peek() == '\\' && self.input.peek_nth(1) == 'u') {
            return Err(ScanError::from_kind(
                *start_mark,
                ErrorKind::MissingLowSurrogate,
            ));
        }
        self.skip_n_non_blank(2);
        // Validate before consuming, so a later source error cannot replace this one.
        let low = self.peek_hex_escape(
            4,
            start_mark,
            ErrorKind::InvalidLowSurrogateHexEscape,
        )?;
        if !(0xDC00..=0xDFFF).contains(&low) {
            return Err(ScanError::from_kind(
                *start_mark,
                ErrorKind::InvalidLowSurrogate,
            ));
        }
        self.skip_n_non_blank(4);
        Ok(0x10000 + (((high - 0xD800) << 10) | (low - 0xDC00)))
    }

    /// The source text of a borrowed plain scalar from `start` to `end_mark`.
    fn borrowed_plain_scalar(
        &self,
        start: Option<usize>,
        end_mark: Marker,
        start_mark: Marker,
    ) -> Result<&'input str, ScanError> {
        let start = start.expect("borrowed scalar has a start offset");
        // Lending an empty slice does not oblige an input to lend the whole scalar.
        end_mark
            .byte_offset()
            .and_then(|end| self.try_borrow_slice(start, end))
            .ok_or_else(|| {
                ScanError::from_kind(start_mark, ErrorKind::InputOffsetsWithoutSlice)
            })
    }

    /// Append the blanks between two content runs on one plain scalar line to `output`.
    fn join_plain_scalar_blanks(&mut self, output: Option<&mut String>) {
        if let Some(output) = output {
            output.push_str(&self.buf_whitespaces);
        }
        self.buf_whitespaces.clear();
    }

    /// Fold the line breaks before a plain scalar's continuation line into `output`.
    fn fold_plain_scalar_breaks(&mut self, output: Option<&mut String>) {
        if let Some(output) = output {
            if !self.buf_leading_break.is_empty() && self.buf_trailing_breaks.is_empty()
            {
                output.push(' ');
            } else {
                output.push_str(&self.buf_trailing_breaks);
            }
        }
        self.buf_trailing_breaks.clear();
        self.buf_leading_break.clear();
        self.leading_whitespace = false;
    }

    /// Consume plain scalar content up to the next blank, break, or indicator.
    fn scan_plain_scalar_content(&mut self, mut output: Option<&mut String>) {
        loop {
            // Fill the buffer once and process all characters in it until the next fetch.
            // `next_can_be_plain_scalar` needs 2 lookahead characters, so keep one spare slot
            // for normal inputs while still forcing progress for very small buffer lengths.
            self.input.lookahead(self.input.bufmaxlen());
            let chunk_len = self.input.bufmaxlen().saturating_sub(1).max(1);
            let (stop, chars_consumed) = if let Some(output) = output.as_deref_mut() {
                self.input.fetch_plain_scalar_chunk(
                    output,
                    chunk_len,
                    self.flow_level > 0,
                )
            } else {
                self.input
                    .skip_plain_scalar_chunk(chunk_len, self.flow_level > 0)
            };
            self.mark.offsets.chars += chars_consumed;
            self.mark.col += chars_consumed;
            self.mark.offsets.bytes = self.input.byte_offset();
            if stop {
                break;
            }
        }
    }

    /// Skip the blanks and line breaks after plain scalar content, recording them for folding.
    fn skip_plain_scalar_blanks(
        &mut self,
        indent: isize,
        start_mark: Marker,
    ) -> ScanResult {
        self.input.lookahead(2);
        while self.input.next_is_blank_or_break() {
            if !self.input.next_is_blank() {
                // `skip_break` sets `leading_whitespace`, so read it first.
                if self.leading_whitespace {
                    self.buf_trailing_breaks.push('\n');
                } else {
                    self.buf_whitespaces.clear();
                    self.buf_leading_break.push('\n');
                }
                self.skip_break();
                self.leading_whitespace = true;
            } else if !self.leading_whitespace {
                self.buf_whitespaces.push(self.input.peek());
                self.skip_blank();
            } else if (self.mark.col as isize) < indent && self.input.peek() == '\t' {
                // A tab in the indentation columns is allowed only on an otherwise empty line.
                self.skip_ws_to_eol(SkipTabs::Yes)?;
                if !self.input.next_is_breakz() {
                    return Err(ScanError::from_kind(
                        start_mark,
                        ErrorKind::TabInPlainScalar,
                    ));
                }
            } else {
                self.skip_blank();
            }
            self.input.lookahead(2);
        }
        Ok(())
    }

    fn fetch_plain_scalar(&mut self) -> ScanResult {
        self.save_simple_key();
        self.disallow_simple_key();

        let token_index = self.tokens.len();
        let tok = self.scan_plain_scalar()?;

        self.insert_token(token_index, tok);
        Ok(())
    }

    /// Scan for a plain scalar.
    ///
    /// Plain scalars are the most readable but restricted style. They may span multiple lines in
    /// some contexts.
    #[allow(clippy::too_many_lines)]
    fn scan_plain_scalar(&mut self) -> Result<Token<'input>, ScanError> {
        self.unroll_non_block_indents();
        let indent = self.indent + 1;
        let start_mark = self.mark;

        let borrow_start = start_mark
            .byte_offset()
            .filter(|start| self.try_borrow_slice(*start, *start).is_some());
        let mut string = borrow_start.is_none().then(|| String::with_capacity(32));
        let mut has_content = false;
        self.buf_whitespaces.clear();
        self.buf_leading_break.clear();
        self.buf_trailing_breaks.clear();
        let mut end_mark = self.mark;

        loop {
            self.input.lookahead(4);
            if (self.mark.col == 0 && self.input.next_is_document_indicator())
                || self.input.peek() == '#'
            {
                // BS4K: If a `#` starts a comment after some separation spaces following content
                // of a plain scalar in block context, and there is potential continuation on the
                // next line, this is invalid. We cannot decide yet if there will be continuation,
                // so record that a comment interrupted a plain scalar.
                if self.input.peek() == '#'
                    && has_content
                    && !self.buf_whitespaces.is_empty()
                    && self.flow_level == 0
                {
                    self.interrupted_plain_by_comment = Some(self.mark);
                }
                break;
            }

            // The dash restriction applies only at the start of a plain scalar.
            if !has_content
                && self.flow_level > 0
                && self.input.peek() == '-'
                && is_flow(self.input.peek_nth(1))
            {
                return Err(
                    self.scan_error(ErrorKind::PlainScalarStartsWithDashFlowIndicator)
                );
            }

            if !self.input.next_is_blank_or_breakz()
                && self.input.next_can_be_plain_scalar(self.flow_level > 0)
            {
                // A continuation line can't be borrowed: folding transforms the source.
                if self.leading_whitespace && has_content && string.is_none() {
                    let prefix =
                        self.borrowed_plain_scalar(borrow_start, end_mark, start_mark)?;
                    string = Some(prefix.to_owned());
                }
                if self.leading_whitespace {
                    self.fold_plain_scalar_breaks(string.as_mut());
                } else {
                    self.join_plain_scalar_blanks(string.as_mut());
                }

                // We can unroll the first iteration of the loop.
                has_content = true;
                if let Some(output) = string.as_mut() {
                    output.push(self.input.peek());
                }
                self.skip_non_blank();
                if let Some(output) = string.as_mut() {
                    output.reserve(self.input.bufmaxlen());
                }

                self.scan_plain_scalar_content(string.as_mut());
                end_mark = self.mark;
            }

            // We may reach the end of a plain scalar if:
            //  - We reach eof
            //  - We reach ": "
            //  - We find a flow character in a flow context
            if !(self.input.next_is_blank() || self.input.next_is_break()) {
                break;
            }

            self.skip_plain_scalar_blanks(indent, start_mark)?;

            // Stop before an under-indented continuation. In strict flow context the
            // next token fetch reports its indentation error without consuming the content.
            if (self.flow_level == 0 || self.options.strict_indentation)
                && (self.mark.col as isize) < indent
            {
                break;
            }
        }

        if self.leading_whitespace {
            self.allow_simple_key();
        }

        let borrowed_contents = if string.is_none() {
            Some(self.borrowed_plain_scalar(borrow_start, end_mark, start_mark)?)
        } else {
            None
        };
        let scalar_text = borrowed_contents.unwrap_or_else(|| {
            string
                .as_deref()
                .expect("owned plain scalar has an output buffer")
        });
        if let Some(character) = find_non_printable(scalar_text) {
            return Err(ScanError::from_kind(
                start_mark,
                ErrorKind::UnexpectedCharacter { character },
            ));
        }
        self.ensure_current_char_is_printable()?;

        // A custom input's `next_can_be_plain_scalar` may reject the first character; failing
        // here guarantees progress, since `fetch_next_token` retries until something is consumed.
        if !has_content {
            return Err(ScanError::from_kind(
                start_mark,
                ErrorKind::UnexpectedEndOfPlainScalar,
            ));
        }
        let contents = if let Some(slice) = borrowed_contents {
            Cow::Borrowed(slice)
        } else {
            Cow::Owned(string.expect("owned plain scalar has an output buffer"))
        };
        Ok(Token(
            Span::new(start_mark, end_mark),
            TokenType::Scalar(ScalarStyle::Plain, contents),
        ))
    }

    fn fetch_key(&mut self) -> ScanResult {
        let start_mark = self.mark;
        if self.flow_level == 0 {
            // Check if we are allowed to start a new key (not necessarily simple).
            if !self.simple_key_allowed {
                return Err(self.scan_error(ErrorKind::MappingKeyNotAllowed));
            }
            self.roll_indent(
                start_mark.col,
                None,
                TokenType::BlockMappingStart,
                start_mark,
            )?;
        } else {
            // The scanner, upon emitting a `Key`, will prepend a `MappingStart` event.
            self.set_current_flow_mapping_started(true);
        }

        self.clear_simple_key();

        if self.flow_level == 0 {
            self.allow_simple_key();
        } else {
            self.disallow_simple_key();
        }

        self.skip_non_blank();
        let end_mark = self.mark;
        let token_index = self.tokens.len();
        self.explicit_key_tab_check_pending = false;
        // Block keys permit tab separation for scalar/flow nodes, but require spaces for
        // compact collections and indentation on subsequent lines. Keep that validation active
        // across comments, and across properties in strict mode. Flow collections use the
        // ordinary whitespace skipper.
        if self.flow_level == 0 {
            let stopped_after_comment = self.skip_yaml_whitespace()?;
            self.explicit_key_tab_check_pending = stopped_after_comment
                || (self.options.strict_indentation
                    && matches!(self.input.peek(), '&' | '!'));
        }
        self.insert_token(
            token_index,
            Token(Span::new(start_mark, end_mark), TokenType::Key),
        );
        Ok(())
    }

    /// Fetch a value in a mapping inside of a flow collection.
    ///
    /// This must not be called if [`self.flow_level`] is 0. This ensures the rules surrounding
    /// values in flow collections are respected prior to calling [`fetch_value`].
    ///
    /// [`self.flow_level`]: Self::flow_level
    /// [`fetch_value`]: Self::fetch_value
    fn fetch_flow_value(&mut self) -> ScanResult {
        let nc = self.input.peek_nth(1);

        // If we encounter a ':' inside a flow collection and it is not immediately
        // followed by a blank or breakz:
        //   - We must check whether an adjacent value is allowed
        //     `["a":[]]` is valid. If the key is double-quoted, no need for a space. This
        //     is needed for JSON compatibility.
        //   - If not, we must ensure there is a space after the ':' and before its value.
        //     `[a: []]` is valid while `[a:[]]` isn't. `[a:b]` is treated as `["a:b"]`.
        //   - But if the value is empty (null), then it's okay.
        // The last line is for YAMLs like `[a:]`. The ':' is followed by a ']' (which is a
        // flow character), but the ']' is not the value. The value is an invisible empty
        // space which is represented as null ('~').
        if self.mark.index() != self.adjacent_value_allowed_at
            && (nc == '[' || nc == '{')
        {
            return Err(self.scan_error(ErrorKind::FlowMappingValueAdjacentCollection));
        }

        self.fetch_value()
    }

    /// Fetch a value from a mapping (after a `:`).
    fn fetch_value(&mut self) -> ScanResult {
        let sk = *self.simple_keys.last().unwrap();
        let start_mark = self.mark;
        let is_implicit_flow_mapping = self.current_flow_collection_is_sequence()
            && !self.current_flow_mapping_started()
            && !self.implicit_flow_mapping_states.is_empty();
        if is_implicit_flow_mapping {
            *self.implicit_flow_mapping_states.last_mut().unwrap() =
                ImplicitMappingState::Inside(self.flow_level);
        }

        // Skip over ':'.
        self.skip_non_blank();
        // Tabs after ':' are separation whitespace. Queue any following comment after the
        // Value token; indentation on subsequent lines is checked by `skip_to_next_token`.
        let mut trailing_tokens = VecDeque::new();
        let mut found_tabs = false;
        if matches!(self.input.look_ch(), ' ' | '\t') {
            let trailing_token_index = self.tokens.len();
            found_tabs = self.skip_ws_to_eol(SkipTabs::Yes)?.found_tabs();
            trailing_tokens = self.tokens.split_off(trailing_token_index);
        }

        if sk.possible {
            // Under-indented flow entries are a compatibility extension, but implicit keys
            // spanning lines are rejected by both PyYAML and ruamel.yaml. Keep that extension
            // limited to single-line keys; explicitly marked `?` keys may span lines.
            let block_indent = self.flow_block_indent();
            if self.flow_level > 0
                && sk.mark.line < start_mark.line
                && (sk.mark.col as isize) <= block_indent
            {
                return Err(ScanError::from_kind(
                    start_mark,
                    ErrorKind::InvalidColonPlacement,
                ));
            }
            let token_index = self.simple_key_token_index(&sk);
            self.validate_flow_key_indentation(&sk, token_index, start_mark)?;
            // insert simple key
            let tok = Token(Span::empty(sk.mark), TokenType::Key);
            self.insert_token(token_index, tok);
            if is_implicit_flow_mapping {
                if sk.mark.line < start_mark.line {
                    return Err(ScanError::from_kind(
                        start_mark,
                        ErrorKind::InvalidColonPlacement,
                    ));
                }
                self.insert_token(
                    token_index,
                    Token(Span::empty(sk.mark), TokenType::FlowMappingStart),
                );
            }

            // Add the BLOCK-MAPPING-START token if needed.
            self.roll_indent(
                sk.mark.col,
                Some(sk.token_number),
                TokenType::BlockMappingStart,
                sk.mark,
            )?;
            self.roll_one_col_indent();

            self.clear_simple_key();
            self.disallow_simple_key();
        } else {
            if is_implicit_flow_mapping {
                self.tokens.push_back(
                    Token(Span::empty(start_mark), TokenType::FlowMappingStart).into(),
                );
            }
            // The ':' indicator follows a complex key.
            if self.flow_level == 0 {
                if !self.simple_key_allowed {
                    return Err(ScanError::from_kind(
                        start_mark,
                        ErrorKind::MappingValueNotAllowed,
                    ));
                }

                self.roll_indent(
                    start_mark.col,
                    None,
                    TokenType::BlockMappingStart,
                    start_mark,
                )?;
            }
            self.roll_one_col_indent();

            // An explicit value may start a compact block collection on this line only when
            // its indentation contains only spaces. Tabs still separate scalar and flow values.
            if self.flow_level == 0 && !found_tabs {
                self.allow_simple_key();
            } else {
                self.disallow_simple_key();
            }
        }
        self.tokens
            .push_back(Token(Span::empty(start_mark), TokenType::Value).into());
        self.tokens.append(&mut trailing_tokens);

        Ok(())
    }

    /// Keep under-indented implicit flow keys on one line, including their properties.
    fn validate_flow_key_indentation(
        &self,
        key: &SimpleKey,
        token_index: usize,
        colon: Marker,
    ) -> ScanResult {
        if self.flow_level == 0 || key.mark.line == colon.line {
            return Ok(());
        }

        let block_indent = self.flow_block_indent();
        // The saved key mark may point to an anchor or tag before the actual content.
        // Check each property and the first content token; comments do not indent a key.
        // Stop at the content token so a collection key's children are not checked here.
        let mut under_indented = [key.mark.col, colon.col]
            .iter()
            .any(|&col| (col as isize) <= block_indent);
        for token in self.tokens.iter().skip(token_index) {
            if matches!(token.1, QueuedTokenType::Comment(_)) {
                continue;
            }
            under_indented |= (token.0.start.col as isize) <= block_indent;
            if !matches!(
                token.1,
                QueuedTokenType::Anchor(_) | QueuedTokenType::Tag(..)
            ) {
                break;
            }
        }
        if under_indented {
            return Err(ScanError::from_kind(
                colon,
                ErrorKind::InvalidColonPlacement,
            ));
        }
        Ok(())
    }

    /// Return the enclosing block indentation, excluding a temporary one-column indent.
    fn flow_block_indent(&self) -> isize {
        self.indents
            .last()
            .filter(|indent| !indent.needs_block_end)
            .map_or(self.indent, |indent| indent.indent)
    }

    /// Add an indentation level to the stack with the given block token, if needed.
    ///
    /// An indentation level is added only if:
    ///   - We are not in a flow-style construct (which don't have indentation per-se).
    ///   - The current column is further indented than the last indent we have registered.
    fn roll_indent(
        &mut self,
        col: usize,
        number: Option<usize>,
        tok: TokenType<'input>,
        mark: Marker,
    ) -> ScanResult {
        if self.flow_level > 0 {
            return Ok(());
        }

        // If the last indent was a non-block indent, remove it.
        // This means that we prepared an indent that we thought we wouldn't use, but realized just
        // now that it is a block indent.
        if self.indent <= col as isize
            && let Some(indent) = self.indents.last()
            && !indent.needs_block_end
        {
            self.indent = indent.indent;
            self.indents.pop();
        }

        if self.indent < col as isize {
            if self.block_level >= self.options.block_nesting_limit {
                return Err(ScanError::from_kind(
                    mark,
                    ErrorKind::RecursionLimitExceeded,
                ));
            }
            self.indents.push(Indent {
                indent: self.indent,
                needs_block_end: true,
            });
            self.block_level += 1;
            self.indent = col as isize;
            let tokens_parsed = self.tokens_parsed;
            match number {
                Some(n) => {
                    self.insert_token(n - tokens_parsed, Token(Span::empty(mark), tok));
                }
                None => self.tokens.push_back(Token(Span::empty(mark), tok).into()),
            }
        }
        Ok(())
    }

    /// Pop indentation levels from the stack as much as needed.
    ///
    /// Indentation levels are popped from the stack while they are further indented than `col`.
    /// If we are in a flow-style construct (which don't have indentation per-se), this function
    /// does nothing.
    fn unroll_indent(&mut self, col: isize) {
        if self.flow_level > 0 {
            return;
        }
        while self.indent > col {
            let indent = self.indents.pop().unwrap();
            self.indent = indent.indent;
            if indent.needs_block_end {
                debug_assert!(self.block_level > 0);
                self.block_level -= 1;
                self.tokens.push_back(
                    Token(Span::empty(self.mark), TokenType::BlockEnd).into(),
                );
            }
        }
    }

    /// Add an indentation level of 1 column that does not start a block.
    ///
    /// See the documentation of [`Indent::needs_block_end`] for more details.
    /// An indentation is not added if we are inside a flow level or if the last indent is already
    /// a non-block indent.
    fn roll_one_col_indent(&mut self) {
        if self.flow_level == 0
            && self.indents.last().is_some_and(|x| x.needs_block_end)
        {
            self.indents.push(Indent {
                indent: self.indent,
                needs_block_end: false,
            });
            self.indent += 1;
        }
    }

    /// Unroll all last indents created with [`Self::roll_one_col_indent`].
    fn unroll_non_block_indents(&mut self) {
        while let Some(indent) = self.indents.last() {
            if indent.needs_block_end {
                break;
            }
            self.indent = indent.indent;
            self.indents.pop();
        }
    }

    /// Mark the next token to be inserted as a potential simple key.
    fn save_simple_key(&mut self) {
        if self.simple_key_allowed {
            let required = self.flow_level == 0
                && self.indent == (self.mark.col as isize)
                && self.indents.last().unwrap().needs_block_end;

            *self
                .simple_keys
                .last_mut()
                .expect("every flow level has a simple-key slot") = SimpleKey {
                mark: self.mark,
                possible: true,
                required,
                token_number: self.tokens_parsed + self.tokens.len(),
            };
            self.first_simple_key.get_or_insert(self.flow_level);
        }
    }

    fn remove_simple_key(&mut self) -> ScanResult {
        let last = self.simple_keys.last().unwrap();
        if last.possible && last.required {
            return Err(Self::simple_key_expected(last.mark));
        }

        self.clear_simple_key();
        Ok(())
    }

    /// Clear the current key after it is resolved or checked to be optional.
    fn clear_simple_key(&mut self) {
        self.simple_keys.last_mut().unwrap().possible = false;
        if self.first_simple_key == Some(self.flow_level) {
            // This is the top level, so no other possible keys follow it.
            self.first_simple_key = None;
        }
    }

    /// Return whether the scanner is inside a block but outside of a flow sequence.
    fn is_within_block(&self) -> bool {
        !self.indents.is_empty()
    }

    /// If an implicit mapping had started, end it.
    ///
    /// This function does not pop the state in [`implicit_flow_mapping_states`].
    ///
    /// [`implicit_flow_mapping_states`]: Self::implicit_flow_mapping_states
    fn end_implicit_mapping(&mut self, mark: Marker, flow_level: usize) {
        if self
            .implicit_flow_mapping_states
            .last()
            .is_some_and(|state| *state == ImplicitMappingState::Inside(flow_level))
        {
            *self.implicit_flow_mapping_states.last_mut().unwrap() =
                ImplicitMappingState::Possible;
            self.set_current_flow_mapping_started(false);
            self.tokens
                .push_back(Token(Span::empty(mark), TokenType::FlowMappingEnd).into());
        }
    }

    fn current_flow_collection_is_sequence(&self) -> bool {
        self.flow_markers
            .last()
            .is_some_and(|(_, bracket)| *bracket == '[')
    }

    fn current_flow_mapping_started(&self) -> bool {
        self.flow_mapping_started.last().copied().unwrap_or(false)
    }

    fn set_current_flow_mapping_started(&mut self, started: bool) {
        *self
            .flow_mapping_started
            .last_mut()
            .expect("only called inside a flow collection") = started;
    }
}

/// Chomping, how final line breaks and trailing empty lines are interpreted.
///
/// See YAML spec 8.1.1.2.
#[derive(PartialEq, Eq)]
enum Chomping {
    /// The final line break and any trailing empty lines are excluded.
    Strip,
    /// The final line break is preserved, but trailing empty lines are excluded.
    Clip,
    /// The final line break and trailing empty lines are included.
    Keep,
}

impl TokenType<'_> {
    /// Borrow lexical evidence without cloning decoded values.
    #[must_use]
    #[inline]
    pub fn kind(&self) -> crate::TokenKind<'_> {
        match self {
            Self::StreamStart => crate::TokenKind::StreamStart,
            Self::StreamEnd => crate::TokenKind::StreamEnd,
            Self::VersionDirective(..) => crate::TokenKind::VersionDirective,
            Self::TagDirective(..) => crate::TokenKind::TagDirective,
            Self::DocumentStart => crate::TokenKind::DocumentStart,
            Self::DocumentEnd => crate::TokenKind::DocumentEnd,
            Self::BlockSequenceStart => crate::TokenKind::BlockSequenceStart,
            Self::BlockMappingStart => crate::TokenKind::BlockMappingStart,
            Self::BlockEnd => crate::TokenKind::BlockEnd,
            Self::FlowSequenceStart => crate::TokenKind::FlowSequenceStart,
            Self::FlowSequenceEnd => crate::TokenKind::FlowSequenceEnd,
            Self::FlowMappingStart => crate::TokenKind::FlowMappingStart,
            Self::FlowMappingEnd => crate::TokenKind::FlowMappingEnd,
            Self::BlockEntry => crate::TokenKind::BlockEntry,
            Self::FlowEntry => crate::TokenKind::FlowEntry,
            Self::Key => crate::TokenKind::Key,
            Self::Value => crate::TokenKind::Value,
            Self::Alias(..) => crate::TokenKind::Alias,
            Self::Anchor(..) => crate::TokenKind::Anchor,
            Self::Tag(..) => crate::TokenKind::Tag,
            Self::ReservedDirective(..) => crate::TokenKind::ReservedDirective,
            Self::Scalar(style, value) => crate::TokenKind::Scalar {
                style: *style,
                value: value.as_ref(),
            },
            Self::Comment(comment) => crate::TokenKind::Comment {
                text: comment.text(),
                placement: comment.placement(),
            },
        }
    }
}
