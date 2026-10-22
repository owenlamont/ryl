use std::string::String;
use std::string::ToString;

use ryl_yaml_parser::Marker;
use ryl_yaml_parser::{ErrorKind, InputIoError, ScanError};

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

#[test]
fn unique_std_input_io_error_can_be_recovered() {
    use std::io;

    let details = InputIoError::from(io::Error::from_raw_os_error(12_345));
    let error = details
        .try_into_io_error()
        .expect("a uniquely owned io::Error should be recoverable");

    assert_eq!(error.raw_os_error(), Some(12_345));
}

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

#[test]
fn every_error_kind_renders_its_message() {
    for (kind, message) in [
        (
            ErrorKind::TooManyComments,
            "too many consecutive comments before resolving collection entry",
        ),
        (
            ErrorKind::InputIo {
                error: InputIoError::from_message("disk full"),
            },
            "input I/O error: disk full",
        ),
        (
            ErrorKind::InputDecoding {
                message: "bad byte".to_owned(),
            },
            "input decoding error: bad byte",
        ),
        (
            ErrorKind::InputByteLimitExceeded { limit: 8 },
            "input exceeds the configured limit of 8 bytes",
        ),
        (
            ErrorKind::UnexpectedEofImplicitFlowMapping,
            "unexpected EOF while parsing an implicit flow mapping",
        ),
        (
            ErrorKind::UnexpectedEofBlockSequence,
            "unexpected EOF while parsing a block sequence",
        ),
        (
            ErrorKind::UnexpectedEofBlockMapping,
            "unexpected EOF while parsing a block mapping",
        ),
        (
            ErrorKind::DuplicateVersionDirective,
            "duplicate version directive",
        ),
        (
            ErrorKind::UnsupportedYamlMajorVersion,
            "unsupported YAML major version",
        ),
        (
            ErrorKind::DuplicateTagDirective,
            "the TAG directive must only be given at most once per handle in the same document",
        ),
        (
            ErrorKind::ExpectedDocumentStart,
            "did not find expected <document start>",
        ),
        (
            ErrorKind::MissingDocumentEndBeforeDirective,
            "missing explicit document end marker before directive",
        ),
        (
            ErrorKind::UnknownAnchor,
            "while parsing node, found unknown anchor",
        ),
        (
            ErrorKind::ExpectedBlockMappingKey,
            "while parsing a block mapping, did not find expected key",
        ),
        (
            ErrorKind::ExpectedFlowMappingSeparator,
            "while parsing a flow mapping, did not find expected ',' or '}'",
        ),
        (
            ErrorKind::ExpectedFlowSequenceSeparator,
            "while parsing a flow sequence, expected ',' or ']'",
        ),
        (
            ErrorKind::ExpectedBlockSequenceEntry,
            "while parsing a block collection, did not find expected '-' indicator",
        ),
        (ErrorKind::UndeclaredTagHandle, "the handle wasn't declared"),
        (
            ErrorKind::MissingIncludeResolver,
            "No include resolver set for parser stack.",
        ),
        (
            ErrorKind::Custom("adapter failed".to_owned()),
            "adapter failed",
        ),
        (
            ErrorKind::MultipleDocumentsUnsupported,
            "multiple documents not supported here",
        ),
        (
            ErrorKind::InputOffsetsWithoutSlice,
            "internal error: input advertised offsets but did not provide a slice",
        ),
        (
            ErrorKind::InputSlicingUnavailable,
            "internal error: input advertised slicing but did not provide a slice",
        ),
        (
            ErrorKind::ExpectedTagBang,
            "while scanning a tag, did not find expected '!'",
        ),
        (
            ErrorKind::ExpectedTagDirectiveBang,
            "while parsing a tag directive, did not find expected '!'",
        ),
        (
            ErrorKind::InvalidGlobalTagCharacter,
            "invalid global tag character",
        ),
        (ErrorKind::SimpleKeyExpected, "simple key expected ':'"),
        (
            ErrorKind::InvalidDocumentEnd,
            "invalid content after document end marker",
        ),
        (ErrorKind::InvalidIndentation, "invalid indentation"),
        (
            ErrorKind::BomInsideDocument,
            "a BOM must not appear inside a document",
        ),
        (
            ErrorKind::UnexpectedCharacter { character: '\u{1}' },
            "unexpected character: `\\u{1}'",
        ),
        (ErrorKind::TabNotAllowed, "tabs disallowed in this context"),
        (
            ErrorKind::TabInBlockIndentation,
            "tabs disallowed within this context (block indentation)",
        ),
        (
            ErrorKind::CommentInterceptedScalar,
            "comment intercepting the multiline text",
        ),
        (ErrorKind::ExpectedWhitespace, "expected whitespace"),
        (
            ErrorKind::CommentNotSeparated,
            "comments must be separated from other tokens by whitespace",
        ),
        (
            ErrorKind::InvalidDirectiveTerminator,
            "while scanning a directive, did not find expected comment or line break",
        ),
        (
            ErrorKind::MissingYamlVersionSeparator,
            "while scanning a YAML directive, did not find expected digit or '.' character",
        ),
        (
            ErrorKind::MissingDirectiveName,
            "while scanning a directive, could not find expected directive name",
        ),
        (
            ErrorKind::InvalidDirectiveName,
            "while scanning a directive, found unexpected non-alphabetical character",
        ),
        (
            ErrorKind::DirectiveByteLimitExceeded { limit: 8 },
            "directive exceeds the configured limit of 8 bytes",
        ),
        (
            ErrorKind::TooManyReservedDirectiveParams { limit: 2 },
            "reserved directive exceeds the configured limit of 2 parameters",
        ),
        (
            ErrorKind::YamlVersionTooLong,
            "while scanning a YAML directive, found extremely long version number",
        ),
        (
            ErrorKind::MissingYamlVersion,
            "while scanning a YAML directive, did not find expected version number",
        ),
        (
            ErrorKind::InvalidTagDirectiveTerminator,
            "while scanning TAG, did not find expected whitespace or line break",
        ),
        (
            ErrorKind::InvalidTagTerminator,
            "while scanning a tag, did not find expected whitespace or line break",
        ),
        (
            ErrorKind::MissingTagUri,
            "while parsing a tag, did not find expected tag URI",
        ),
        (
            ErrorKind::UnclosedVerbatimTag,
            "while scanning a verbatim tag, did not find the expected '>'",
        ),
        (
            ErrorKind::InvalidTagEscape,
            "while parsing a tag, found an invalid escape sequence",
        ),
        (
            ErrorKind::InvalidTagUtf8LeadingByte,
            "while parsing a tag, found an incorrect leading UTF-8 byte",
        ),
        (
            ErrorKind::InvalidTagUtf8TrailingByte,
            "while parsing a tag, found an incorrect trailing UTF-8 byte",
        ),
        (
            ErrorKind::InvalidTagUtf8,
            "while parsing a tag, found an invalid UTF-8 codepoint",
        ),
        (
            ErrorKind::MissingAnchorOrAliasName,
            "while scanning an anchor or alias, did not find expected alphabetic or numeric character",
        ),
        (ErrorKind::MisplacedFlowCollectionEnd, "misplaced bracket"),
        (
            ErrorKind::MismatchedFlowCollectionEnd {
                open: '[',
                close: '}',
            },
            "mismatched bracket '[' closed by '}'",
        ),
        (
            ErrorKind::UnclosedFlowCollection { open: '{' },
            "unclosed bracket '{'",
        ),
        (
            ErrorKind::RecursionLimitExceeded,
            "recursion limit exceeded",
        ),
        (
            ErrorKind::BlockEntryInFlowCollection,
            "\"-\" is only valid inside a block",
        ),
        (
            ErrorKind::BlockSequenceEntryNotAllowed,
            "block sequence entries are not allowed in this context",
        ),
        (
            ErrorKind::InvalidBlockEntryWhitespace,
            "'-' must be followed by a valid YAML whitespace",
        ),
        (
            ErrorKind::ZeroBlockScalarIndent,
            "while scanning a block scalar, found an indentation indicator equal to 0",
        ),
        (
            ErrorKind::InvalidBlockScalarHeader,
            "while scanning a block scalar, did not find expected comment or line break",
        ),
        (
            ErrorKind::TabAtBlockScalarStart,
            "a block scalar content cannot start with a tab",
        ),
        (
            ErrorKind::InvalidBlockScalarIndent,
            "wrongly indented line in block scalar",
        ),
        (
            ErrorKind::DocumentIndicatorInQuotedScalar,
            "while scanning a quoted scalar, found unexpected document indicator",
        ),
        (ErrorKind::UnclosedQuotedScalar, "unclosed quote"),
        (
            ErrorKind::TabInIndentation,
            "tab cannot be used as indentation",
        ),
        (
            ErrorKind::InvalidQuotedScalarIndent,
            "invalid indentation in multiline quoted scalar",
        ),
        (
            ErrorKind::InvalidTrailingSingleQuotedScalar,
            "invalid trailing content after single-quoted scalar",
        ),
        (
            ErrorKind::InvalidTrailingDoubleQuotedScalar,
            "invalid trailing content after double-quoted scalar",
        ),
        (
            ErrorKind::UnknownQuotedScalarEscape,
            "while parsing a quoted scalar, found unknown escape character",
        ),
        (
            ErrorKind::InvalidQuotedScalarHexEscape,
            "while parsing a quoted scalar, did not find expected hexadecimal number",
        ),
        (
            ErrorKind::InvalidLowSurrogateHexEscape,
            "while parsing a quoted scalar, did not find expected hexadecimal number for low surrogate",
        ),
        (
            ErrorKind::InvalidLowSurrogate,
            "while parsing a quoted scalar, found invalid low surrogate",
        ),
        (
            ErrorKind::MissingLowSurrogate,
            "while parsing a quoted scalar, found high surrogate without following low surrogate",
        ),
        (
            ErrorKind::UnpairedLowSurrogate,
            "while parsing a quoted scalar, found unpaired low surrogate",
        ),
        (
            ErrorKind::InvalidUnicodeEscape,
            "while parsing a quoted scalar, found invalid Unicode character escape code",
        ),
        (
            ErrorKind::PlainScalarStartsWithDashFlowIndicator,
            "plain scalar cannot start with '-' followed by ,[]{}",
        ),
        (
            ErrorKind::TabInPlainScalar,
            "while scanning a plain scalar, found a tab",
        ),
        (
            ErrorKind::UnexpectedEndOfPlainScalar,
            "unexpected end of plain scalar",
        ),
        (
            ErrorKind::MappingKeyNotAllowed,
            "mapping keys are not allowed in this context",
        ),
        (
            ErrorKind::FlowMappingValueAdjacentCollection,
            "':' may not precede any of `[{` in flow mapping",
        ),
        (
            ErrorKind::InvalidColonPlacement,
            "illegal placement of ':' indicator",
        ),
        (
            ErrorKind::MappingValueNotAllowed,
            "mapping values are not allowed in this context",
        ),
    ] {
        assert_eq!(kind.to_string(), message, "{kind:?}");
    }
}

