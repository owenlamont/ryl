use ryl_yaml_parser::{BufferedInput, Scanner, StrInput};
use ryl_yaml_parser::{ErrorKind, ScanError};

#[test]
fn anchor_name_rejects_non_printable_control_chars() {
    let mut scanner = Scanner::new(StrInput::new("&foo\u{0001}\n"));

    scanner.next().transpose().unwrap();
    assert_eq!(
        scanner.next().transpose().unwrap_err().kind(),
        &ErrorKind::UnexpectedCharacter {
            character: '\u{0001}'
        }
    );
}

#[test]
fn alias_name_rejects_non_printable_control_chars() {
    let mut scanner = Scanner::new(StrInput::new("*foo\u{0001}\n"));

    scanner.next().transpose().unwrap();
    assert_eq!(
        scanner.next().transpose().unwrap_err().kind(),
        &ErrorKind::UnexpectedCharacter {
            character: '\u{0001}'
        }
    );
}

#[test]
fn secondary_tag_requires_suffix_in_borrowed_and_buffered_paths() {
    assert_eq!(
        first_scanner_error_kind("!! value\n"),
        ErrorKind::MissingTagUri
    );
    assert_eq!(
        first_buffered_scanner_error_kind("!! value\n"),
        ErrorKind::MissingTagUri
    );
}

fn first_scanner_error_kind(input: &str) -> ErrorKind {
    first_scanner_error(input).kind().clone()
}

fn first_buffered_scanner_error_kind(input: &str) -> ErrorKind {
    let mut scanner = Scanner::new(BufferedInput::new(input.chars()));
    loop {
        match scanner.next().transpose() {
            Ok(Some(_)) => {}
            Ok(None) => panic!("expected scanner error"),
            Err(error) => return error.kind().clone(),
        }
    }
}

fn first_scanner_error(input: &str) -> ScanError {
    let mut scanner = Scanner::new(StrInput::new(input));
    loop {
        match scanner.next().transpose() {
            Ok(Some(_)) => {}
            Ok(None) => panic!("expected scanner error"),
            Err(error) => return error,
        }
    }
}

#[test]
fn iterator_next_emits_error_and_then_stays_empty() {
    let mut scanner = Scanner::new(StrInput::new("\"unterminated"));

    let error = scanner
        .by_ref()
        .find_map(Result::err)
        .expect("scanner should emit the error");
    assert_eq!(error.kind(), &ErrorKind::UnclosedQuotedScalar);
    assert!(scanner.next().is_none());
}

#[test]
fn directive_name_must_be_present() {
    assert_eq!(
        first_scanner_error_kind("%\n"),
        ErrorKind::MissingDirectiveName
    );
}

#[test]
fn yaml_directive_requires_dot_between_version_numbers() {
    assert_eq!(
        first_scanner_error_kind("%YAML 1\n"),
        ErrorKind::MissingYamlVersionSeparator
    );
}

#[test]
fn yaml_directive_requires_major_version_number() {
    assert_eq!(
        first_scanner_error_kind("%YAML .2\n"),
        ErrorKind::MissingYamlVersion
    );
}

#[test]
fn yaml_directive_rejects_extremely_long_version_number() {
    assert_eq!(
        first_scanner_error_kind("%YAML 1234567890.2\n"),
        ErrorKind::YamlVersionTooLong
    );
}

#[test]
fn tag_directive_handle_must_end_with_bang() {
    assert_eq!(
        first_scanner_error_kind("%TAG !bad tag:example.com,2024:\n"),
        ErrorKind::ExpectedTagDirectiveBang
    );
}

#[test]
fn tag_directive_handle_must_start_with_bang() {
    assert_eq!(
        first_scanner_error_kind("%TAG bad! tag:example.com,2024:\n"),
        ErrorKind::ExpectedTagBang
    );
    assert_eq!(
        first_buffered_scanner_error_kind("%TAG bad! tag:example.com,2024:\n"),
        ErrorKind::ExpectedTagBang
    );
}

#[test]
fn tag_directive_prefix_must_start_with_tag_character() {
    assert_eq!(
        first_scanner_error_kind("%TAG !e! `bad\n"),
        ErrorKind::InvalidGlobalTagCharacter
    );
}

