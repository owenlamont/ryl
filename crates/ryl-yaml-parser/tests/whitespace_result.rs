use granit_parser::{
    input::{SkipTabs, WhitespaceResult},
    BufferedInput, ErrorKind, Input, StrInput,
};

const NO_WHITESPACE: WhitespaceResult = WhitespaceResult::new(false, false);
const ONLY_TABS: WhitespaceResult = WhitespaceResult::new(true, false);
const ONLY_SPACES: WhitespaceResult = WhitespaceResult::new(false, true);
const TABS_AND_SPACES: WhitespaceResult = WhitespaceResult::new(true, true);

#[test]
fn whitespace_result_tracks_tabs_and_spaces_independently() {
    // Both construction and inspection are usable in constant expressions.
    const TABS_FOUND: bool = ONLY_TABS.found_tabs();
    const SPACES_FOUND: bool = ONLY_TABS.has_valid_yaml_ws();
    assert_eq!((TABS_FOUND, SPACES_FOUND), (true, false));

    for (result, tabs, spaces) in [
        (NO_WHITESPACE, false, false),
        (ONLY_TABS, true, false),
        (ONLY_SPACES, false, true),
        (TABS_AND_SPACES, true, true),
    ] {
        assert_eq!(result.found_tabs(), tabs);
        assert_eq!(result.has_valid_yaml_ws(), spaces);
        assert_eq!(result, WhitespaceResult::new(tabs, spaces));
    }
}

#[derive(Clone, Copy)]
enum Hook {
    ToEol,
    Blanks,
}

struct Case {
    source: &'static str,
    policy: SkipTabs,
    consumed: usize,
    result: Result<WhitespaceResult, ErrorKind>,
    remaining: &'static str,
}

fn policy_name(policy: SkipTabs) -> &'static str {
    // Exhaustive matching also verifies that policy no longer includes a result variant.
    match policy {
        SkipTabs::Yes => "Yes",
        SkipTabs::No => "No",
    }
}

fn run_hook<T: Input>(
    input: &mut T,
    hook: Hook,
    policy: SkipTabs,
) -> (usize, Result<WhitespaceResult, ErrorKind>) {
    match hook {
        Hook::ToEol => input.skip_ws_to_eol(policy),
        Hook::Blanks => {
            let (consumed, result) = input.skip_ws_to_eol_blanks(policy);
            (consumed, Ok(result))
        }
    }
}

fn check_case<T: Input>(mut input: T, case: &Case, hook: Hook, lookahead: usize) {
    input.lookahead(lookahead);
    let (consumed, result) = run_hook(&mut input, hook, case.policy);

    assert_eq!(
        (consumed, &result),
        (case.consumed, &case.result),
        "source {:?}, SkipTabs::{}, lookahead {lookahead}",
        case.source,
        policy_name(case.policy),
    );
    if let Some(byte_offset) = input.byte_offset() {
        // Counts are characters, whereas the stable input's offset is bytes.
        assert_eq!(byte_offset, case.source.len() - case.remaining.len());
    }

    if result.is_err() {
        // Failed comment separation consumes neither the '#' nor any following text.
        assert_eq!(run_hook(&mut input, hook, case.policy), (consumed, result));
    }

    // Check the complete remaining stream, including CRLF and embedded NUL, not only its front.
    let mut remaining = String::new();
    for _ in case.remaining.chars() {
        remaining.push(input.look_ch());
        input.skip();
    }
    assert_eq!(remaining, case.remaining, "source {:?}", case.source);
    assert_eq!(input.look_ch(), '\0', "source {:?}", case.source);
}

fn check_backends(cases: &[Case], hook: Hook) {
    for case in cases {
        for lookahead in [0, 1, 4, 16] {
            check_case(StrInput::new(case.source), case, hook, lookahead);
            // BufferedInput exercises the trait's default implementation, not StrInput's hook.
            check_case(
                BufferedInput::new(case.source.chars()),
                case,
                hook,
                lookahead,
            );
        }
    }
}