#[test]
fn input_io_error_retains_only_an_io_source() {
    use core::error::Error as _;
    use std::io;

    let portable = InputIoError::from_message("disk full");
    assert!(portable.io_error().is_none());
    assert_eq!(portable.clone().try_into_io_error().unwrap_err(), portable);

    let retained = InputIoError::from_io(io::Error::other("disk full"));
    assert_eq!(retained.io_error().unwrap().kind(), io::ErrorKind::Other);
    assert_eq!(retained.source().unwrap().to_string(), "disk full");
    // Equality and hashing see only the message, so both spellings are one value.
    assert_eq!(
        std::collections::HashSet::from([portable, retained]).len(),
        1
    );
}

#[test]
fn source_io_failure_surfaces_through_the_scan_error() {
    use core::error::Error as _;
    use std::io;

    let failure = ErrorKind::InputIo {
        error: InputIoError::from_io(io::Error::other("connection reset")),
    };
    let error = ryl_yaml_parser::Parser::new_from_fallible_iter(
        "key: ".chars().map(Ok).chain(std::iter::once(Err(failure))),
    )
    .find_map(Result::err)
    .expect("a source failure must surface as an error");
    assert_eq!(error.source().unwrap().to_string(), "connection reset");
    let details = error
        .try_into_input_io_error()
        .expect("an input I/O failure keeps its details");
    assert_eq!(details.message(), "connection reset");

    let custom = ScanError::new(Marker::new(0, 1, 0), "adapter failed");
    assert!(custom.source().is_none());
    assert_eq!(
        custom.clone().try_into_input_io_error().unwrap_err(),
        custom
    );
}
