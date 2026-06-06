#[path = "property_check/harness.rs"]
mod harness;
#[path = "property_check/strategy.rs"]
mod strategy;

use std::path::Path;

use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use ryl::lint::lint_str;

use harness::{check_spans_in_bounds, collect_spans, trigger_all_config};
use strategy::arb_document;

fn lint(content: &str) -> Vec<ryl::lint::LintProblem> {
    lint_str(
        content,
        Path::new("in.yaml"),
        trigger_all_config(),
        Path::new("."),
    )
}

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

    /// A leading `# ryl disable` must mute every rule, so only a syntax error
    /// (`rule == None`) can survive.
    #[test]
    fn leading_disable_directive_suppresses_all_rules(document in arb_document()) {
        let content = format!("# ryl disable\n{}", document.render());
        for problem in lint(&content) {
            prop_assert!(
                problem.rule.is_none(),
                "rule {:?} survived a leading `# ryl disable` at {}:{}",
                problem.rule, problem.line, problem.column
            );
        }
    }

    /// Block-disabling a rule that fires on a document removes every diagnostic for
    /// that rule (others may shift by one line but are not asserted here).
    #[test]
    fn block_disabling_a_present_rule_removes_its_diagnostics(document in arb_document()) {
        let content = document.render();
        let Some(rule) = lint(&content).iter().find_map(|problem| problem.rule) else {
            return Ok(());
        };
        let disabled = format!("# ryl disable rule:{rule}\n{content}");
        for problem in lint(&disabled) {
            prop_assert!(
                problem.rule != Some(rule),
                "rule `{rule}` survived its own block disable at {}:{}",
                problem.line, problem.column
            );
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
    ("tags", "a: !!omap []\n"),
    ("trailing-spaces", "a: 1   \n"),
    ("truthy", "a: Yes\n"),
];

/// Guards the hand-maintained `rules::ALL_RULE_IDS` (which a bare `# ryl disable`
/// expands to) against drift: it must list exactly the rules `RULE_TRIGGERS` does, and
/// every `RULE_TRIGGERS` entry is proven to actually fire by the test below. A rule
/// added to one list but not the other &mdash; e.g. a new rule omitted from
/// `ALL_RULE_IDS`, which would silently make bare `# ryl disable` skip it &mdash; fails
/// here.
#[test]
fn rule_triggers_cover_exactly_all_rule_ids() {
    use std::collections::BTreeSet;
    let triggered: BTreeSet<&str> =
        RULE_TRIGGERS.iter().map(|(rule, _)| *rule).collect();
    let all: BTreeSet<&str> = ryl::rules::ALL_RULE_IDS.iter().copied().collect();
    assert_eq!(
        triggered, all,
        "RULE_TRIGGERS and rules::ALL_RULE_IDS must list the same rules"
    );
}

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
fn generated_merge_blocks_reach_canonical_merge_detection() {
    // The structure `arb_merge_block` emits must actually drive key-duplicates'
    // merge-collision path, so the `<<` syntax is not fuzzed vacuously: two
    // anchored bases whose shared key differs collide once merged.
    let merge = "b0: &m0 {dup: 1}\nb1: &m1 {dup: 2}\nh:\n  <<: [*m0, *m1]\n";
    let spans = collect_spans(merge, trigger_all_config());
    assert!(
        spans.iter().any(|span| span.rule == "key-duplicates"),
        "a generated merge block must trigger a key-duplicates collision: {spans:?}"
    );
    check_spans_in_bounds(merge, &spans)
        .unwrap_or_else(|message| panic!("merge block spans out of bounds: {message}"));
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
