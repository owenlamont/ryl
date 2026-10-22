use ryl_yaml_parser::ErrorKind;
use ryl_yaml_parser::input::{Input, SkipTabs, WhitespaceResult};

struct MinimalInput;

impl Input for MinimalInput {
    fn lookahead(&mut self, _count: usize) {}

    fn buflen(&self) -> usize {
        0
    }

    fn bufmaxlen(&self) -> usize {
        0
    }

    fn raw_read_ch(&mut self) -> char {
        '\0'
    }

    fn raw_read_non_breakz_ch(&mut self) -> Option<char> {
        None
    }

    fn skip(&mut self) {}

    fn skip_n(&mut self, _count: usize) {}

    fn peek(&self) -> char {
        '\0'
    }

    fn peek_nth(&self, _n: usize) -> char {
        '\0'
    }
}

#[test]
fn default_slice_bytes_returns_none() {
    let mut input = MinimalInput;

    input.lookahead(4);
    assert_eq!(input.buflen(), 0);
    assert_eq!(input.bufmaxlen(), 0);
    assert_eq!(input.raw_read_ch(), '\0');
    assert_eq!(input.raw_read_non_breakz_ch(), None);
    input.skip();
    input.skip_n(2);
    assert_eq!(input.peek(), '\0');
    assert_eq!(input.peek_nth(1), '\0');
    assert_eq!(input.byte_offset(), None);
    assert_eq!(input.slice_bytes(0, 0), None);
}

#[test]
fn default_skip_ws_to_eol_rejects_unseparated_comment() {
    for policy in [SkipTabs::Yes, SkipTabs::No] {
        let mut input = ryl_yaml_parser::BufferedInput::new("#é中\n".chars());

        let (consumed, result) = input.skip_ws_to_eol(policy);

        assert_eq!(consumed, 0);
        assert_eq!(result, Err(ErrorKind::CommentNotSeparated));
        assert_eq!(input.peek(), '#');
    }
}

#[test]
fn whitespace_result_flags_are_independent() {
    const TAB_ONLY: WhitespaceResult = WhitespaceResult::new(true, false);
    const TAB_FLAGS: (bool, bool) =
        (TAB_ONLY.found_tabs(), TAB_ONLY.has_valid_yaml_ws());
    assert_eq!(TAB_FLAGS, (true, false));

    for found_tabs in [false, true] {
        for found_spaces in [false, true] {
            let result = WhitespaceResult::new(found_tabs, found_spaces);
            assert_eq!(result.found_tabs(), found_tabs);
            assert_eq!(result.has_valid_yaml_ws(), found_spaces);
        }
    }
}

#[test]
fn default_skip_ws_to_eol_preserves_policy_flags_and_character_counts() {
    for (source, policy, consumed, found_tabs, found_spaces, next) in [
        ("", SkipTabs::Yes, 0, false, false, '\0'),
        ("  value", SkipTabs::Yes, 2, false, true, 'v'),
        ("\tvalue", SkipTabs::Yes, 1, true, false, 'v'),
        (" \t#é中\r\n", SkipTabs::Yes, 5, true, true, '\r'),
        ("\t# é 中\n", SkipTabs::Yes, 6, true, false, '\n'),
        (" #é中", SkipTabs::Yes, 4, false, true, '\0'),
        (" \t#é中\n", SkipTabs::No, 1, false, true, '\t'),
        ("\t#é中\n", SkipTabs::No, 0, false, false, '\t'),
    ] {
        let mut input = ryl_yaml_parser::BufferedInput::new(source.chars());
        assert_eq!(
            input.skip_ws_to_eol(policy),
            (
                consumed,
                Ok(WhitespaceResult::new(found_tabs, found_spaces))
            ),
            "source: {source:?}",
        );
        assert_eq!(input.peek(), next, "source: {source:?}");
    }
}

#[test]
fn default_skip_ws_to_eol_blanks_preserves_comments_and_unskipped_tabs() {
    for (source, policy, consumed, found_tabs, found_spaces, next) in [
        ("", SkipTabs::Yes, 0, false, false, '\0'),
        ("  #é中\n", SkipTabs::Yes, 2, false, true, '#'),
        ("\t#é中\n", SkipTabs::Yes, 1, true, false, '#'),
        (" \t#é中\n", SkipTabs::Yes, 2, true, true, '#'),
        (" \t#é中\n", SkipTabs::No, 1, false, true, '\t'),
        ("\t#é中\n", SkipTabs::No, 0, false, false, '\t'),
    ] {
        let mut input = ryl_yaml_parser::BufferedInput::new(source.chars());
        assert_eq!(
            input.skip_ws_to_eol_blanks(policy),
            (consumed, WhitespaceResult::new(found_tabs, found_spaces)),
            "source: {source:?}",
        );
        assert_eq!(input.peek(), next, "source: {source:?}");
    }
}
