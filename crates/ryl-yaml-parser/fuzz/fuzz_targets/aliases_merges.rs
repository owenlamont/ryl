#![cfg_attr(not(test), no_main)]

#[cfg(not(test))]
mod common;
#[cfg(test)]
use crate::common;

use common::parse_with_both_inputs;
use granit_parser::Parser;
#[cfg(not(test))]
use libfuzzer_sys::fuzz_target;

const MAX_INPUT_LEN: usize = 8 * 1024;

fn quoted_scalar(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('"');
    for character in value.chars() {
        match character {
            '\0' => quoted.push_str("\\0"),
            '\x07' => quoted.push_str("\\a"),
            '\x08' => quoted.push_str("\\b"),
            '\t' => quoted.push_str("\\t"),
            '\n' => quoted.push_str("\\n"),
            '\x0b' => quoted.push_str("\\v"),
            '\x0c' => quoted.push_str("\\f"),
            '\r' => quoted.push_str("\\r"),
            '\x1b' => quoted.push_str("\\e"),
            '\u{85}' => quoted.push_str("\\N"),
            '\u{2028}' => quoted.push_str("\\L"),
            '\u{2029}' => quoted.push_str("\\P"),
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '\u{feff}' | '\u{fffe}' | '\u{ffff}' => quoted.push('_'),
            character if character.is_control() => quoted.push('_'),
            character => quoted.push(character),
        }
    }
    quoted.push('"');
    quoted
}

fn assert_aliases_reference_preceding_anchors(input: &str) {
    let events = Parser::new_from_str(input)
        .collect::<Result<Vec<_>, _>>()
        .expect("generated anchor and alias YAML must parse");
    let mut anchors = Vec::new();
    let mut aliases = 0usize;

    for (event, _) in events {
        if let Some(anchor_id) = event.anchor_id() {
            assert_ne!(anchor_id, 0);
            assert!(!anchors.contains(&anchor_id));
            anchors.push(anchor_id);
        }
        if let Some(alias_id) = event.alias_id() {
            assert!(anchors.contains(&alias_id));
            aliases += 1;
        }
    }

    assert_eq!(anchors.len(), 2);
    assert_eq!(aliases, 2);
}

// Select one construction per iteration. Half are guaranteed-valid documents with
// escaped payloads and semantic alias checks; the others deliberately expose raw
// input to malformed anchor, alias, flow-mapping, and merge-key paths.
pub fn check_input(data: &[u8]) {
    if data.len() > MAX_INPUT_LEN {
        return;
    }

    let (selector, payload_bytes): (u8, &[u8]) = match data.split_first() {
        Some((&selector, payload)) => (selector, payload),
        None => (0, &[]),
    };
    let Ok(payload) = core::str::from_utf8(payload_bytes) else {
        return;
    };

    let yaml = match selector % 4 {
        0 => {
            let scalar = quoted_scalar(payload);
            let yaml = format!(
                "value: &A {scalar}\nvalue_alias: *A\nsequence: &S [{scalar}, 2]\nsequence_alias: *S\n"
            );
            assert_aliases_reference_preceding_anchors(&yaml);
            yaml
        }
        1 => {
            let scalar = quoted_scalar(payload);
            let yaml = format!(
                "base1: &B1 {{k: 1, v: {scalar}}}\nbase2: &B2 {{k: 2, w: {scalar}}}\nmerged: {{<<: [*B1, *B2], extra: 3}}\n"
            );
            assert_aliases_reference_preceding_anchors(&yaml);
            yaml
        }
        2 => format!("anchored: &{payload} value\nalias: *{payload}\n"),
        _ => format!(
            "base1: &B1 {{k: 1, v: {payload}}}\nbase2: &B2 {{k: 2, w: {payload}}}\nmerged: {{<<: [*B1, *B2], extra: 3}}\n"
        ),
    };

    parse_with_both_inputs(&yaml);
}

#[cfg(not(test))]
fuzz_target!(|data: &[u8]| check_input(data));
