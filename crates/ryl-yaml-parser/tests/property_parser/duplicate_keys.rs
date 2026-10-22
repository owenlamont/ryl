use super::common;

use common::{parse_with_both_inputs, quoted_scalar};
use ryl_yaml_parser::Parser;

const MAX_INPUT_LEN: usize = 8 * 1024;
const MAX_SIMPLE_KEY_BYTES: usize = 384;

fn assert_duplicate_scalar_key_is_preserved(input: &str) {
    let events = Parser::new_from_str(input)
        .collect::<Result<Vec<_>, _>>()
        .expect("generated duplicate-key YAML must parse");
    let scalars = events
        .iter()
        .filter_map(|(event, _)| event.scalar().map(|(value, _)| value))
        .collect::<Vec<_>>();

    // Both valid constructions contain exactly two scalar key/value pairs.
    assert_eq!(scalars.len(), 4);
    assert_eq!(scalars[0], scalars[2]);
}

fn simple_key_payload(value: &str) -> &str {
    let mut end = value.len().min(MAX_SIMPLE_KEY_BYTES);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

// The parser is event-based and intentionally preserves duplicate mapping keys.
// Select one construction per iteration: valid escaped keys exercise successful
// block/flow parsing, while raw cases retain malformed and nested syntax coverage.
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

    let yaml = match selector % 5 {
        0 => {
            let key = quoted_scalar(simple_key_payload(payload));
            let value = quoted_scalar(payload);
            let yaml = format!("{key}: {value}\n{key}: {value}\n");
            assert_duplicate_scalar_key_is_preserved(&yaml);
            yaml
        }
        1 => {
            let key = quoted_scalar(simple_key_payload(payload));
            let value = quoted_scalar(payload);
            let yaml = format!("{{{key}: {value}, {key}: {value}}}\n");
            assert_duplicate_scalar_key_is_preserved(&yaml);
            yaml
        }
        2 => format!("a: 1\na: 2\nkey: {payload}\nkey: {payload}\n"),
        3 => format!(
            "outer:\n  inner: {{x: 1, x: 2}}\n  arr: [{{k: {payload}}}, {{k: {payload}}}]\n"
        ),
        _ => format!("{{'{payload}': 1, '{payload}': 2}}\n"),
    };

    parse_with_both_inputs(&yaml);
}
