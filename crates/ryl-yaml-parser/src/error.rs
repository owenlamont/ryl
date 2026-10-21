//! Parser and scanner error types.

#[cfg(feature = "std")]
use alloc::sync::Arc;
use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use core::fmt;

use crate::scanner::Marker;

/// Details of an I/O failure reported by an input adapter.
///
/// This error is primarily intended for terminal failures such as a missing file, insufficient
/// permissions, or a failed read, where an exact character position is usually not meaningful.
/// Streaming inputs may be read ahead by a small lookahead window. Once an adapter reports an I/O
/// failure, the parser reports it at its current marker; consequently, a few successfully read
/// characters that were already buffered ahead of that marker may not be scanned or emitted.
///
/// The human-readable message is available in every build. With the `std` feature enabled, an
/// instance constructed from `std::io::Error` also retains that original error and exposes it
/// through `InputIoError::io_error` and the standard error source chain.
///
/// Equality and hashing use the portable message. The optional retained `std` error does not
/// participate, so these operations have the same behavior with and without the `std` feature.
#[derive(Clone, Debug)]
pub struct InputIoError {
    message: String,
    #[cfg(feature = "std")]
    source: Option<Arc<std::io::Error>>,
}

impl InputIoError {
    /// Create I/O error details from a portable message.
    ///
    /// This constructor is available in `no_std` builds. It does not retain a typed source error.
    #[must_use]
    pub fn from_message(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            #[cfg(feature = "std")]
            source: None,
        }
    }

    /// Create I/O error details while retaining the original [`std::io::Error`].
    #[cfg(feature = "std")]
    #[must_use]
    pub fn from_io(error: std::io::Error) -> Self {
        Self {
            message: error.to_string(),
            source: Some(Arc::new(error)),
        }
    }

    /// Return the portable human-readable error message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Return the retained [`std::io::Error`], when one is available.
    #[cfg(feature = "std")]
    #[must_use]
    pub fn io_error(&self) -> Option<&std::io::Error> {
        self.source.as_deref()
    }

    /// Recover the retained [`std::io::Error`] when this is its only owner.
    ///
    /// # Errors
    /// Returns the original `InputIoError` when it was created from a portable message or when
    /// another clone still shares the retained error.
    #[cfg(feature = "std")]
    pub fn try_into_io_error(self) -> Result<std::io::Error, Self> {
        let Self { message, source } = self;
        let Some(source) = source else {
            return Err(Self {
                message,
                source: None,
            });
        };

        match Arc::try_unwrap(source) {
            Ok(error) => Ok(error),
            Err(source) => Err(Self {
                message,
                source: Some(source),
            }),
        }
    }
}

#[cfg(feature = "std")]
impl From<std::io::Error> for InputIoError {
    fn from(error: std::io::Error) -> Self {
        Self::from_io(error)
    }
}

impl PartialEq for InputIoError {
    fn eq(&self, other: &Self) -> bool {
        self.message == other.message
    }
}

impl Eq for InputIoError {}

impl core::hash::Hash for InputIoError {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        core::hash::Hash::hash(&self.message, state);
    }
}

impl fmt::Display for InputIoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl core::error::Error for InputIoError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        #[cfg(feature = "std")]
        {
            self.source
                .as_deref()
                .map(|error| error as &(dyn core::error::Error + 'static))
        }

        #[cfg(not(feature = "std"))]
        {
            None
        }
    }
}

