#[path = "property_check/harness.rs"]
mod harness;
#[path = "property_check/strategy.rs"]
mod strategy;

use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;

use harness::{check_spans_in_bounds, collect_spans, trigger_all_config};
use strategy::arb_document;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 512,
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
            "tests/proptest-regressions/property_check.txt",
        ))),
        ..ProptestConfig::default()
    })]

    #[test]
    fn rule_checks_never_panic_and_report_in_bounds_spans(document in arb_document()) {
        let content = document.render();
        let spans = collect_spans(&content, trigger_all_config());
        if let Err(message) = check_spans_in_bounds(&content, &spans) {
            return Err(TestCaseError::fail(message));
        }
    }
}

const RULE_TRIGGERS: &[(&str, &str)] = &[
    ("anchors", "a: *missing\n"),
    ("braces", "a: { b: 1 }\n"),
    ("brackets", "a: [ 1 ]\n"),
    ("colons", "a :  b\n"),
    ("commas", "a: [1 ,2]\n"),
    ("comments", "a: 1\n#bad\n"),
    ("comments-indentation", "a: 1\n   # over-indented\nb: 2\n"),
    ("document-end", "a: 1\n"),
    ("document-start", "a: 1\n"),
    ("empty-lines", "a: 1\n\n\n\nb: 2\n"),
    ("empty-values", "a:\n"),
    ("float-values", "a: .5\n"),
    ("hyphens", "a:\n  -  x\n"),
    ("indentation", "a:\n   b: 1\n"),
    ("key-duplicates", "a: 1\na: 2\n"),
    ("key-ordering", "b: 1\na: 2\n"),
    ("line-length", "a: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n"),
    ("new-line-at-end-of-file", "a: 1"),
    ("new-lines", "a: 1\r\n"),
    ("octal-values", "a: 010\n"),
    ("quoted-strings", "a: 'plain'\n"),
    ("trailing-spaces", "a: 1   \n"),
    ("truthy", "a: Yes\n"),
];

#[test]
fn each_rule_triggers_and_reports_in_bounds_spans() {
    for (rule, input) in RULE_TRIGGERS {
        let spans = collect_spans(input, trigger_all_config());
        assert!(
            spans.iter().any(|span| span.rule == *rule),
            "expected rule `{rule}` to fire on {input:?}, collected {spans:?}"
        );
        check_spans_in_bounds(input, &spans).unwrap_or_else(|message| {
            panic!("crafted trigger for `{rule}`: {message}")
        });
    }
}

#[test]
fn multibyte_flow_punctuation_spans_stay_in_bounds() {
    let inputs = [
        "{ééé: 1 }\n",
        "ééé :  1\n",
        "[ééé , a]\n",
        "[ 世界 ]\n",
        "k🦀: { å:  1 }\n",
        "a:\r\n  - 世\r\n  -  🦀\r\n",
    ];
    for input in inputs {
        let spans = collect_spans(input, trigger_all_config());
        check_spans_in_bounds(input, &spans).unwrap_or_else(|message| {
            panic!("multibyte regression (issue #232): {message}")
        });
    }
}