#[test]
fn skip_to_eol_matches_default_input_for_comments_unicode_breaks_and_eof() {
    let cases = [
        ("", SkipTabs::Yes, 0, Ok(NO_WHITESPACE), ""),
        ("  ", SkipTabs::Yes, 2, Ok(ONLY_SPACES), ""),
        ("\t\t", SkipTabs::Yes, 2, Ok(ONLY_TABS), ""),
        (" \t ", SkipTabs::Yes, 3, Ok(TABS_AND_SPACES), ""),
        ("\t# note", SkipTabs::Yes, 7, Ok(ONLY_TABS), ""),
        (" # note", SkipTabs::Yes, 7, Ok(ONLY_SPACES), ""),
        (
            "  \t # note\nx",
            SkipTabs::Yes,
            10,
            Ok(TABS_AND_SPACES),
            "\nx",
        ),
        (
            "\t# é 中\t\r\nnext",
            SkipTabs::Yes,
            7,
            Ok(ONLY_TABS),
            "\r\nnext",
        ),
        (
            "  #é中🙂\r\nnext",
            SkipTabs::Yes,
            6,
            Ok(ONLY_SPACES),
            "\r\nnext",
        ),
        (" #中\rnext", SkipTabs::Yes, 3, Ok(ONLY_SPACES), "\rnext"),
        (" #中\0next", SkipTabs::Yes, 3, Ok(ONLY_SPACES), "\0next"),
        ("\nnext", SkipTabs::Yes, 0, Ok(NO_WHITESPACE), "\nnext"),
        (" é中", SkipTabs::Yes, 1, Ok(ONLY_SPACES), "é中"),
        (
            "\u{a0}#note",
            SkipTabs::Yes,
            0,
            Ok(NO_WHITESPACE),
            "\u{a0}#note",
        ),
        ("\t#note", SkipTabs::No, 0, Ok(NO_WHITESPACE), "\t#note"),
        ("  \t#note", SkipTabs::No, 2, Ok(ONLY_SPACES), "\t#note"),
        ("  #é中", SkipTabs::No, 5, Ok(ONLY_SPACES), ""),
        (
            "#é中\r\nnext",
            SkipTabs::Yes,
            0,
            Err(ErrorKind::CommentNotSeparated),
            "#é中\r\nnext",
        ),
        (
            "#é中",
            SkipTabs::No,
            0,
            Err(ErrorKind::CommentNotSeparated),
            "#é中",
        ),
    ]
    .map(|(source, policy, consumed, result, remaining)| Case {
        source,
        policy,
        consumed,
        result,
        remaining,
    });

    check_backends(&cases, Hook::ToEol);
}

#[test]
fn skip_blanks_matches_default_input_and_preserves_comments_and_breaks() {
    let cases = [
        ("", SkipTabs::Yes, 0, NO_WHITESPACE, ""),
        ("  ", SkipTabs::Yes, 2, ONLY_SPACES, ""),
        ("\t\t", SkipTabs::Yes, 2, ONLY_TABS, ""),
        (" \t ", SkipTabs::Yes, 3, TABS_AND_SPACES, ""),
        ("#é中", SkipTabs::Yes, 0, NO_WHITESPACE, "#é中"),
        (
            " #é中\r\nnext",
            SkipTabs::Yes,
            1,
            ONLY_SPACES,
            "#é中\r\nnext",
        ),
        ("\t# é 中", SkipTabs::Yes, 1, ONLY_TABS, "# é 中"),
        (" \t#note", SkipTabs::Yes, 2, TABS_AND_SPACES, "#note"),
        ("  \r\nnext", SkipTabs::Yes, 2, ONLY_SPACES, "\r\nnext"),
        (" \nnext", SkipTabs::Yes, 1, ONLY_SPACES, "\nnext"),
        (" \rnext", SkipTabs::Yes, 1, ONLY_SPACES, "\rnext"),
        (" \0next", SkipTabs::Yes, 1, ONLY_SPACES, "\0next"),
        (" é中", SkipTabs::Yes, 1, ONLY_SPACES, "é中"),
        (
            "\u{a0}#note",
            SkipTabs::Yes,
            0,
            NO_WHITESPACE,
            "\u{a0}#note",
        ),
        ("\t#note", SkipTabs::No, 0, NO_WHITESPACE, "\t#note"),
        ("  \t#note", SkipTabs::No, 2, ONLY_SPACES, "\t#note"),
        ("  #é中", SkipTabs::No, 2, ONLY_SPACES, "#é中"),
    ]
    .map(|(source, policy, consumed, result, remaining)| Case {
        source,
        policy,
        consumed,
        result: Ok(result),
        remaining,
    });

    check_backends(&cases, Hook::Blanks);
}