/// Machine-readable category for a [`ScanError`].
#[derive(Clone, PartialEq, Debug, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorKind {
    /// Too many consecutive comments were buffered before a collection entry.
    #[cfg(feature = "comments")]
    TooManyComments,
    /// Reading from the input source failed.
    InputIo {
        /// Portable details and, with the `std` feature, an optional retained I/O error.
        error: InputIoError,
    },
    /// The input source was not valid text in the adapter's expected encoding.
    InputDecoding {
        /// Human-readable details supplied by the input adapter.
        message: String,
    },
    /// The raw input exceeded a configured byte limit.
    InputByteLimitExceeded {
        /// Maximum number of raw input bytes accepted by the adapter.
        limit: usize,
    },
    /// Input ended while parsing a flow sequence.
    UnexpectedEofFlowSequence,
    /// Input ended while parsing a flow mapping.
    UnexpectedEofFlowMapping,
    /// Input ended while parsing an implicit flow mapping.
    UnexpectedEofImplicitFlowMapping,
    /// Input ended while parsing a block sequence.
    UnexpectedEofBlockSequence,
    /// Input ended while parsing a block mapping.
    UnexpectedEofBlockMapping,
    /// Input ended unexpectedly in another parser state.
    UnexpectedEof,
    /// A stream-start token was expected.
    ExpectedStreamStart,
    /// More than one YAML version directive was found for a document.
    DuplicateVersionDirective,
    /// The YAML major version is unsupported.
    UnsupportedYamlMajorVersion,
    /// A tag directive handle was declared more than once for a document.
    DuplicateTagDirective,
    /// A document-start token was expected.
    ExpectedDocumentStart,
    /// A directive followed an implicit document without an explicit document end.
    MissingDocumentEndBeforeDirective,
    /// The parser ran out of representable anchor identifiers.
    AnchorCountOverflow,
    /// An alias referred to an unknown anchor.
    UnknownAnchor,
    /// The parser did not find expected node content.
    ExpectedNodeContent,
    /// A block mapping key was expected.
    ExpectedBlockMappingKey,
    /// A flow mapping separator or closing brace was expected.
    ExpectedFlowMappingSeparator,
    /// A flow sequence separator or closing bracket was expected.
    ExpectedFlowSequenceSeparator,
    /// A block sequence entry indicator was expected.
    ExpectedBlockSequenceEntry,
    /// A tag used a handle that was not declared.
    UndeclaredTagHandle,
    /// No include resolver was configured for a parser stack.
    MissingIncludeResolver,
    /// An error supplied by an external parser adapter or resolver.
    Custom(String),
    /// A parser-stack entry contained multiple documents where only one is supported.
    MultipleDocumentsUnsupported,
    /// An input advertised byte offsets but did not provide the requested slice.
    InputOffsetsWithoutSlice,
    /// An input advertised slicing but did not provide the requested slice.
    InputSlicingUnavailable,
    /// A tag did not begin with the expected exclamation mark.
    ExpectedTagBang,
    /// A tag directive handle did not end with the expected exclamation mark.
    ExpectedTagDirectiveBang,
    /// A global tag started with an invalid character.
    InvalidGlobalTagCharacter,
    /// A required simple key was not followed by a value indicator.
    SimpleKeyExpected,
    /// A previously saved simple key was no longer valid.
    InvalidSimpleKey,
    /// Invalid content followed a document-end marker.
    InvalidDocumentEnd,
    /// Indentation was invalid for the current parser context.
    InvalidIndentation,
    /// A byte-order mark appeared inside a document.
    BomInsideDocument,
    /// An unexpected reserved character was encountered.
    UnexpectedCharacter {
        /// The character that was encountered.
        character: char,
    },
    /// A tab was used in a context where it is not allowed.
    TabNotAllowed,
    /// A tab was used in block indentation.
    TabInBlockIndentation,
    /// A comment interrupted a multiline plain scalar.
    CommentInterceptedScalar,
    /// Required whitespace was not found.
    ExpectedWhitespace,
    /// A comment was not separated from the preceding token by whitespace.
    CommentNotSeparated,
    /// A directive did not end with a comment or line break.
    InvalidDirectiveTerminator,
    /// A YAML version directive did not contain the expected digit or dot.
    MissingYamlVersionSeparator,
    /// A directive name was missing.
    MissingDirectiveName,
    /// A directive name contained an invalid character.
    InvalidDirectiveName,
    /// A retained directive name and payload exceeded the configured byte limit.
    DirectiveByteLimitExceeded {
        /// Maximum number of directive bytes accepted by the scanner.
        limit: usize,
    },
    /// A reserved directive carried more parameters than the configured limit.
    TooManyReservedDirectiveParams {
        /// Maximum number of reserved directive parameters accepted by the scanner.
        limit: usize,
    },
    /// A YAML version component exceeded the supported length.
    YamlVersionTooLong,
    /// A YAML version component was missing.
    MissingYamlVersion,
    /// A tag directive did not end with whitespace or a line break.
    InvalidTagDirectiveTerminator,
    /// A tag token did not end with valid separation whitespace.
    InvalidTagTerminator,
    /// A tag URI was missing.
    MissingTagUri,
    /// A verbatim tag was missing its closing angle bracket.
    UnclosedVerbatimTag,
    /// A tag contained an invalid percent escape.
    InvalidTagEscape,
    /// A tag escape started with an invalid UTF-8 byte.
    InvalidTagUtf8LeadingByte,
    /// A tag escape contained an invalid trailing UTF-8 byte.
    InvalidTagUtf8TrailingByte,
    /// A tag escape did not decode to one valid Unicode scalar value.
    InvalidTagUtf8,
    /// An anchor or alias name was missing.
    MissingAnchorOrAliasName,
    /// A flow collection closing bracket was misplaced.
    MisplacedFlowCollectionEnd,
    /// A flow collection was closed with the wrong bracket type.
    MismatchedFlowCollectionEnd {
        /// The bracket that opened the flow collection.
        open: char,
        /// The bracket that closed the flow collection.
        close: char,
    },
    /// A flow collection was not closed.
    UnclosedFlowCollection {
        /// The bracket that opened the flow collection.
        open: char,
    },
    /// A configured flow or block collection nesting limit was exceeded.
    RecursionLimitExceeded,
    /// A block entry indicator appeared inside a flow collection.
    BlockEntryInFlowCollection,
    /// A block sequence entry appeared in a context that does not allow it.
    BlockSequenceEntryNotAllowed,
    /// A block entry indicator was followed by invalid whitespace.
    InvalidBlockEntryWhitespace,
    /// A block scalar used an indentation indicator of zero.
    ZeroBlockScalarIndent,
    /// A block scalar header did not end with a comment or line break.
    InvalidBlockScalarHeader,
    /// Block scalar content began with a tab.
    TabAtBlockScalarStart,
    /// A block scalar content line had invalid indentation.
    InvalidBlockScalarIndent,
    /// A document indicator appeared inside a quoted scalar.
    DocumentIndicatorInQuotedScalar,
    /// A quoted scalar was not closed.
    UnclosedQuotedScalar,
    /// A tab was used as indentation.
    TabInIndentation,
    /// A multiline quoted scalar had invalid indentation.
    InvalidQuotedScalarIndent,
    /// Invalid content followed a single-quoted scalar.
    InvalidTrailingSingleQuotedScalar,
    /// Invalid content followed a double-quoted scalar.
    InvalidTrailingDoubleQuotedScalar,
    /// A quoted scalar contained an unknown escape character.
    UnknownQuotedScalarEscape,
    /// A quoted scalar escape did not contain the expected hexadecimal digits.
    InvalidQuotedScalarHexEscape,
    /// A low-surrogate escape did not contain the expected hexadecimal digits.
    InvalidLowSurrogateHexEscape,
    /// A surrogate pair contained an invalid low surrogate.
    InvalidLowSurrogate,
    /// A high surrogate was not followed by a low surrogate.
    MissingLowSurrogate,
    /// A low surrogate appeared without a preceding high surrogate.
    UnpairedLowSurrogate,
    /// A quoted scalar escape did not represent a valid Unicode scalar value.
    InvalidUnicodeEscape,
    /// A flow scalar started at invalid indentation.
    InvalidFlowScalarIndent,
    /// A plain scalar began with a dash followed by a flow indicator.
    PlainScalarStartsWithDashFlowIndicator,
    /// A tab appeared where a plain scalar could not accept it.
    TabInPlainScalar,
    /// A plain scalar ended before consuming any content.
    UnexpectedEndOfPlainScalar,
    /// A mapping key appeared in a context that does not allow one.
    MappingKeyNotAllowed,
    /// A flow mapping value indicator was adjacent to a collection start.
    FlowMappingValueAdjacentCollection,
    /// A mapping value indicator was followed by invalid whitespace.
    InvalidMappingValueWhitespace,
    /// A value indicator was placed illegally in an implicit flow mapping.
    InvalidColonPlacement,
    /// A mapping value appeared in a context that does not allow one.
    MappingValueNotAllowed,
}

