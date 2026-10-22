//! Property suites over the parser's semantic oracles.
//!
//! Every input backend must agree on the full event and token traces, including the prefix
//! emitted before an error, and disabling comments must remove only comment events. Each
//! construction module decodes raw bytes: the leading control bytes select a grammar shape and
//! the rest becomes its payload, so arbitrary bytes are hostile input and short seeds reach
//! every construction.

#[path = "property_parser/aliases_merges.rs"]
mod aliases_merges;
#[path = "property_parser/common.rs"]
mod common;
#[path = "property_parser/directives_tags.rs"]
mod directives_tags;
#[path = "property_parser/duplicate_keys.rs"]
mod duplicate_keys;
#[path = "property_parser/fallible_input.rs"]
mod fallible_input;
#[path = "property_parser/flow_collections.rs"]
mod flow_collections;
#[path = "property_parser/large_scalars.rs"]
mod large_scalars;
#[path = "property_parser/options.rs"]
mod options;
#[path = "property_parser/parser_stack.rs"]
mod parser_stack;
#[path = "property_parser/seeds.rs"]
mod seeds;
mod support;

use proptest::collection::vec;
use proptest::prelude::*;
use proptest::sample::select;
use proptest::test_runner::FileFailurePersistence;
use ryl_yaml_parser::ErrorKind;

fn raw_bytes(max_len: usize) -> impl Strategy<Value = Vec<u8>> {
    vec(any::<u8>(), 0..max_len)
}

/// Control bytes drawn from `header`, followed by a raw payload.
fn seeded(
    header: impl Strategy<Value = Vec<u8>>,
    max_payload: usize,
) -> impl Strategy<Value = Vec<u8>> {
    (header, raw_bytes(max_payload)).prop_map(|(mut header, payload)| {
        header.extend(payload);
        header
    })
}

fn check_raw_text(input: &str) {
    common::parse_with_both_inputs(input);
    common::scan_with_all_inputs(input);
    common::check_comment_suppression(input);
}

#[test]
fn raw_regressions_keep_backends_in_agreement() {
    for input in seeds::RAW_INPUTS {
        check_raw_text(input);
    }
}

#[test]
fn named_seeds_reach_their_oracles() {
    for target in seeds::TARGETS {
        for input in seeds::seeds(target) {
            match *target {
                "aliases_merges" => aliases_merges::check_input(&input),
                "directives_tags" => directives_tags::check_input(&input),
                "duplicate_keys" => duplicate_keys::check_input(&input),
                "fallible_input" => fallible_input::check_input(&input),
                "flow_collections" => flow_collections::check_input(&input),
                "large_scalars" => large_scalars::check_input(&input),
                "options" => options::check_input(&input),
                "parser_stack" => parser_stack::check_input(&input),
                "parse_any" => {
                    common::parse_with_both_inputs(
                        std::str::from_utf8(&input).unwrap(),
                    );
                }
                "scan_any" => {
                    common::scan_with_all_inputs(std::str::from_utf8(&input).unwrap());
                }
                _ => panic!("unhandled oracle: {target}"),
            }
        }
    }
}

#[test]
fn stack_comment_run_before_eof_places_only_the_last_comment_last() {
    // Only the final EOF comment is Last; a preceding comment in the same run is Above.
    parser_stack::check_input(&[0, 0, 2, 0, 0]);
}

#[test]
fn lookahead_before_a_carriage_return_run_does_not_overrun() {
    check_raw_text("\u{1}'null\"\r\r\r\r\r\n\r\r\r\r");
}

#[test]
fn unbalanced_implicit_flow_mappings_do_not_unwrap_a_missing_context() {
    let raw: &[u8] = &[
        91, 91, 32, 101, 58, 9, 123, 63, 32, 45, 106, 101, 58, 9, 123, 63, 32, 44, 117,
        101, 58, 9, 123, 63, 32, 44, 9, 26, 58, 32, 126, 93, 8, 58, 32, 58, 10, 29, 58,
        58, 58, 32, 58, 29, 63, 32, 44, 9, 26, 58, 32, 126, 93, 8, 58, 32, 58, 10, 78,
        32,
    ];
    check_raw_text(std::str::from_utf8(raw).unwrap());
}