#[test]
fn tag_directive_prefix_must_end_before_invalid_content() {
    assert_eq!(
        first_scanner_error_kind("%TAG !e! tag:example.com^suffix\n"),
        ErrorKind::InvalidTagDirectiveTerminator
    );
}

#[test]
fn tag_requires_separation_after_suffix() {
    assert_eq!(
        first_scanner_error_kind("!foo,bar\n"),
        ErrorKind::InvalidTagTerminator
    );
}

#[test]
fn verbatim_tag_requires_uri() {
    assert_eq!(
        first_scanner_error_kind("!<> foo\n"),
        ErrorKind::MissingTagUri
    );
}

#[test]
fn verbatim_tag_requires_closing_angle_bracket() {
    assert_eq!(
        first_scanner_error_kind("!<tag:yaml.org,2002:str foo\n"),
        ErrorKind::UnclosedVerbatimTag
    );
}

#[test]
fn tag_uri_escape_requires_hex_digits() {
    assert_eq!(
        first_scanner_error_kind("!!bad%zz foo\n"),
        ErrorKind::InvalidTagEscape
    );
}

#[test]
fn tag_uri_escape_rejects_bad_leading_utf8_byte() {
    assert_eq!(
        first_scanner_error_kind("!!bad%80 foo\n"),
        ErrorKind::InvalidTagUtf8LeadingByte
    );
}

#[test]
fn tag_uri_escape_rejects_bad_trailing_utf8_byte() {
    assert_eq!(
        first_scanner_error_kind("!!bad%C2%41 foo\n"),
        ErrorKind::InvalidTagUtf8TrailingByte
    );
}

#[test]
fn tag_uri_escape_rejects_invalid_utf8_codepoint() {
    assert_eq!(
        first_scanner_error_kind("!!bad%F4%90%80%80 foo\n"),
        ErrorKind::InvalidTagUtf8
    );
}

#[test]
fn anchors_and_aliases_require_names() {
    assert_eq!(
        first_scanner_error_kind("& \n"),
        ErrorKind::MissingAnchorOrAliasName
    );
    assert_eq!(
        first_scanner_error_kind("* \n"),
        ErrorKind::MissingAnchorOrAliasName
    );
}

#[test]
fn document_end_marker_rejects_trailing_content() {
    assert_eq!(
        first_scanner_error_kind("... trailing\n"),
        ErrorKind::InvalidDocumentEnd
    );
}

#[test]
fn reserved_indicators_are_rejected_outside_directives() {
    let error = first_scanner_error(" @\n");

    assert_eq!(
        error.kind(),
        &ErrorKind::UnexpectedCharacter { character: '@' }
    );
}

#[test]
fn flow_block_entry_indicator_is_rejected() {
    assert_eq!(
        first_scanner_error_kind("[- ]\n"),
        ErrorKind::BlockEntryInFlowCollection
    );
}

#[test]
fn block_entry_after_tabbed_separator_reports_specific_error() {
    assert_eq!(
        first_scanner_error_kind("-\t- value\n"),
        ErrorKind::InvalidBlockEntryWhitespace
    );
}

#[test]
fn document_indicator_reports_unclosed_flow_collection() {
    let error = first_scanner_error("[\n---\n");

    assert_eq!(
        error.kind(),
        &ErrorKind::UnclosedFlowCollection { open: '[' }
    );
}

#[test]
fn block_scalar_header_rejects_trailing_content() {
    assert_eq!(
        first_scanner_error_kind("|+ trailing\n"),
        ErrorKind::InvalidBlockScalarHeader
    );
}

#[test]
fn block_scalar_rejects_zero_indent_indicator() {
    assert_eq!(
        first_scanner_error_kind("|0\n"),
        ErrorKind::ZeroBlockScalarIndent
    );
    assert_eq!(
        first_scanner_error_kind("|+0\n"),
        ErrorKind::ZeroBlockScalarIndent
    );
}

#[test]
fn root_explicit_indent_block_scalar_rejects_underindented_content() {
    assert_eq!(
        first_scanner_error_kind("|2\nx\n"),
        ErrorKind::InvalidBlockScalarIndent
    );
}