#[cfg(feature = "error_messages")]
impl fmt::Display for ErrorKind {
    #[allow(clippy::too_many_lines)]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(feature = "comments")]
            Self::TooManyComments => {
                f.write_str("too many consecutive comments before resolving collection entry")
            }
            Self::InputIo { error } => write!(f, "input I/O error: {error}"),
            Self::InputDecoding { message } => {
                write!(f, "input decoding error: {message}")
            }
            Self::InputByteLimitExceeded { limit } => {
                write!(f, "input exceeds the configured limit of {limit} bytes")
            }
            Self::UnexpectedEofFlowSequence => {
                f.write_str("unexpected EOF while parsing a flow sequence")
            }
            Self::UnexpectedEofFlowMapping => {
                f.write_str("unexpected EOF while parsing a flow mapping")
            }
            Self::UnexpectedEofImplicitFlowMapping => {
                f.write_str("unexpected EOF while parsing an implicit flow mapping")
            }
            Self::UnexpectedEofBlockSequence => {
                f.write_str("unexpected EOF while parsing a block sequence")
            }
            Self::UnexpectedEofBlockMapping => {
                f.write_str("unexpected EOF while parsing a block mapping")
            }
            Self::UnexpectedEof => f.write_str("unexpected eof"),
            Self::ExpectedStreamStart => f.write_str("did not find expected <stream-start>"),
            Self::DuplicateVersionDirective => f.write_str("duplicate version directive"),
            Self::UnsupportedYamlMajorVersion => {
                f.write_str("unsupported YAML major version")
            }
            Self::DuplicateTagDirective => f.write_str(
                "the TAG directive must only be given at most once per handle in the same document",
            ),
            Self::ExpectedDocumentStart => {
                f.write_str("did not find expected <document start>")
            }
            Self::MissingDocumentEndBeforeDirective => {
                f.write_str("missing explicit document end marker before directive")
            }
            Self::AnchorCountOverflow => {
                f.write_str("while parsing anchor, anchor count exceeded supported limit")
            }
            Self::UnknownAnchor => f.write_str("while parsing node, found unknown anchor"),
            Self::ExpectedNodeContent => {
                f.write_str("while parsing a node, did not find expected node content")
            }
            Self::ExpectedBlockMappingKey => {
                f.write_str("while parsing a block mapping, did not find expected key")
            }
            Self::ExpectedFlowMappingSeparator => {
                f.write_str("while parsing a flow mapping, did not find expected ',' or '}'")
            }
            Self::ExpectedFlowSequenceSeparator => {
                f.write_str("while parsing a flow sequence, expected ',' or ']'")
            }
            Self::ExpectedBlockSequenceEntry => f.write_str(
                "while parsing a block collection, did not find expected '-' indicator",
            ),
            Self::UndeclaredTagHandle => f.write_str("the handle wasn't declared"),
            Self::MissingIncludeResolver => {
                f.write_str("No include resolver set for parser stack.")
            }
            Self::Custom(message) => f.write_str(message),
            Self::MultipleDocumentsUnsupported => {
                f.write_str("multiple documents not supported here")
            }
            Self::InputOffsetsWithoutSlice => f.write_str(
                "internal error: input advertised offsets but did not provide a slice",
            ),
            Self::InputSlicingUnavailable => f.write_str(
                "internal error: input advertised slicing but did not provide a slice",
            ),
            Self::ExpectedTagBang => {
                f.write_str("while scanning a tag, did not find expected '!'")
            }
            Self::ExpectedTagDirectiveBang => {
                f.write_str("while parsing a tag directive, did not find expected '!'")
            }
            Self::InvalidGlobalTagCharacter => f.write_str("invalid global tag character"),
            Self::SimpleKeyExpected => f.write_str("simple key expected ':'"),
            Self::InvalidSimpleKey => f.write_str("simple key is no longer valid"),
            Self::InvalidDocumentEnd => {
                f.write_str("invalid content after document end marker")
            }
            Self::InvalidIndentation => f.write_str("invalid indentation"),
            Self::BomInsideDocument => {
                f.write_str("a BOM must not appear inside a document")
            }
            Self::UnexpectedCharacter { character } => {
                write!(f, "unexpected character: `{}'", character.escape_default())
            }
            Self::TabNotAllowed => f.write_str("tabs disallowed in this context"),
            Self::TabInBlockIndentation => {
                f.write_str("tabs disallowed within this context (block indentation)")
            }
            Self::CommentInterceptedScalar => {
                f.write_str("comment intercepting the multiline text")
            }
            Self::ExpectedWhitespace => f.write_str("expected whitespace"),
            Self::CommentNotSeparated => {
                f.write_str("comments must be separated from other tokens by whitespace")
            }
            Self::InvalidDirectiveTerminator => f.write_str(
                "while scanning a directive, did not find expected comment or line break",
            ),
            Self::MissingYamlVersionSeparator => f.write_str(
                "while scanning a YAML directive, did not find expected digit or '.' character",
            ),
            Self::MissingDirectiveName => f.write_str(
                "while scanning a directive, could not find expected directive name",
            ),
            Self::InvalidDirectiveName => f.write_str(
                "while scanning a directive, found unexpected non-alphabetical character",
            ),
            Self::DirectiveByteLimitExceeded { limit } => write!(
                f,
                "directive exceeds the configured limit of {limit} bytes"
            ),
            Self::TooManyReservedDirectiveParams { limit } => write!(
                f,
                "reserved directive exceeds the configured limit of {limit} parameters"
            ),
            Self::YamlVersionTooLong => {
                f.write_str("while scanning a YAML directive, found extremely long version number")
            }
            Self::MissingYamlVersion => f.write_str(
                "while scanning a YAML directive, did not find expected version number",
            ),
            Self::InvalidTagDirectiveTerminator => {
                f.write_str("while scanning TAG, did not find expected whitespace or line break")
            }
            Self::InvalidTagTerminator => f.write_str(
                "while scanning a tag, did not find expected whitespace or line break",
            ),
            Self::MissingTagUri => {
                f.write_str("while parsing a tag, did not find expected tag URI")
            }
            Self::UnclosedVerbatimTag => {
                f.write_str("while scanning a verbatim tag, did not find the expected '>'")
            }
            Self::InvalidTagEscape => {
                f.write_str("while parsing a tag, found an invalid escape sequence")
            }
            Self::InvalidTagUtf8LeadingByte => {
                f.write_str("while parsing a tag, found an incorrect leading UTF-8 byte")
            }
            Self::InvalidTagUtf8TrailingByte => {
                f.write_str("while parsing a tag, found an incorrect trailing UTF-8 byte")
            }
            Self::InvalidTagUtf8 => {
                f.write_str("while parsing a tag, found an invalid UTF-8 codepoint")
            }
            Self::MissingAnchorOrAliasName => f.write_str(
                "while scanning an anchor or alias, did not find expected alphabetic or numeric character",
            ),
            Self::MisplacedFlowCollectionEnd => f.write_str("misplaced bracket"),
            Self::MismatchedFlowCollectionEnd { open, close } => {
                write!(f, "mismatched bracket '{open}' closed by '{close}'")
            }
            Self::UnclosedFlowCollection { open } => {
                write!(f, "unclosed bracket '{open}'")
            }
            Self::RecursionLimitExceeded => f.write_str("recursion limit exceeded"),
            Self::BlockEntryInFlowCollection => {
                f.write_str(r#""-" is only valid inside a block"#)
            }
            Self::BlockSequenceEntryNotAllowed => {
                f.write_str("block sequence entries are not allowed in this context")
            }
            Self::InvalidBlockEntryWhitespace => {
                f.write_str("'-' must be followed by a valid YAML whitespace")
            }
            Self::ZeroBlockScalarIndent => f.write_str(
                "while scanning a block scalar, found an indentation indicator equal to 0",
            ),
            Self::InvalidBlockScalarHeader => f.write_str(
                "while scanning a block scalar, did not find expected comment or line break",
            ),
            Self::TabAtBlockScalarStart => {
                f.write_str("a block scalar content cannot start with a tab")
            }
            Self::InvalidBlockScalarIndent => {
                f.write_str("wrongly indented line in block scalar")
            }
            Self::DocumentIndicatorInQuotedScalar => f.write_str(
                "while scanning a quoted scalar, found unexpected document indicator",
            ),
            Self::UnclosedQuotedScalar => f.write_str("unclosed quote"),
            Self::TabInIndentation => f.write_str("tab cannot be used as indentation"),
            Self::InvalidQuotedScalarIndent => {
                f.write_str("invalid indentation in multiline quoted scalar")
            }
            Self::InvalidTrailingSingleQuotedScalar => {
                f.write_str("invalid trailing content after single-quoted scalar")
            }
            Self::InvalidTrailingDoubleQuotedScalar => {
                f.write_str("invalid trailing content after double-quoted scalar")
            }
            Self::UnknownQuotedScalarEscape => {
                f.write_str("while parsing a quoted scalar, found unknown escape character")
            }
            Self::InvalidQuotedScalarHexEscape => f.write_str(
                "while parsing a quoted scalar, did not find expected hexadecimal number",
            ),
            Self::InvalidLowSurrogateHexEscape => f.write_str(
                "while parsing a quoted scalar, did not find expected hexadecimal number for low surrogate",
            ),
            Self::InvalidLowSurrogate => {
                f.write_str("while parsing a quoted scalar, found invalid low surrogate")
            }
            Self::MissingLowSurrogate => f.write_str(
                "while parsing a quoted scalar, found high surrogate without following low surrogate",
            ),
            Self::UnpairedLowSurrogate => {
                f.write_str("while parsing a quoted scalar, found unpaired low surrogate")
            }
            Self::InvalidUnicodeEscape => f.write_str(
                "while parsing a quoted scalar, found invalid Unicode character escape code",
            ),
            Self::InvalidFlowScalarIndent => {
                f.write_str("invalid indentation in flow construct")
            }
            Self::PlainScalarStartsWithDashFlowIndicator => {
                f.write_str("plain scalar cannot start with '-' followed by ,[]{}")
            }
            Self::TabInPlainScalar => {
                f.write_str("while scanning a plain scalar, found a tab")
            }
            Self::UnexpectedEndOfPlainScalar => f.write_str("unexpected end of plain scalar"),
            Self::MappingKeyNotAllowed => {
                f.write_str("mapping keys are not allowed in this context")
            }
            Self::FlowMappingValueAdjacentCollection => {
                f.write_str("':' may not precede any of `[{` in flow mapping")
            }
            Self::InvalidMappingValueWhitespace => {
                f.write_str("':' must be followed by a valid YAML whitespace")
            }
            Self::InvalidColonPlacement => f.write_str("illegal placement of ':' indicator"),
            Self::MappingValueNotAllowed => {
                f.write_str("mapping values are not allowed in this context")
            }
        }
    }
}

