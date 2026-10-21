//! Holds functions to determine if a character belongs to a specific character set.

/// Check whether the character is nil (`\0`).
#[inline]
#[must_use]
pub fn is_z(c: char) -> bool {
    c == '\0'
}

/// Check whether the character is a line break (`\r` or `\n`).
#[inline]
#[must_use]
pub fn is_break(c: char) -> bool {
    c == '\n' || c == '\r'
}

/// Check whether the character is nil or a line break (`\0`, `\r`, `\n`).
#[inline]
#[must_use]
pub fn is_breakz(c: char) -> bool {
    is_break(c) || is_z(c)
}

/// Check whether the character is a whitespace (` ` or `\t`).
#[inline]
#[must_use]
pub fn is_blank(c: char) -> bool {
    c == ' ' || c == '\t'
}

/// Check whether the character is nil, a line break, or whitespace.
///
/// `\0`, ` `, `\t`, `\n`, `\r`
#[inline]
#[must_use]
pub fn is_blank_or_breakz(c: char) -> bool {
    is_blank(c) || is_breakz(c)
}

/// Check whether the character is an ASCII digit.
#[inline]
#[must_use]
pub fn is_digit(c: char) -> bool {
    c.is_ascii_digit()
}

/// Check whether the character is an ASCII alphanumeric, `_` or `-`.
///
/// This is used for scanning tag handles and similar constructs.
/// Note: This is slightly more permissive than YAML's `ns-word-char` (which excludes `_`).
/// For strict `ns-word-char` compliance, use `is_word_char` instead.
///
/// Matches: `[0-9a-zA-Z_-]`
#[inline]
#[must_use]
pub fn is_alpha(c: char) -> bool {
    matches!(c, '0'..='9' | 'a'..='z' | 'A'..='Z' | '_' | '-')
}

/// Check whether the character is a hexadecimal character (case insensitive).
#[inline]
#[must_use]
pub fn is_hex(c: char) -> bool {
    c.is_ascii_digit() || ('a'..='f').contains(&c) || ('A'..='F').contains(&c)
}

/// Convert the hexadecimal digit to an integer.
///
/// # Panics
/// Panics if `c` is not an ASCII hexadecimal digit.
#[track_caller]
#[inline]
#[must_use]
pub fn as_hex(c: char) -> u32 {
    match c {
        '0'..='9' => (c as u32) - ('0' as u32),
        'a'..='f' => (c as u32) - ('a' as u32) + 10,
        'A'..='F' => (c as u32) - ('A' as u32) + 10,
        _ => unreachable!("as_hex called with a non-hexadecimal character"),
    }
}

/// Check whether the character is a YAML flow character (one of `,[]{}`).
#[inline]
#[must_use]
pub fn is_flow(c: char) -> bool {
    matches!(c, ',' | '[' | ']' | '{' | '}')
}

/// Check whether the character is the BOM character.
#[inline]
#[must_use]
pub fn is_bom(c: char) -> bool {
    c == '\u{FEFF}'
}

/// Check whether the character is a YAML non-breaking character.
#[inline]
#[must_use]
pub fn is_yaml_non_break(c: char) -> bool {
    is_printable(c) && !is_break(c) && !is_bom(c)
}

/// Check whether the character is a YAML printable character (`c-printable`).
#[inline]
#[must_use]
pub(crate) fn is_printable(c: char) -> bool {
    matches!(
        c as u32,
        0x0009
            | 0x000A
            | 0x000D
            | 0x0020..=0x007E
            | 0x0085
            | 0x00A0..=0xD7FF
            | 0xE000..=0xFFFD
            | 0x10000..=0x0010_FFFF
    )
}

const PRINTABLE_ASCII_FAST_PATH_MIN_BYTES: usize = 32;
const BYTE_LANES_ONES: u64 = 0x0101_0101_0101_0101;
const BYTE_LANES_HIGH_BITS: u64 = 0x8080_8080_8080_8080;
const BYTE_LANES_TOP_THREE_BITS: u64 = 0xe0e0_e0e0_e0e0_e0e0;
const BYTE_LANES_DEL: u64 = 0x7f7f_7f7f_7f7f_7f7f;

#[inline]
fn has_zero_byte(word: u64) -> bool {
    word.wrapping_sub(BYTE_LANES_ONES) & !word & BYTE_LANES_HIGH_BITS != 0
}

#[inline]
fn is_suspicious_scalar_byte(byte: u8) -> bool {
    (byte < 0x20 && !matches!(byte, b'\t' | b'\n' | b'\r')) || byte >= 0x7f
}

