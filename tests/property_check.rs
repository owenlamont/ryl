#[path = "property_check/harness.rs"]
mod harness;
#[path = "property_check/strategy.rs"]
mod strategy;

use std::path::Path;

use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use ryl::lint::lint_str;

use harness::{
    check_spans_in_bounds, collect_spans, comments_indentation_open_config,
    hyphens_dash_on_own_line_config, trigger_all_config,
};
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

    /// `comments-indentation: allow-any-open-indent` is purely additive acceptance:
    /// enabling it can only *remove* violations the default reports (a comment now
    /// matches an open block level) and never adds one or moves a span. Exercises the
    /// open-block-indent stack over generated nested comments and pins that contract.
    #[test]
    fn allow_any_open_indent_only_relaxes_comments_indentation(
        document in arb_document(),
    ) {
        use ryl::rules::comments_indentation::{Config, check};
        let content = document.render();
        let strict = check(&content, &Config::new_for_tests(false));
        let relaxed = check(&content, &Config::new_for_tests(true));
        for violation in &relaxed {
            prop_assert!(
                strict.contains(violation),
                "allow-any-open-indent reported {violation:?} the default did not, \
                 for {content:?}"
            );
        }
    }

    /// `hyphens: dash-on-own-line` is purely additive: enabling it preserves every
    /// `max-spaces-after` violation the default reports and only *adds* dash-on-own-line
    /// violations. Exercises the scanner-driven detection over generated sequences of
    /// mappings and pins that contract (the inverse of the relaxing option above).
    #[test]
    fn dash_on_own_line_only_adds_hyphens_violations(document in arb_document()) {
        use ryl::rules::hyphens::{Config, check};
        let content = document.render();
        let base = check(&content, &Config::new_for_tests(1));
        let enhanced =
            check(&content, &Config::new_for_tests(1).with_dash_on_own_line(true));
        for violation in &base {
            prop_assert!(
                enhanced.contains(violation),
                "dash-on-own-line dropped base violation {violation:?} for {content:?}"
            );
        }
    }
}

const RULE_TRIGGERS: &[(&str, &str)] = &[
    ("anchors", "a: *missing\n"),
    // Colon welded to a used, non-duplicate anchor/alias: only the ryl-only
    // `forbid-ambiguous-anchor-alias-names` dispatch can flag it, so this proves
    // that dispatch fires (not vacuously the undeclared/duplicated/unused checks).
    ("anchors", "a: &foo: 1\nb: *foo:\n"),
    ("block-scalar-chomping", "a: |\n  hi\n"),
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
    ("merge-keys", "a: &a {x: 1}\nb:\n  <<: *a\n"),
    ("new-line-at-end-of-file", "a: 1"),
    ("new-lines", "a: 1\r\n"),
    ("octal-values", "a: 010\n"),
    ("quoted-strings", "a: 'plain'\n"),
    ("tags", "a: !!omap []\n"),
    ("trailing-spaces", "a: 1   \n"),
    ("truthy", "a: Yes\n"),
    ("unicode-line-breaks", "a: \"x\u{2028}y\"\n"),
];

/// Guards the hand-maintained `rules::ALL_RULE_IDS` (which a bare `# ryl disable`
/// expands to) against drift: it must list exactly the rules `RULE_TRIGGERS` does, and
/// every `RULE_TRIGGERS` entry is proven to actually fire by the test below. A rule
/// added to one list but not the other (e.g. a new rule omitted from
/// `ALL_RULE_IDS`, which would silently make bare `# ryl disable` skip it) fails
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
fn open_indent_config_relaxes_a_generated_shape() {
    // Mirrors the merge guard below: a comment at an open block level the default
    // flags (a shape `arb_document` emits from entries/seq-items/comments at varied
    // indents) must be *accepted* by the harness's open-indent config but flagged by
    // the default, so the second `collect_spans` dispatch (and the monotonicity
    // property) is not exercised vacuously.
    use ryl::rules::comments_indentation::{Config, check};
    let content = "items:\n  - one\n# boundary\n  - two\n";
    let strict = check(content, &Config::resolve(trigger_all_config()));
    let relaxed = check(
        content,
        &Config::resolve(comments_indentation_open_config()),
    );
    assert!(
        !strict.is_empty() && relaxed.is_empty(),
        "open-indent config must accept an open-level comment the default flags: \
         strict={strict:?} relaxed={relaxed:?}"
    );
}

#[test]
fn dash_on_own_line_config_flags_a_generated_shape() {
    // Mirrors the open-indent guard: a sequence-of-mappings shape `arb_document` emits
    // (a `- key: val` entry) must be *flagged* by the harness's dash-on-own-line config
    // but ignored by the default, so the second `collect_spans` dispatch (and the
    // monotonicity property) is not exercised vacuously.
    use ryl::rules::hyphens::{Config, check};
    let content = "items:\n  - name: web\n    port: 80\n";
    let base = check(content, &Config::resolve(trigger_all_config()));
    let enhanced = check(content, &Config::resolve(hyphens_dash_on_own_line_config()));
    assert!(
        base.is_empty() && !enhanced.is_empty(),
        "dash-on-own-line config must flag a sequence-of-mappings the default ignores: \
         base={base:?} enhanced={enhanced:?}"
    );
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
        check_spans_in_bounds(input, &spans)
            .unwrap_or_else(|message| panic!("multibyte regression: {message}"));
    }
}

/// A document delimited by bare `\r` (and mixed `\r`/`\r\n`/`\n`, with multibyte
/// content) must keep CR-aware spans in bounds against the CR-aware oracle.
#[test]
fn bare_cr_line_breaks_keep_spans_in_bounds() {
    let inputs = [
        "a: 1  \rb: 2\r",            // trailing-spaces + EOF break is a bare CR
        "café:  \rå: 1\r",           // multibyte before trailing spaces on a CR line
        "aa\raa bb\r",               // line-length: a bare CR splits the line
        "a: 1\r\r\r\rb: 2\r",        // empty-lines: blank run via bare CR
        "a: 1\rb: \"x\u{2028}y\"\r", // unicode-line-breaks on the second CR line
        "b: 1\r   # over\rc: 2\r",   // comments-indentation on bare-CR lines
        "a: 1\r\nb: 2\rc: 3\n",      // mixed CRLF / bare CR / LF in one document
    ];
    for input in inputs {
        let spans = collect_spans(input, trigger_all_config());
        check_spans_in_bounds(input, &spans)
            .unwrap_or_else(|message| panic!("bare-CR regression: {message}"));
    }
}
