#![cfg(feature = "lsp")]
//! Tier-1 property tests for the language server's pure bridges: the
//! position/URI encoder and the lint/fix-to-LSP analysis layer. They reuse the
//! engine's YAML document generator and pound on the genuinely-new code — the
//! UTF-8/16/32 column math and `file:` URI parsing — over random, multibyte input.
//!
//! Invariants pinned here:
//! - `problem_range` is bounded, ordered across encodings (bytes >= UTF-16 units
//!   >= code points), and monotone in the column.
//! - diagnostics are identical across encodings except for the (consistently
//!   ordered) column, and every range is well-formed.
//! - `uri_to_path` is total (never panics, whatever the input).
//! - the fix-all edit, applied via an *independent* position->byte converter,
//!   reproduces `apply_safe_fixes` exactly (so `full_range` covers the document).

#[path = "property_check/harness.rs"]
#[allow(dead_code)] // shared harness; this suite uses only a few of its helpers
mod harness;
#[path = "property_check/strategy.rs"]
mod strategy;

use std::path::Path;

use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;

use ryl::config::SourceKind;
use ryl::fix::apply_safe_fixes;
use ryl::lsp::analysis::{diagnostics, fix_all_edit};
use ryl::lsp::encoding::{PositionEncoding, problem_range, uri_to_path};

use harness::trigger_all_config;
use strategy::arb_document;

const ENCODINGS: [PositionEncoding; 3] = [
    PositionEncoding::Utf8,
    PositionEncoding::Utf16,
    PositionEncoding::Utf32,
];

fn lint_path() -> &'static Path {
    Path::new("in.yaml")
}

fn base() -> &'static Path {
    Path::new(".")
}

/// One line of text: a mix of ASCII and arbitrary (including astral-plane)
/// scalars, with line breaks filtered out so it is genuinely a single line.
fn arb_line() -> impl Strategy<Value = String> {
    prop::collection::vec(any::<char>(), 0..16).prop_map(|chars| {
        chars
            .into_iter()
            .filter(|ch| *ch != '\n' && *ch != '\r')
            .collect()
    })
}

/// URIs biased toward `file:` forms (plus arbitrary strings) to exercise the
/// parser's branches while checking it is total.
fn arb_uri() -> impl Strategy<Value = String> {
    prop_oneof![
        any::<String>(),
        "(?i)file:(//[^/\r\n]*)?(/[^\r\n]*)?",
        "[a-z]+:[^\r\n]*",
    ]
}

/// Independent (inverse) LSP-position-to-byte converter, CR-aware like the engine
/// but written separately, so the fix-all round-trip cross-checks the encoder
/// rather than re-using it.
fn position_to_byte(
    text: &str,
    line: u32,
    character: u32,
    enc: PositionEncoding,
) -> usize {
    let bytes = text.as_bytes();
    let mut byte = 0;
    let mut current_line = 0;
    while current_line < line && byte < bytes.len() {
        match bytes[byte] {
            b'\r' => {
                byte += 1;
                if bytes.get(byte) == Some(&b'\n') {
                    byte += 1;
                }
                current_line += 1;
            }
            b'\n' => {
                byte += 1;
                current_line += 1;
            }
            _ => byte += 1,
        }
    }
    let target = character as usize;
    let mut units = 0;
    while units < target
        && byte < bytes.len()
        && bytes[byte] != b'\r'
        && bytes[byte] != b'\n'
    {
        let ch = text[byte..]
            .chars()
            .next()
            .expect("byte is a char boundary");
        units += match enc {
            PositionEncoding::Utf8 => ch.len_utf8(),
            PositionEncoding::Utf16 => ch.len_utf16(),
            PositionEncoding::Utf32 => 1,
        };
        byte += ch.len_utf8();
    }
    byte
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 512,
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
            "tests/proptest-regressions/property_lsp.txt",
        ))),
        ..ProptestConfig::default()
    })]

    #[test]
    fn problem_range_encoding_invariants(line in arb_line(), column in 1usize..24) {
        let lines = [line.as_str()];
        let utf8 = problem_range(&lines, 1, column, PositionEncoding::Utf8);
        let utf16 = problem_range(&lines, 1, column, PositionEncoding::Utf16);
        let utf32 = problem_range(&lines, 1, column, PositionEncoding::Utf32);

        for range in [utf8, utf16, utf32] {
            prop_assert_eq!(range.start.line, 0);
            prop_assert_eq!(range.end.line, 0);
            prop_assert!(range.start.character <= range.end.character);
        }
        // bytes >= UTF-16 units >= code points for the same point.
        prop_assert!(utf8.start.character >= utf16.start.character);
        prop_assert!(utf16.start.character >= utf32.start.character);
        // never past the line's length in the given units.
        prop_assert!(usize::try_from(utf32.start.character).unwrap() <= line.chars().count());
        prop_assert!(usize::try_from(utf8.start.character).unwrap() <= line.len());
        // monotone in the column.
        let next = problem_range(&lines, 1, column + 1, PositionEncoding::Utf16);
        prop_assert!(next.start.character >= utf16.start.character);
    }

    #[test]
    fn diagnostics_are_consistent_across_encodings(document in arb_document()) {
        let content = document.render();
        let render = |enc| {
            diagnostics(&content, lint_path(), trigger_all_config(), base(), SourceKind::Yaml, enc)
        };
        let utf8 = render(PositionEncoding::Utf8);
        let utf16 = render(PositionEncoding::Utf16);
        let utf32 = render(PositionEncoding::Utf32);
        prop_assert_eq!(utf8.len(), utf16.len());
        prop_assert_eq!(utf16.len(), utf32.len());
        for ((a, b), c) in utf8.iter().zip(&utf16).zip(&utf32) {
            prop_assert_eq!(a.range.start.line, b.range.start.line);
            prop_assert_eq!(&a.code, &b.code);
            prop_assert_eq!(&a.message, &b.message);
            prop_assert!(a.range.start.character >= b.range.start.character);
            prop_assert!(b.range.start.character >= c.range.start.character);
            prop_assert!(a.range.start.character <= a.range.end.character);
        }
    }

    #[test]
    fn uri_to_path_is_total(uri in arb_uri()) {
        let _ = uri_to_path(&uri);
    }

    #[test]
    fn fix_all_edit_round_trips(document in arb_document()) {
        let text = document.render();
        let expected =
            apply_safe_fixes(&text, trigger_all_config(), lint_path(), base());
        for enc in ENCODINGS {
            let Some(edit) = fix_all_edit(
                &text,
                lint_path(),
                trigger_all_config(),
                base(),
                SourceKind::Yaml,
                enc,
            ) else {
                continue;
            };
            let start =
                position_to_byte(&text, edit.range.start.line, edit.range.start.character, enc);
            let end =
                position_to_byte(&text, edit.range.end.line, edit.range.end.character, enc);
            let mut applied = text.clone();
            applied.replace_range(start..end, &edit.new_text);
            prop_assert_eq!(
                applied,
                expected.clone(),
                "applying the fix-all edit must reproduce apply_safe_fixes"
            );
        }
    }
}