#[test]
fn block_scalar_indices_count_characters_on_every_backend() {
    check_raw_text("|\r \u{4b4}A");
}

#[test]
fn indentless_sequence_probe_reports_the_same_prefix_on_every_backend() {
    let error = ryl_yaml_parser::Parser::new_from_str("a:\n-\nb")
        .find_map(Result::err)
        .expect("an empty indentless entry followed by a plain scalar must fail");
    assert_eq!(error.kind(), &ErrorKind::SimpleKeyExpected);
    check_raw_text("a:\n-\nb");
}

#[test]
fn invalid_bytes_inside_flow_collections_do_not_panic() {
    let raw = String::from_utf8_lossy(&[
        255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 10, 9, 35, 8, 10, 9,
        255, 255, 255, 255, 255, 10, 9, 35, 8, 35, 91, 93, 58,
    ])
    .into_owned();
    for input in [
        format!("[{raw}]"),
        format!("{{{raw}}}"),
        format!("root: {{{raw}}}\narray: [{raw}]\n"),
    ] {
        check_raw_text(&input);
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
            "tests/proptest-regressions/property_parser.txt",
        ))),
        ..ProptestConfig::default()
    })]

    #[test]
    fn hostile_bytes_keep_backends_in_agreement(data in raw_bytes(1024)) {
        check_raw_text(&String::from_utf8_lossy(&data));
    }

    #[test]
    fn yaml_like_text_keeps_backends_in_agreement(
        input in "([-?:,#&*!|>'\"%@` \t\r\n\\[\\]{}a-c.é]|---|\\.\\.\\.){0,96}"
    ) {
        check_raw_text(&input);
    }

    #[test]
    fn anchors_aliases_and_merges_keep_their_relationships(
        data in seeded(vec(0u8..4, 1), 256)
    ) {
        aliases_merges::check_input(&data);
    }

    #[test]
    fn duplicate_keys_are_preserved(data in seeded(vec(0u8..5, 1), 256)) {
        duplicate_keys::check_input(&data);
    }

    #[test]
    fn directives_and_tags_resolve(
        data in seeded(vec(prop_oneof![0u8..6, 240u8..=255], 1), 256)
    ) {
        directives_tags::check_input(&data);
    }

    #[test]
    fn flow_collections_match_their_construction(
        data in seeded((prop_oneof![0u8..6, 252u8..=254], any::<u8>()).prop_map(|(mode, shape)| vec![mode, shape]), 256)
    ) {
        flow_collections::check_input(&data);
    }

    #[test]
    fn resource_options_reject_exactly_past_their_limits(
        data in seeded(vec(any::<u8>(), 5), 256)
    ) {
        options::check_input(&data);
    }

    #[test]
    fn parser_stack_backends_agree_on_events_and_errors(
        data in seeded((0u8..7, 0u8..5, select(vec![0u8, 2, 4]), 0u8..3, 0u8..2).prop_map(|(backend, tail, comments, newline, nested)| vec![backend, tail, comments, newline, nested]), 64)
    ) {
        parser_stack::check_input(&data);
    }

    #[test]
    fn injected_source_failures_stop_scanning(data in seeded(vec(any::<u8>(), 3), 256)) {
        fallible_input::check_input(&data);
    }
}

proptest! {
    // Each case builds scalars of up to 256 KiB.
    #![proptest_config(ProptestConfig {
        cases: 32,
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
            "tests/proptest-regressions/property_parser.txt",
        ))),
        ..ProptestConfig::default()
    })]

    #[test]
    fn large_scalars_keep_values_and_spans(
        data in seeded(vec(any::<u8>(), 3), 256)
    ) {
        large_scalars::check_input(&data);
    }
}
