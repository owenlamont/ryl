//! Deterministic checks of the exact semantic oracles used by libFuzzer.
//! Keep these independent of libFuzzer so ordinary CI checks every generated construction.

#[path = "../fuzz/fuzz_targets/aliases_merges.rs"]
mod aliases_merges;
#[path = "../fuzz/fuzz_targets/common.rs"]
mod common;
#[path = "../fuzz/fuzz_targets/directives_tags.rs"]
mod directives_tags;
#[path = "../fuzz/fuzz_targets/duplicate_keys.rs"]
mod duplicate_keys;
#[path = "../fuzz/fuzz_targets/fallible_input.rs"]
mod fallible_input;
#[path = "../fuzz/fuzz_targets/flow_collections.rs"]
mod flow_collections;
#[path = "../fuzz/fuzz_targets/large_scalars.rs"]
mod large_scalars;
#[path = "../fuzz/fuzz_targets/options.rs"]
mod options;
#[path = "../fuzz/fuzz_targets/parser_stack.rs"]
mod parser_stack;
#[path = "../fuzz/seed_corpus.rs"]
mod seed_corpus;

#[test]
fn raw_fuzz_oracles_cover_changelog_fixes_and_malformed_prefixes() {
    for input in seed_corpus::RAW_INPUTS {
        common::parse_with_both_inputs(input);
        common::scan_with_all_inputs(input);
        common::check_comment_suppression(input);
    }
}

#[test]
fn generated_fuzz_seeds_reach_their_oracles() {
    for target in seed_corpus::TARGETS {
        for input in seed_corpus::seeds(target) {
            match *target {
                "aliases_merges" => aliases_merges::check_input(&input),
                "directives_tags" => directives_tags::check_input(&input),
                "duplicate_keys" => duplicate_keys::check_input(&input),
                "fallible_input" => fallible_input::check_input(&input),
                "flow_collections" => flow_collections::check_input(&input),
                "large_scalars" => large_scalars::check_input(&input),
                "options" => options::check_input(&input),
                "parser_stack" => parser_stack::check_input(&input),
                "parse_any" => common::parse_with_both_inputs(std::str::from_utf8(&input).unwrap()),
                "scan_any" => common::scan_with_all_inputs(std::str::from_utf8(&input).unwrap()),
                _ => panic!("unhandled fuzz target: {target}"),
            }
        }
    }
}

#[test]
fn directive_and_option_fuzz_oracles_cover_boundaries() {
    for mode in [0, 1, 2, 252, 253, 254] {
        for flags in 0..4 {
            for selector in 0..13 {
                let mut input = vec![mode, flags, selector, selector, selector];
                input.extend_from_slice(b"%FUTURE x # comment\n---\nkey: [value]\n");
                options::check_input(&input);
            }
        }
    }
    for selector in (0..6).chain(240..=255) {
        for payload in ["", "é🪨\t# ---\n", "a\0b"] {
            let mut input = vec![selector];
            input.extend_from_slice(payload.as_bytes());
            directives_tags::check_input(&input);
        }
    }
}

#[test]
fn flow_fuzz_oracles_cover_indicators_and_tab_separation() {
    for mode in (0..6).chain(252..=254) {
        for shape in [0, 1, 2, 3, 15, 31, 63, 255] {
            for payload in [b"".as_slice(), b"a|b >c #\n", "é🪨".as_bytes(), &[0, 255]] {
                let mut input = vec![mode, shape];
                input.extend_from_slice(payload);
                flow_collections::check_input(&input);
            }
        }
    }
}

#[test]
fn scalar_fuzz_oracles_cover_folding_chomping_and_document_boundaries() {
    for mode in 0..8 {
        large_scalars::check_input(&[mode, 0, b'a', b'b']);
    }
    for variant in 0..=255 {
        // Cover every variant at the smallest size while varying width and marker separation.
        let mut input = vec![254, 5 * (variant % 12), variant];
        input.extend_from_slice("é🪨".as_bytes());
        large_scalars::check_input(&input);
    }
    for bucket in 1..5 {
        large_scalars::check_input(&[254, bucket, 255, b'x']);
    }
}

#[test]
fn stack_fuzz_oracles_cover_backends_comments_and_document_errors() {
    // Keep the case that caught the oracle's original mistake: only the final EOF comment
    // is Last; a preceding comment in the same run is Above (see Placement's contract).
    parser_stack::check_input(&[0, 0, 2, 0, 0]);
    for backend in 0..7 {
        for tail in 0..5 {
            for comments in [0, 4] {
                for newline in 0..3 {
                    for nested in 0..2 {
                        let mut input = vec![backend, tail, comments, newline, nested];
                        input.extend_from_slice(b"trailing # ---\t\x80\xff");
                        parser_stack::check_input(&input);
                    }
                }
            }
        }
    }
}
