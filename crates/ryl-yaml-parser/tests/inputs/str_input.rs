use std::string::String;

use ryl_yaml_parser::{
    ErrorKind,
    input::{BorrowedInput, Input, SkipTabs, WhitespaceResult},
};

use ryl_yaml_parser::StrInput;

#[test]
pub fn is_document_start() {
    let input = StrInput::new("---\n");
    assert!(input.next_is_document_start());
    assert!(input.next_is_document_indicator());
    let input = StrInput::new("---");
    assert!(input.next_is_document_start());
    assert!(input.next_is_document_indicator());
    let input = StrInput::new("...\n");
    assert!(!input.next_is_document_start());
    assert!(input.next_is_document_indicator());
    let input = StrInput::new("--- ");
    assert!(input.next_is_document_start());
    assert!(input.next_is_document_indicator());
}

#[test]
pub fn is_document_end() {
    let input = StrInput::new("...\n");
    assert!(input.next_is_document_end());
    assert!(input.next_is_document_indicator());
    let input = StrInput::new("...");
    assert!(input.next_is_document_end());
    assert!(input.next_is_document_indicator());
    let input = StrInput::new("---\n");
    assert!(!input.next_is_document_end());
    assert!(input.next_is_document_indicator());
    let input = StrInput::new("... ");
    assert!(input.next_is_document_end());
    assert!(input.next_is_document_indicator());
}

#[test]
fn raw_reads_track_byte_offsets_and_eof() {
    let mut input = StrInput::new("aé");

    assert_eq!(input.raw_read_ch(), 'a');
    assert_eq!(input.byte_offset(), Some(1));
    assert_eq!(input.raw_read_ch(), 'é');
    assert_eq!(input.byte_offset(), Some(3));
    assert_eq!(input.raw_read_ch(), '\0');
    assert_eq!(input.byte_offset(), Some(3));
}

#[test]
fn raw_read_non_breakz_stops_before_breakz() {
    let mut input = StrInput::new("a\n");

    assert_eq!(input.raw_read_non_breakz_ch(), Some('a'));
    assert_eq!(input.raw_read_non_breakz_ch(), None);
    assert_eq!(input.peek(), '\n');

    let mut empty = StrInput::new("");
    assert_eq!(empty.raw_read_non_breakz_ch(), None);
}

#[test]
fn skip_handles_ascii_unicode_and_eof() {
    let mut input = StrInput::new("éab");

    input.skip();
    assert_eq!(input.peek(), 'a');

    input.skip_n(8);
    assert_eq!(input.peek(), '\0');

    input.skip();
    assert_eq!(input.peek(), '\0');
}

#[test]
fn peeking_past_end_returns_nul() {
    let ascii = StrInput::new("ab");
    assert_eq!(ascii.peek_nth(1), 'b');
    assert_eq!(ascii.peek_nth(3), '\0');

    let unicode = StrInput::new("éab");
    assert!(unicode.next_3_are('é', 'a', 'b'));
    assert!(!unicode.next_3_are('é', 'a', 'c'));
}

#[test]
fn skip_ws_to_eol_without_tabs_stops_before_tab() {
    let mut input = StrInput::new("  \t# comment\n");

    let (consumed, result) = input.skip_ws_to_eol(SkipTabs::No);

    assert_eq!(consumed, 2);
    let result = result.unwrap();
    assert!(!result.found_tabs());
    assert!(result.has_valid_yaml_ws());
    assert_eq!(input.peek(), '\t');
}

#[test]
fn skip_ws_to_eol_skips_comments_after_whitespace() {
    let mut input = StrInput::new("  # comment\nnext");

    let (consumed, result) = input.skip_ws_to_eol(SkipTabs::Yes);

    assert_eq!(consumed, 11);
    let result = result.unwrap();
    assert!(!result.found_tabs());
    assert!(result.has_valid_yaml_ws());
    assert_eq!(input.peek(), '\n');
}

#[test]
fn skip_ws_to_eol_rejects_unseparated_comment() {
    for policy in [SkipTabs::Yes, SkipTabs::No] {
        let mut input = StrInput::new("#é中\n");

        let (consumed, result) = input.skip_ws_to_eol(policy);

        assert_eq!(consumed, 0);
        assert_eq!(result, Err(ErrorKind::CommentNotSeparated));
        assert_eq!(input.byte_offset(), Some(0));
        assert_eq!(input.peek(), '#');
    }
}

#[test]
fn skip_ws_to_eol_counts_unicode_comments_and_tracks_whitespace_outside_them() {
    for (source, consumed, expected, byte_offset, next) in [
        ("", 0, WhitespaceResult::new(false, false), 0, '\0'),
        (" \t#é中\r\n", 5, WhitespaceResult::new(true, true), 8, '\r'),
        ("\t# é 中\n", 6, WhitespaceResult::new(true, false), 9, '\n'),
        (" #é中", 4, WhitespaceResult::new(false, true), 7, '\0'),
    ] {
        let mut input = StrInput::new(source);
        assert_eq!(
            input.skip_ws_to_eol(SkipTabs::Yes),
            (consumed, Ok(expected))
        );
        assert_eq!(input.byte_offset(), Some(byte_offset));
        assert_eq!(input.peek(), next);
    }
}

#[test]
fn skip_ws_to_eol_blanks_preserves_comments_and_unskipped_tabs() {
    for (source, policy, consumed, found_tabs, found_spaces, next) in [
        ("", SkipTabs::Yes, 0, false, false, '\0'),
        ("  #é中\n", SkipTabs::Yes, 2, false, true, '#'),
        ("\t#é中\n", SkipTabs::Yes, 1, true, false, '#'),
        (" \t#é中\n", SkipTabs::Yes, 2, true, true, '#'),
        (" \t#é中\n", SkipTabs::No, 1, false, true, '\t'),
        ("\t#é中\n", SkipTabs::No, 0, false, false, '\t'),
    ] {
        let mut input = StrInput::new(source);
        assert_eq!(
            input.skip_ws_to_eol_blanks(policy),
            (consumed, WhitespaceResult::new(found_tabs, found_spaces)),
            "source: {source:?}",
        );
        assert_eq!(input.byte_offset(), Some(consumed));
        assert_eq!(input.peek(), next, "source: {source:?}");
    }
}

#[test]
fn fetch_while_is_alpha_is_ascii_only() {
    let mut input = StrInput::new("abc_123-é");
    let mut out = String::new();

    assert_eq!(input.fetch_while_is_alpha(&mut out), 8);
    assert_eq!(out, "abc_123-");
    assert_eq!(input.peek(), 'é');
}

#[test]
fn fetch_plain_scalar_chunk_handles_non_ascii_after_colon() {
    let mut input = StrInput::new("a:é ");
    let mut out = String::new();

    assert_eq!(
        input.fetch_plain_scalar_chunk(&mut out, 16, false),
        (true, 3)
    );
    assert_eq!(out, "a:é");
    assert_eq!(input.peek(), ' ');
}

#[test]
fn fetch_plain_scalar_chunk_stops_at_flow_indicator() {
    let mut input = StrInput::new("abc,def");
    let mut out = String::new();

    assert_eq!(
        input.fetch_plain_scalar_chunk(&mut out, 16, true),
        (true, 3)
    );
    assert_eq!(out, "abc");
    assert_eq!(input.peek(), ',');
}

#[test]
fn borrowed_slices_use_original_input_lifetime() {
    let input = StrInput::new("aéz");

    assert_eq!(BorrowedInput::slice_borrowed(&input, 1, 3), Some("é"));
    assert_eq!(input.slice_bytes(3, 4), Some("z"));
}
