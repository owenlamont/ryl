#![cfg_attr(not(test), no_main)]

#[cfg(not(test))]
mod common;
#[cfg(test)]
use crate::common;

use std::str;

use common::{check_comment_suppression_with_options, parse_with_options, scan_with_options};
use granit_parser::{ErrorKind, Options, Parser, Scanner, StrInput};
#[cfg(not(test))]
use libfuzzer_sys::fuzz_target;

#[cfg(feature = "comments")]
const COMMENT_LIMITS: &[usize] = &[0, 1, 31, 32, 33, 95, 96, 97, 255];
const KEY_LIMITS: &[usize] = &[0, 1, 15, 16, 17, 127, 128, 129, 1023, 1024, 1025, 4096];
const FLOW_LIMITS: &[usize] = &[0, 1, 2, 254, 255, 256, 512];
// A zero selector keeps the previously implicit default for each newly fuzzed option.
const BLOCK_LIMITS: &[usize] = &[255, 0, 1, 2, 31, 32, 33, 254, 256];
const DIRECTIVE_LIMITS: &[usize] = &[1024, 0, 1, 2, 7, 8, 15, 16, 17, 1023, 1025, 4096];
const PARAM_LIMITS: &[usize] = &[16, 0, 1, 2, 15, 17, 31, 32];

fn assert_outcome(input: &str, options: Options, expected_error: Option<&ErrorKind>) {
    let parser_error = Parser::new_from_str_with_options(input, options.clone())
        .find_map(Result::err)
        .map(|error| error.kind().clone());
    assert_eq!(
        parser_error.as_ref(),
        expected_error,
        "generated input: {input:?}"
    );
    let scanner_error = Scanner::with_options(StrInput::new(input), options.clone())
        .find_map(Result::err)
        .map(|error| error.kind().clone());
    assert_eq!(
        scanner_error.as_ref(),
        expected_error,
        "generated input: {input:?}"
    );
    parse_with_options(input, options.clone());
    scan_with_options(input, options);
}

fn check_generated_boundary(mode: u8, flags: u8, selector: u8) {
    let mut options = Options::default();
    #[cfg(feature = "comments")]
    {
        options.emit_comments = flags & 1 != 0;
    }
    match mode {
        252 => {
            let length = DIRECTIVE_LIMITS[usize::from(selector) % DIRECTIVE_LIMITS.len()];
            let parameter = if flags & 2 == 0 { "a" } else { "é" }.repeat(length);
            let directive = if parameter.is_empty() {
                "X".to_string()
            } else {
                format!("X {parameter}")
            };
            let yaml = format!("%{directive}\n---\nvalue\n");
            options.max_directive_bytes = directive.len();
            assert_outcome(&yaml, options.clone(), None);
            options.max_directive_bytes -= 1;
            let limit = options.max_directive_bytes;
            assert_outcome(
                &yaml,
                options,
                Some(&ErrorKind::DirectiveByteLimitExceeded { limit }),
            );
        }
        253 => {
            let count = PARAM_LIMITS[usize::from(selector) % PARAM_LIMITS.len()];
            options.max_reserved_directive_params = count;
            let parameters = " a".repeat(count);
            assert_outcome(
                &format!("%X{parameters}\n---\nvalue\n"),
                options.clone(),
                None,
            );
            assert_outcome(
                &format!("%X{parameters} a\n---\nvalue\n"),
                options,
                Some(&ErrorKind::TooManyReservedDirectiveParams { limit: count }),
            );
        }
        254 => {
            let depth = BLOCK_LIMITS[usize::from(selector) % BLOCK_LIMITS.len()];
            options.block_nesting_limit = depth;
            let collections = "- ".repeat(depth);
            assert_outcome(&format!("{collections}value\n"), options.clone(), None);
            assert_outcome(
                &format!("{collections}- value\n"),
                options,
                Some(&ErrorKind::RecursionLimitExceeded),
            );
        }
        _ => unreachable!(),
    }
}

// Exercise option boundaries independently of the grammar-specific targets. Bytes before the
// UTF-8 payload choose the oracle and values around meaningful resource-limit boundaries.
pub fn check_input(data: &[u8]) {
    let [mode, flags, comment, key, flow, input @ ..] = data else {
        return;
    };
    #[cfg(not(feature = "comments"))]
    let _ = comment;
    if input.len() > 64 * 1024 {
        return;
    }
    // Keep the original five-byte header and raw mode dispatch for existing corpus entries.
    // Only these three selectors construct a small exact-limit/over-limit pair per iteration.
    if matches!(mode, 252..=254) {
        check_generated_boundary(*mode, *flags, *key);
        return;
    }
    let Ok(input) = str::from_utf8(input) else {
        return;
    };

    // The high flag bit selects strict indentation for every raw oracle, including comment
    // suppression. Lower flags and the five-byte header keep their existing meanings.
    let mut options = granit_parser::options! { strict_indentation: flags & 0x80 != 0 };
    if mode % 3 == 2 {
        check_comment_suppression_with_options(input, options);
        return;
    }

    #[cfg(feature = "comments")]
    {
        options.emit_comments = flags & 1 != 0;
    }
    #[cfg(feature = "comments")]
    {
        options.max_buffered_comment_events =
            COMMENT_LIMITS[*comment as usize % COMMENT_LIMITS.len()];
    }
    options.simple_key_max_lookahead = KEY_LIMITS[*key as usize % KEY_LIMITS.len()];
    options.flow_nesting_limit = FLOW_LIMITS[*flow as usize % FLOW_LIMITS.len()];
    options.block_nesting_limit = BLOCK_LIMITS[usize::from(*flow >> 3) % BLOCK_LIMITS.len()];
    options.max_directive_bytes = DIRECTIVE_LIMITS[usize::from(*key >> 3) % DIRECTIVE_LIMITS.len()];
    options.max_reserved_directive_params =
        PARAM_LIMITS[usize::from(*flags >> 1) % PARAM_LIMITS.len()];

    if mode % 3 == 0 {
        parse_with_options(input, options);
    } else {
        scan_with_options(input, options);
    }
}

#[cfg(not(test))]
fuzz_target!(|data: &[u8]| check_input(data));