#[cfg(not(feature = "error_messages"))]
impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("")
    }
}

/// An error that occurred while reading, scanning, or parsing YAML.
#[derive(Clone, PartialEq, Debug, Eq)]
pub struct ScanError {
    /// The position at which the error happened in the source.
    mark: Marker,
    /// Machine-readable error category.
    kind: ErrorKind,
    /// Source names captured by a parser stack before its failing entry is removed.
    source_stack: Vec<String>,
}

impl ScanError {
    /// Create an externally supplied error from a location and message.
    ///
    /// The message is stored in [`ErrorKind::Custom`]. This is useful for adapters that
    /// participate in parser APIs, such as a custom [`ParserTrait`](crate::ParserTrait)
    /// implementation or a [`ParserStack`](crate::ParserStack) include resolver.
    #[must_use]
    #[cold]
    pub fn new(loc: Marker, message: impl Into<String>) -> ScanError {
        Self::from_kind(loc, ErrorKind::Custom(message.into()))
    }

    #[must_use]
    #[cold]
    pub(crate) fn from_kind(loc: Marker, kind: ErrorKind) -> ScanError {
        ScanError {
            mark: loc,
            kind,
            source_stack: Vec::new(),
        }
    }

    #[must_use]
    pub(crate) fn with_source_stack(mut self, source_stack: Vec<String>) -> Self {
        self.source_stack = source_stack;
        self
    }

