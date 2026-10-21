#![cfg_attr(not(test), no_main)]
#[cfg(feature = "comments")]
use granit_parser::Placement;

#[cfg(not(test))]
mod common;
#[cfg(test)]
use crate::common;

use common::{parse_with_both_inputs, parse_with_options, scan_with_options};
use granit_parser::{ErrorKind, Event, Options, Parser, Scanner, StrInput, TokenType};
#[cfg(not(test))]
use libfuzzer_sys::fuzz_target;

const MAX_INPUT_LEN: usize = 8 * 1024;
const HEX: &[u8; 16] = b"0123456789abcdef";

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

fn uri_component(value: &str) -> String {
    let mut component = String::with_capacity(value.len() * 2 + 1);
    component.push('x');
    for &byte in value.as_bytes() {
        component.push(char::from(HEX[usize::from(byte >> 4)]));
        component.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    component
}

fn assert_single_tag(
    input: &str,
    expected_handle: &str,
    expected_suffix: &str,
    expected_original_handle: &str,
    expect_alias: bool,
    options: Options,
) {
    let events = Parser::new_from_str_with_options(input, options)
        .collect::<Result<Vec<_>, _>>()
        .expect("generated directive and tag YAML must parse with sufficient resource limits");
    let tagged_events = events
        .iter()
        .filter(|(event, _)| event.tag().is_some())
        .collect::<Vec<_>>();

    assert_eq!(tagged_events.len(), 1);
    let tagged = &tagged_events[0].0;
    let tag = tagged.tag().expect("tagged event disappeared");
    assert_eq!(tag.handle(), expected_handle);
    assert_eq!(tag.suffix(), expected_suffix);
    assert_eq!(tag.original_handle(), expected_original_handle);

    let aliases = events
        .iter()
        .filter_map(|(event, _)| event.alias_id())
        .collect::<Vec<_>>();
    if expect_alias {
        let anchor_id = tagged
            .anchor_id()
            .expect("tagged node must also carry the generated anchor");
        assert_eq!(aliases, [anchor_id]);
    } else {
        assert_eq!(aliases, Vec::<usize>::new());
    }
}

fn check_reserved_directive_comment(selector: u8, payload: &str) {
    let parameter = format!(
        "{}#literal",
        uri_component(&payload.chars().take(32).collect::<String>())
    );
    let parameters = if selector & 2 == 0 {
        vec![]
    } else {
        vec![parameter]
    };
    let mut directive = String::from("FUTURE");
    for parameter in &parameters {
        directive.push(' ');
        directive.push_str(parameter);
    }
    let comment: String = payload
        .chars()
        .map(|character| match character {
            character if character.is_control() => '_',
            '\u{2028}' | '\u{2029}' | '\u{feff}' | '\u{fffe}' | '\u{ffff}' => '_',
            character => character,
        })
        .collect();
    let separator = if selector & 4 == 0 { " " } else { "\t \t" };
    let yaml = format!("%{directive}{separator}#{comment}\n---\nvalue\n");
    let mut options = Options::default();
    #[cfg(feature = "comments")]
    {
        options.emit_comments = selector & 8 != 0;
    }
    options.max_directive_bytes = directive.len();
    options.max_reserved_directive_params = parameters.len();

    let tokens = Scanner::with_options(StrInput::new(&yaml), options.clone())
        .collect::<Result<Vec<_>, _>>()
        .expect("comment text and separation must not consume directive limits");
    assert_eq!(
        tokens[1].token_type(),
        &TokenType::ReservedDirective("FUTURE".into(), parameters.clone()),
    );
    #[cfg(feature = "comments")]
    {
        let comments = tokens
            .iter()
            .filter_map(|token| match token.token_type() {
                #[cfg(feature = "comments")]
                TokenType::Comment(comment) => Some((comment, token.span())),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(comments.len(), usize::from(options.emit_comments));
        if options.emit_comments {
            assert_eq!(comments[0].0.text(), comment);
            assert_eq!(comments[0].0.placement(), Placement::Right);
            assert_eq!(
                comments[0].1.slice(&yaml),
                Some(format!("#{comment}").as_str())
            );
        }
    }
    let events = Parser::new_from_str_with_options(&yaml, options.clone())
        .collect::<Result<Vec<_>, _>>()
        .expect("a separated directive comment must preserve the following document");
    #[cfg(feature = "comments")]
    assert_eq!(
        events
            .iter()
            .filter(|(event, _)| matches!(event, Event::Comment(..)))
            .count(),
        usize::from(cfg!(feature = "comments") && options.emit_comments),
    );
    assert_eq!(
        events
            .iter()
            .filter_map(|(event, _)| match event {
                Event::Scalar(value, ..) => Some(value.as_ref()),
                _ => None,
            })
            .collect::<Vec<_>>(),
        ["value"],
    );
    parse_with_options(&yaml, options.clone());
    scan_with_options(&yaml, options.clone());

    // Removing one byte of retained storage must fail even if a valid comment follows it.
    options.max_directive_bytes -= 1;
    let error = Parser::new_from_str_with_options(&yaml, options.clone())
        .find_map(Result::err)
        .expect("comment handling must not bypass the directive byte limit");
    assert_eq!(
        error.kind(),
        &ErrorKind::DirectiveByteLimitExceeded {
            limit: options.max_directive_bytes
        }
    );
    parse_with_options(&yaml, options.clone());
    scan_with_options(&yaml, options);
}

fn check_primary_handle_override(selector: u8, payload: &str) {
    let prefix = format!(
        "tag:example.com,2026:{}:",
        uri_component(&payload.chars().take(32).collect::<String>())
    );
    let scalar = quoted_scalar(payload);
    let node = match selector & 6 {
        0 => format!("! {scalar}"),
        2 => format!("! [{scalar}]"),
        4 => format!("! {{key: {scalar}}}"),
        _ => format!("! &anchor {scalar}"),
    };
    let yaml = format!("%TAG ! {prefix}\n---\n- {node}\n- !item {scalar}\n");
    let events = Parser::new_from_str(&yaml)
        .collect::<Result<Vec<_>, _>>()
        .expect("a primary handle override must preserve the non-specific tag");
    let tags = events
        .iter()
        .filter_map(|(event, _)| event.tag())
        .collect::<Vec<_>>();
    assert_eq!(tags.len(), 2);
    assert_eq!(tags[0].parts(), ("", "!"));
    assert_eq!(tags[0].original_parts(), ("", "!"));
    assert_eq!(tags[0].original(), "!");
    assert_eq!(tags[1].parts(), (prefix.as_str(), "item"));
    assert_eq!(tags[1].original_parts(), ("!", "item"));
    assert_eq!(tags[1].original(), "!item");
    parse_with_both_inputs(&yaml);
}

// Select one construction per iteration. Valid branches sanitize directive/tag
// components and escape scalar data so tag resolution can be asserted. Raw branches
// preserve malformed directives, percent escapes, node properties, and comments.
/// Generate bounded directive and tag inputs and verify their parser and scanner behavior.
///
/// # Panics
/// Panics if generated inputs violate the expected tags, aliases, comments, or resource-limit
/// errors, or if input backends disagree.
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

    // Preserve the existing six constructions for selectors 0..240 and reserve the high
    // selectors for explicit 1.2.1 semantic assertions, one construction per iteration.
    if selector >= 240 {
        if selector & 1 == 0 {
            check_reserved_directive_comment(selector, payload);
        } else {
            check_primary_handle_override(selector, payload);
        }
        return;
    }

    let yaml = match selector % 6 {
        0 => {
            let scalar = quoted_scalar(payload);
            let prefix = format!("tag:example.com,2026:{}", uri_component(payload));
            let yaml =
                format!("%YAML 1.2\n%TAG !e! {prefix}\n---\nkey: !e!item {scalar}\n");

            let mut options = Options::default();
            let directive_bytes = b"TAG !e! ".len() + prefix.len();
            if directive_bytes > options.max_directive_bytes {
                // The default resource-limit error is valid. Require that exact error, then retry
                // with enough budget so the tag-resolution oracle still exercises this payload.
                let limit = options.max_directive_bytes;
                let error = Parser::new_from_str(&yaml)
                    .collect::<Result<Vec<_>, _>>()
                    .expect_err("an oversized generated %TAG directive must be rejected");
                assert_eq!(
                    error.kind(),
                    &ErrorKind::DirectiveByteLimitExceeded { limit }
                );
                options.max_directive_bytes = directive_bytes;
            }

            assert_single_tag(&yaml, &prefix, "item", "!e!", false, options);
            yaml
        }
        1 => {
            let scalar = quoted_scalar(payload);
            let uri = format!("tag:example.com,2026:{}", uri_component(payload));
            let yaml = format!("---\n!<{uri}> {scalar}\n");
            assert_single_tag(&yaml, "", &uri, "", false, Options::default());
            yaml
        }
        2 => {
            let scalar = quoted_scalar(payload);
            let suffix = format!("local-{}", uri_component(payload));
            let yaml = format!(
                "---\nnode:\n  &anchor !{suffix}\n  # generated\n  {scalar}\nalias: *anchor\n"
            );
            assert_single_tag(&yaml, "!", &suffix, "!", true, Options::default());
            yaml
        }
        3 => format!(
            "%YAML 1.2\n%TAG !e! tag:example.com,2026:{payload}\n---\nkey: !e!item {payload}\n# {payload}\n"
        ),
        4 => format!("%FOO {payload}\n---\n!<tag:example.com,2026:{payload}> value\n"),
        _ => format!(
            "---\nnode:\n  &anchor !local{payload}\n  # {payload}\n  value\nalias: *anchor\n"
        ),
    };

    parse_with_both_inputs(&yaml);
}

#[cfg(not(test))]
fuzz_target!(|data: &[u8]| check_input(data));
