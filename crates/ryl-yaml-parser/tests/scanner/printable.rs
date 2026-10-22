use proptest::prelude::*;
use proptest::sample::select;
use ryl_yaml_parser::{ErrorKind, Parser};

/// The first character a block scalar rejects as not `c-printable`, if any.
fn rejected_block_scalar_character(content: &str) -> Option<char> {
    let yaml = format!("|\n  {content}\n");
    Parser::new_from_str(&yaml)
        .find_map(Result::err)
        .map(|error| match error.kind() {
            ErrorKind::UnexpectedCharacter { character } => *character,
            kind => panic!("{yaml:?}: unexpected error {kind:?}"),
        })
}

fn is_printable(character: char) -> bool {
    matches!(
        character,
        '\t' | '\n' | '\r' | '\x20'..='\x7e' | '\u{85}' | '\u{a0}'..='\u{d7ff}'
            | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}'
    )
}

#[test]
fn long_block_scalar_lines_reject_controls_after_the_word_scan() {
    let prefix = "x".repeat(80);
    assert_eq!(
        rejected_block_scalar_character(&format!("{prefix}\u{80}\u{7f}")),
        Some('\u{80}')
    );
    assert_eq!(
        rejected_block_scalar_character(&format!("{prefix}é\u{10000}")),
        None
    );
}

#[test]
fn short_block_scalar_lines_reject_controls() {
    assert_eq!(rejected_block_scalar_character("a\u{1}b"), Some('\u{1}'));
    assert_eq!(rejected_block_scalar_character("a\u{feff}b"), None);
}

proptest! {
    #[test]
    fn block_scalars_reject_the_first_non_printable_character(
        prefix_len in 0usize..96,
        suffix in select(vec![
            "plain", "\tafter", "éafter", "\u{85}after", "\u{80}after", "\u{7f}after",
            "é\u{7f}after", "\u{85}\u{9f}after", "\u{10000}\u{ffff}after", "\u{fffe}",
            "\u{d7ff}\u{e000}", "\0", "\u{1f}", "\u{a0}",
        ]),
    ) {
        let content = "x".repeat(prefix_len) + suffix;
        prop_assert_eq!(
            rejected_block_scalar_character(&content),
            content.chars().find(|&character| !is_printable(character))
        );
    }
}