/// Return the first character that is not YAML `c-printable`.
///
/// Character iteration is cheaper for short strings. For longer strings, inspect eight ASCII
/// bytes at a time. Words that may contain a control, DEL, or non-ASCII byte are checked exactly;
/// a non-ASCII suffix falls back to the canonical character predicate.
#[inline]
pub(crate) fn find_non_printable(s: &str) -> Option<char> {
    if s.len() < PRINTABLE_ASCII_FAST_PATH_MIN_BYTES {
        return s.chars().find(|&character| !is_printable(character));
    }

    let bytes = s.as_bytes();
    let mut chunks = bytes.chunks_exact(8);
    let mut byte_offset = 0;
    let mut suspicious_offset = None;

    for chunk in &mut chunks {
        // u64 will always be 8 bytes, this unwrap cannot trigger.
        let word = u64::from_ne_bytes(chunk.try_into().unwrap());
        let may_have_suspicious_byte = word & BYTE_LANES_HIGH_BITS != 0
            || has_zero_byte(word & BYTE_LANES_TOP_THREE_BITS)
            || has_zero_byte(word ^ BYTE_LANES_DEL);

        if may_have_suspicious_byte {
            if let Some(chunk_offset) = chunk
                .iter()
                .position(|&byte| is_suspicious_scalar_byte(byte))
            {
                suspicious_offset = Some(byte_offset + chunk_offset);
                break;
            }
        }
        byte_offset += chunk.len();
    }

    let suspicious_offset = suspicious_offset.or_else(|| {
        chunks
            .remainder()
            .iter()
            .position(|&byte| is_suspicious_scalar_byte(byte))
            .map(|remainder_offset| byte_offset + remainder_offset)
    });

    match suspicious_offset {
        None => None,
        Some(offset) if bytes[offset].is_ascii() => Some(char::from(bytes[offset])),
        // All preceding bytes are printable ASCII, so this is the start of a UTF-8 character.
        Some(offset) => s[offset..]
            .chars()
            .find(|&character| !is_printable(character)),
    }
}

/// Check whether the character is NOT a YAML whitespace (` ` / `\t`).
#[inline]
#[must_use]
pub fn is_yaml_non_space(c: char) -> bool {
    is_yaml_non_break(c) && !is_blank(c)
}

/// Check whether the character is a valid YAML anchor name character.
#[inline]
#[must_use]
pub fn is_anchor_char(c: char) -> bool {
    is_yaml_non_space(c) && !is_flow(c) && !is_z(c)
}

/// Check whether the character is a valid YAML word character (`ns-word-char`).
///
/// Per YAML 1.2 spec: `ns-word-char ::= ns-dec-digit | ns-ascii-letter | "-"`
///
/// Matches: `[0-9a-zA-Z-]`
#[inline]
#[must_use]
pub fn is_word_char(c: char) -> bool {
    is_alpha(c) && c != '_'
}

/// Check whether the character is a valid URI character.
#[inline]
#[must_use]
pub fn is_uri_char(c: char) -> bool {
    is_word_char(c) || "#;/?:@&=+$,_.!~*\'()[]%".contains(c)
}

/// Check whether the character is a valid tag character.
#[inline]
#[must_use]
pub fn is_tag_char(c: char) -> bool {
    is_uri_char(c) && !is_flow(c) && c != '!'
}

#[cfg(test)]
mod tests {
    use alloc::string::String;

    use super::*;

    #[test]
    fn printable_ranges_include_private_and_supplementary_planes() {
        assert!(is_printable('\u{E000}'));
        assert!(is_printable('\u{10FFFF}'));
        assert!(is_yaml_non_break('\u{10000}'));
        assert!(!is_yaml_non_break('\u{FEFF}'));
        assert!(!is_yaml_non_break('\n'));
    }

    #[test]
    fn optimized_non_printable_search_matches_yaml_boundaries() {
        let printable = [
            '\t',
            '\n',
            '\r',
            ' ',
            '~',
            '\u{85}',
            '\u{a0}',
            '\u{d7ff}',
            '\u{e000}',
            '\u{feff}',
            '\u{fffd}',
            '\u{10000}',
            '\u{10ffff}',
        ];
        for character in printable {
            let mut short = String::from("before");
            short.push(character);
            short.push_str("after");
            assert_eq!(find_non_printable(&short), None, "rejected {character:?}");

            let mut long = "x".repeat(80);
            long.push(character);
            long.push_str("after");
            assert_eq!(find_non_printable(&long), None, "rejected {character:?}");
        }

        let non_printable = [
            '\0', '\u{1}', '\u{8}', '\u{b}', '\u{c}', '\u{e}', '\u{1f}', '\u{7f}', '\u{80}',
            '\u{84}', '\u{86}', '\u{9f}', '\u{fffe}', '\u{ffff}',
        ];
        for character in non_printable {
            let mut short = String::from("before");
            short.push(character);
            short.push_str("after");
            assert_eq!(
                find_non_printable(&short),
                Some(character),
                "accepted {character:?}",
            );

            let mut long = "x".repeat(80);
            long.push(character);
            long.push_str("after");
            assert_eq!(
                find_non_printable(&long),
                Some(character),
                "accepted {character:?}",
            );
        }

        let mut multiple = "x".repeat(80);
        multiple.push('\u{80}');
        multiple.push('\u{7f}');
        assert_eq!(find_non_printable(&multiple), Some('\u{80}'));
    }

    #[test]
    fn optimized_non_printable_search_matches_reference_across_chunk_boundaries() {
        let suffixes = [
            "plain",
            "\tafter",
            "\nafter",
            "\rafter",
            "éafter",
            "\u{85}after",
            "\u{80}after",
            "\u{7f}after",
            "é\u{7f}after",
            "\u{85}\u{9f}after",
            "\u{10000}\u{ffff}after",
        ];

        for prefix_len in 56..=80 {
            for suffix in suffixes {
                let input = "x".repeat(prefix_len) + suffix;
                let expected = input.chars().find(|&character| !is_printable(character));
                assert_eq!(
                    find_non_printable(&input),
                    expected,
                    "mismatch at prefix length {prefix_len} for {suffix:?}",
                );
            }
        }
    }

    #[test]
    fn word_uri_and_tag_character_sets_are_distinct() {
        assert!(is_word_char('-'));
        assert!(!is_word_char('_'));
        assert!(is_uri_char('_'));
        assert!(is_uri_char('%'));
        assert!(!is_tag_char('!'));
        assert!(!is_tag_char('['));
    }
}
