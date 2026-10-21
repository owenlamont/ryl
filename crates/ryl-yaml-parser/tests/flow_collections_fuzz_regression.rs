use std::panic::{catch_unwind, AssertUnwindSafe};

use granit_parser::Parser;

const CRASH_CA24C2F5B1341124FCD324CBAAFCAA9A1F6D034C: &[u8] = &[
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 10, 9, 35, 8, 10, 9, 255, 255, 255, 255,
    255, 10, 9, 35, 8, 35, 91, 93, 58,
];

fn drain_str_parser(input: &str) {
    for event in Parser::new_from_str(input) {
        if event.is_err() {
            break;
        }
    }
}

fn drain_iter_parser(input: &str) {
    for event in Parser::new_from_iter(input.chars()) {
        if event.is_err() {
            break;
        }
    }
}

#[test]
fn crash_ca24c2f5b1341124fcd324cbaafcaa9a1f6d034c_does_not_panic() {
    let s = String::from_utf8_lossy(CRASH_CA24C2F5B1341124FCD324CBAAFCAA9A1F6D034C);
    let inputs = [
        format!("[{s}]"),
        format!("{{{s}}}"),
        format!("root: {{{s}}}\narray: [{s}]\n"),
    ];

    for input in inputs {
        let result = catch_unwind(AssertUnwindSafe(|| drain_str_parser(&input)));
        assert!(result.is_ok(), "str parser panicked for input: {input:?}");

        let result = catch_unwind(AssertUnwindSafe(|| drain_iter_parser(&input)));
        assert!(result.is_ok(), "iter parser panicked for input: {input:?}");
    }
}
