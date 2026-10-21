#![no_main]

mod common;

use common::scan_with_all_inputs;
use libfuzzer_sys::fuzz_target;

// Scanner errors can occur beyond the point at which the parser stops, so scan raw input
// independently and compare the optimized, buffered, and fallible-buffered input paths.
fuzz_target!(|input: &str| {
    if input.len() <= 64 * 1024 {
        scan_with_all_inputs(input);
    }
});