    #[cold]
    pub(crate) fn into_result<T>(self) -> Result<T, ScanError> {
        Err(self)
    }

    /// Return the marker pointing to the error in the source.
    #[must_use]
    pub fn marker(&self) -> &Marker {
        &self.mark
    }

    /// Return the machine-readable error category.
    #[must_use]
    pub fn kind(&self) -> &ErrorKind {
        &self.kind
    }

    /// Extract the input I/O error details without cloning them.
    ///
    /// # Errors
    /// Returns the original scan error unchanged when it has a different error category.
    pub fn try_into_input_io_error(self) -> Result<InputIoError, Self> {
        let Self {
            mark,
            kind,
            source_stack,
        } = self;

        match kind {
            ErrorKind::InputIo { error } => Ok(error),
            kind => Err(Self {
                mark,
                kind,
                source_stack,
            }),
        }
    }

    /// Return source names captured by a parser stack, from bottom to top.
    #[must_use]
    pub fn source_stack(&self) -> &[String] {
        &self.source_stack
    }

    /// Render the error as a human-readable description.
    ///
    /// Parser-stack errors include their nested source names. The result remains
    /// empty when the `error_messages` feature is disabled.
    #[must_use]
    pub fn info(&self) -> String {
        let mut info = self.kind.to_string();
        if !info.is_empty() && self.source_stack().len() > 1 {
            info.push_str("\nwhile parsing ");
            info.push_str(&self.source_stack().join(" -> "));
        }
        info
    }
}