#[test]
fn quoted_scalar_rejects_document_indicator_at_line_start() {
    assert_eq!(
        first_scanner_error_kind("\"one\n---\ntwo\"\n"),
        ErrorKind::DocumentIndicatorInQuotedScalar
    );
}

#[test]
fn quoted_scalar_rejects_tab_indentation_after_line_break() {
    assert_eq!(
        first_scanner_error_kind("a: \"one\n\tbad\"\n"),
        ErrorKind::TabInIndentation
    );
}

#[test]
fn quoted_scalar_rejects_underindented_continuation() {
    assert_eq!(
        first_scanner_error_kind("a: \"one\nbad\"\n"),
        ErrorKind::InvalidQuotedScalarIndent
    );
    assert_eq!(
        first_scanner_error_kind("a:\n  b: 'one\n  bad'\n"),
        ErrorKind::InvalidQuotedScalarIndent
    );
}

#[test]
fn quoted_scalar_continuation_one_column_past_block_indent() {
    // s-flow-line-prefix(n): a continuation line needs only n spaces, where n is one more
    // than the indentation of the enclosing block mapping or sequence.
    for input in [
        "a: \"one\n two\"\n",
        "a: 'one\n two'\n",
        "a:\n  b: \"one\n   two\"\n",
        "- a: \"one\n   two\"\n",
        "? k\n: \"one\n two\"\n",
    ] {
        let mut scanner = Scanner::new(StrInput::new(input));
        loop {
            match scanner.next().transpose() {
                Ok(Some(_)) => {}
                Ok(None) => break,
                Err(error) => panic!("{input:?}: {error}"),
            }
        }
    }
}

#[test]
fn quoted_scalar_trailing_content_error_names_quote_style() {
    assert_eq!(
        first_scanner_error_kind("'foo' trailing\n"),
        ErrorKind::InvalidTrailingSingleQuotedScalar
    );
    assert_eq!(
        first_scanner_error_kind("\"foo\" trailing\n"),
        ErrorKind::InvalidTrailingDoubleQuotedScalar
    );
}

#[test]
fn quoted_scalar_escape_errors_cover_hex_and_surrogate_edges() {
    assert_eq!(
        first_scanner_error_kind("\"\\xG0\"\n"),
        ErrorKind::InvalidQuotedScalarHexEscape
    );
    assert_eq!(
        first_scanner_error_kind("\"\\uD800\\uGGGG\"\n"),
        ErrorKind::InvalidLowSurrogateHexEscape
    );
    assert_eq!(
        first_scanner_error_kind("\"\\uD800\\u0041\"\n"),
        ErrorKind::InvalidLowSurrogate
    );
    assert_eq!(
        first_scanner_error_kind("\"\\U00110000\"\n"),
        ErrorKind::InvalidUnicodeEscape
    );
}

#[test]
fn required_simple_key_requires_value_at_stream_end() {
    let error = first_scanner_error("a:\n&b\n- c\n");

    assert_eq!(error.kind(), &ErrorKind::SimpleKeyExpected);
    assert_eq!(error.marker().index(), 3);
    assert_eq!(error.marker().line(), 2);
    assert_eq!(error.marker().col(), 0);
}

#[test]
fn plain_scalar_rejects_dash_before_flow_indicator() {
    assert_eq!(
        first_scanner_error_kind("[-]\n"),
        ErrorKind::PlainScalarStartsWithIndicatorBeforeFlowIndicator { indicator: '-' }
    );
}

#[test]
fn explicit_key_rejects_tab_used_as_indentation() {
    assert_eq!(
        first_scanner_error_kind("?\n\tfoo\n"),
        ErrorKind::TabNotAllowed
    );
}

#[test]
fn flow_mapping_rejects_adjacent_collection_value_after_plain_key() {
    assert_eq!(
        first_scanner_error_kind("[a:[]]\n"),
        ErrorKind::FlowMappingValueAdjacentCollection
    );
}

#[test]
fn implicit_flow_mapping_colon_cannot_move_to_next_line() {
    assert_eq!(
        first_scanner_error_kind("[foo\n: bar]\n"),
        ErrorKind::InvalidColonPlacement
    );
}
