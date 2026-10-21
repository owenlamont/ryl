#![no_main]

mod common;

use common::parse_with_both_inputs;
use libfuzzer_sys::fuzz_target;

// Keep one unwrapped target so arbitrary top-level syntax, document streams, and malformed
// transitions are not hidden by the grammar-specific generators.
fuzz_target!(|input: &str| {
    if input.len() <= 64 * 1024 {
        parse_with_both_inputs(input);
    }
});