impl fmt::Display for ScanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at char {} line {} column {}",
            self.info(),
            self.mark.index(),
            self.mark.line(),
            self.mark.col() + 1
        )
    }
}

impl core::error::Error for ScanError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match &self.kind {
            ErrorKind::InputIo { error } => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "error_messages")]
    use alloc::format;
    #[cfg(feature = "error_messages")]
    use alloc::string::String;
    use alloc::string::ToString;

    use super::{ErrorKind, InputIoError, ScanError};
    use crate::scanner::Marker;

    #[cfg(feature = "error_messages")]
    #[test]
    fn constructor_retains_kind_and_derives_info() {
        let marker = Marker::new(3, 2, 1);
        let error = ScanError::from_kind(marker, ErrorKind::ExpectedWhitespace);

        assert_eq!(error.kind(), &ErrorKind::ExpectedWhitespace);
        assert_eq!(error.kind().to_string(), "expected whitespace");
        assert_eq!(error.info(), "expected whitespace");
    }

    #[cfg(feature = "error_messages")]
    #[test]
    fn parameterized_kind_constructs_info() {
        let marker = Marker::new(3, 2, 1);
        let error = ScanError::from_kind(
            marker,
            ErrorKind::MismatchedFlowCollectionEnd {
                open: '[',
                close: '}',
            },
        );

        assert_eq!(error.info(), "mismatched bracket '[' closed by '}'");
        assert_eq!(
            format!("{error}"),
            "mismatched bracket '[' closed by '}' at char 3 line 2 column 2"
        );
    }

    #[cfg(feature = "error_messages")]
    #[test]
    fn input_error_kinds_construct_info() {
        assert_eq!(
            ErrorKind::InputIo {
                error: InputIoError::from_message("connection reset")
            }
            .to_string(),
            "input I/O error: connection reset"
        );
        assert_eq!(
            ErrorKind::InputDecoding {
                message: String::from("invalid utf-8")
            }
            .to_string(),
            "input decoding error: invalid utf-8"
        );
        assert_eq!(
            ErrorKind::InputByteLimitExceeded { limit: 4096 }.to_string(),
            "input exceeds the configured limit of 4096 bytes"
        );
    }

    #[test]
    fn message_only_input_io_error_has_no_source() {
        use core::error::Error as _;

        let error = InputIoError::from_message("portable failure");

        assert_eq!(error.message(), "portable failure");
        assert!(error.source().is_none());
    }

    #[cfg(feature = "std")]
    #[test]
    fn std_input_io_error_is_retained_in_scan_error_source_chain() {
        use core::error::Error as _;
        use std::io;

        let details = InputIoError::from(io::Error::new(io::ErrorKind::BrokenPipe, "pipe closed"));
        assert_eq!(details.message(), "pipe closed");
        assert_eq!(
            details
                .io_error()
                .expect("std construction should retain io::Error")
                .kind(),
            io::ErrorKind::BrokenPipe
        );

        let error = ScanError::from_kind(
            Marker::new(3, 2, 1),
            ErrorKind::InputIo {
                error: details.clone(),
            },
        );
        let input_error = error
            .source()
            .and_then(|source| source.downcast_ref::<InputIoError>())
            .expect("ScanError should expose InputIoError as its source");
        let io_error = input_error
            .source()
            .and_then(|source| source.downcast_ref::<io::Error>())
            .expect("InputIoError should expose the retained io::Error");

        assert_eq!(io_error.kind(), io::ErrorKind::BrokenPipe);
        assert_eq!(details, *input_error);
    }

    #[cfg(feature = "std")]
    #[test]
    fn unique_std_input_io_error_can_be_recovered() {
        use std::io;

        let details = InputIoError::from(io::Error::from_raw_os_error(12_345));
        let error = details
            .try_into_io_error()
            .expect("a uniquely owned io::Error should be recoverable");

        assert_eq!(error.raw_os_error(), Some(12_345));
    }

    #[cfg(feature = "std")]
    #[test]
    fn shared_std_input_io_error_can_be_recovered_after_other_clone_is_dropped() {
        use std::io;

        let details = InputIoError::from(io::Error::from_raw_os_error(12_345));
        let other = details.clone();
        let details = details
            .try_into_io_error()
            .expect_err("a shared io::Error cannot be moved out");

        drop(other);

        let error = details
            .try_into_io_error()
            .expect("the last owner should recover the io::Error");
        assert_eq!(error.raw_os_error(), Some(12_345));
    }

    #[cfg(feature = "std")]
    #[test]
    fn scan_error_moves_input_io_error_out_without_cloning() {
        use std::io;

        let error = ScanError::from_kind(
            Marker::new(3, 2, 1),
            ErrorKind::InputIo {
                error: InputIoError::from(io::Error::from_raw_os_error(12_345)),
            },
        );
        let details = error
            .try_into_input_io_error()
            .expect("input I/O details should be extractable");
        let error = details
            .try_into_io_error()
            .expect("extracting the scan error should retain unique ownership");

        assert_eq!(error.raw_os_error(), Some(12_345));
    }

    #[test]
    fn extracting_input_io_error_preserves_other_scan_errors() {
        let error = ScanError::from_kind(Marker::new(3, 2, 1), ErrorKind::ExpectedWhitespace);
        let error = error
            .try_into_input_io_error()
            .expect_err("a non-I/O scan error should be returned unchanged");

        assert_eq!(error.marker(), &Marker::new(3, 2, 1));
        assert_eq!(error.kind(), &ErrorKind::ExpectedWhitespace);
    }

    #[cfg(feature = "error_messages")]
    #[test]
    fn public_constructor_copies_custom_message() {
        let marker = Marker::new(3, 2, 1);
        let mut message = String::from("adapter failed");
        let error = ScanError::new(marker, &message);
        message.clear();

        assert_eq!(
            error.kind(),
            &ErrorKind::Custom(String::from("adapter failed"))
        );
        assert_eq!(error.info(), "adapter failed");
    }

    #[cfg(not(feature = "error_messages"))]
    #[test]
    fn disabled_error_messages_are_empty() {
        let marker = Marker::new(3, 2, 1);
        let error = ScanError::from_kind(
            marker,
            ErrorKind::MismatchedFlowCollectionEnd {
                open: '[',
                close: '}',
            },
        );

        assert_eq!(error.kind().to_string(), "");
        assert_eq!(error.info(), "");
    }
}
