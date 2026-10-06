//! The formatter's guarantee as properties: every pass in `passes::format_passes` must be
//! idempotent, parse-preserving, value-preserving at the representation level (tags,
//! aliases, duplicate keys and entry order included, resolved against the declared YAML
//! version), and must keep every comment beside its node and every anchor and alias name.
//! Folding also keeps the two parser-independent properties in `fold`: only lone spaces
//! become line breaks, and every continuation is deeper than its scalar's owner.
//!
//! The generator layers anchors, aliases and tags (`properties`) over the fix-convergence
//! suite's stacked documents. Deterministic tests pin the quote ladder, show the oracle
//! tells apart what it must, and feed deliberately broken passes through the same checks
//! so the suite cannot pass vacuously.

#[path = "property_safe_fix/ast.rs"]
mod ast;
#[path = "property_safe_fix/config.rs"]
#[expect(
    dead_code,
    reason = "shared with the safe-fix suite, which uses every item"
)]
mod config;
#[path = "property_format/fold.rs"]
mod fold;
#[path = "property_format/passes.rs"]
mod passes;
#[path = "property_format/properties.rs"]
mod properties;
#[path = "property_format/representation.rs"]
mod representation;
#[path = "property_fix_convergence/stack.rs"]
mod stack;
#[path = "property_safe_fix/strategy.rs"]
mod strategy;

use std::collections::BTreeSet;

use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use ryl::config::YamlLintConfig;
use ryl::lint::lint_str;
use ryl::rules::{colons, hyphens};

use config::{synthetic_base_dir, synthetic_path};
use fold::{arb_fold_document, inserted_breaks, under_indented_break};
use passes::{
    FOLD_TARGETS, FORMAT_OWNED_RULES, FormatPass, fold_alone, format_passes,
    named_pass, yaml_rules_for,
};
use properties::arb_document_with_properties;
use representation::{annotations, representation};

fn check_invariants(
    format: &dyn Fn(&str) -> String,
    input: &str,
) -> Result<(), String> {
    let once = format(input);
    let before = representation(input);
    let after = representation(&once);
    if before.is_some() != after.is_some() {
        return Err(format!("parse-preservation: output {once:?}"));
    }
    if before != after {
        return Err(format!(
            "value-preservation: output {once:?}; before {before:?}; after {after:?}"
        ));
    }
    let (before, after) = (annotations(input), annotations(&once));
    if before != after {
        return Err(format!(
            "comment/anchor fidelity: output {once:?}; before {before:?}; after {after:?}"
        ));
    }
    let left_alone = |text: &str| {
        colons::unfixed(text, &colons::Config::format()).len()
            + hyphens::unfixed(text, &hyphens::Config::format()).len()
    };
    if left_alone(input) != left_alone(&once) {
        return Err(format!("left-alone fidelity: output {once:?}"));
    }
    let twice = format(&once);
    if twice != once {
        return Err(format!("idempotence: once {once:?}; twice {twice:?}"));
    }
    Ok(())
}

fn check_pass(pass: &FormatPass, input: &str) -> Result<(), String> {
    check_invariants(&pass.format, input)
        .map_err(|violation| format!("pass '{}' on {input:?}: {violation}", pass.name))
}

proptest! {
    #![proptest_config(ProptestConfig {
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
            "tests/proptest-regressions/property_format.txt",
        ))),
        ..ProptestConfig::default()
    })]

    #[test]
    fn every_format_pass_keeps_the_guarantee(document in arb_document_with_properties()) {
        let input = document.render();
        for pass in format_passes() {
            check_pass(pass, &input).map_err(TestCaseError::fail)?;
        }
    }

    #[test]
    fn every_format_pass_keeps_the_guarantee_on_foldable_documents(
        input in arb_fold_document()
    ) {
        for pass in format_passes() {
            check_pass(pass, &input).map_err(TestCaseError::fail)?;
        }
        for (width, indent) in FOLD_TARGETS {
            check_fold(&input, &fold_alone(&input, width, indent))
                .map_err(|violation| TestCaseError::fail(format!(
                    "fold at width {width}, indent {indent} on {input:?}: {violation}"
                )))?;
        }
    }
}

fn check_fold(input: &str, output: &str) -> Result<(), String> {
    let breaks = inserted_breaks(input, output)?;
    match under_indented_break(output, &breaks) {
        Some(at) => Err(format!(
            "continuation at byte {at} of {output:?} is too shallow"
        )),
        None => Ok(()),
    }
}

#[test]
fn fold_checks_pass_real_folds_and_fail_broken_ones() {
    for (input, folded) in [
        ("- key: aaa bbb ccc\n", "- key: aaa\n    bbb\n    ccc\n"),
        ("k:\n- aaa bbb\n", "k:\n- aaa\n  bbb\n"),
        ("aaa bbb\r\n", "aaa\r\n  bbb\r\n"),
    ] {
        assert_eq!(fold_alone(input, 6, 2), folded);
        check_fold(input, folded).unwrap_or_else(|violation| panic!("{violation}"));
        check_invariants(&|s: &str| fold_alone(s, 6, 2), input)
            .unwrap_or_else(|violation| panic!("{violation}"));
    }
    for (input, broken, what) in [
        (
            "k: aaa  bbb\n",
            "k: aaa \n  bbb\n",
            "breaks at a double space",
        ),
        ("k: aaa bbb\n", "k: aaa\n  bbbb\n", "changes a character"),
        (
            "k: aaa bbb\n",
            "k: aaa\n            bbb\n",
            "lengthens a line",
        ),
        (
            "- key: aaa bbb\n",
            "- key: aaa\n  bbb\n",
            "indents to the owner",
        ),
        (
            "aaa bbb\n",
            "aaa\nbbb\n",
            "indents a root scalar to column 0",
        ),
    ] {
        assert!(
            check_fold(input, broken).is_err(),
            "a fold that {what} must fail the fold checks"
        );
    }
    for (input, broken) in [
        ("k: aaa  bbb\n", "k: aaa\n  bbb\n"),
        ("- key: aaa bbb\n", "- key: aaa\n  bbb\n"),
    ] {
        assert!(check_invariants(&|_: &str| broken.to_string(), input).is_err());
    }
}

#[test]
fn quote_ladder_unquotes_only_when_the_plain_scalar_is_the_same_string() {
    let cases = [
        ("k: 'abc'\n", "\nk: abc\n"),
        ("k: \"abc\"\n", "\nk: abc\n"),
        ("k: '011'\n", "\nk: '011'\n"),
        ("k: 'true'\n", "\nk: 'true'\n"),
        ("k: \"a: b\"\n", "\nk: 'a: b'\n"),
        ("%YAML 1.2\n---\nk: 'no'\n", "\nk: no\n"),
        ("%YAML 1.1\n---\nk: 'no'\n", "\nk: 'no'\n"),
        ("%YAML 1.1\n---\nk: '0b101'\n", "\nk: '0b101'\n"),
    ];
    for pass_name in ["fix/best-practice", "format/default"] {
        let pass = named_pass(pass_name);
        for (input, expected) in cases {
            check_pass(pass, input).unwrap_or_else(|violation| panic!("{violation}"));
            let output = (pass.format)(input);
            assert!(
                output.contains(expected),
                "pass '{pass_name}' on {input:?} must emit {expected:?}, got {output:?}"
            );
        }
    }
}

#[test]
fn quote_ladder_escalates_to_double_only_for_escapes() {
    let pass = named_pass("format/default");
    for (input, expected) in [
        ("k: \"line\\n\"\n", "\nk: \"line\\n\"\n"),
        ("k: \"tab\\there\"\n", "\nk: \"tab\\there\"\n"),
    ] {
        check_pass(pass, input).unwrap_or_else(|violation| panic!("{violation}"));
        let output = (pass.format)(input);
        assert!(
            output.contains(expected),
            "{input:?} must emit {expected:?}, got {output:?}"
        );
    }
}

#[test]
fn quote_ladder_applies_to_keys() {
    let pass = named_pass("format/default");
    for (input, expected) in [
        ("'k': 1\n", "\nk: 1\n"),
        ("\"k\": 1\n", "\nk: 1\n"),
        ("\"a: b\": 1\n", "\n'a: b': 1\n"),
        ("%YAML 1.1\n---\n'no': 1\n", "\n'no': 1\n"),
    ] {
        check_pass(pass, input).unwrap_or_else(|violation| panic!("{violation}"));
        let output = (pass.format)(input);
        assert!(
            output.contains(expected),
            "{input:?} must emit {expected:?}, got {output:?}"
        );
    }
}

#[test]
fn the_suite_covers_exactly_the_formatter_rules() {
    assert_eq!(
        BTreeSet::from(FORMAT_OWNED_RULES),
        BTreeSet::from(ryl::format::FORMAT_RULE_IDS)
    );
}

#[test]
fn every_pass_rewrites_a_document_every_format_owned_rule_flags() {
    let input = "# lead\r\n  # misindented\nseq: [ &a0 'x' ,  *a0 ]  #note\n\
                 map: {  k: !!str 'v'  }\nq: 'yes'\n\n\n\nlast: \"plain\"   \nend :  bare\n\
                 list:\n-   item\n\
                 long: alpha bravo charlie delta echo foxtrot golf hotel india juliet kilo \
                 lima mike november";
    assert!(representation(input).is_some(), "dirty input must parse");
    let cfg =
        YamlLintConfig::from_yaml_str(&yaml_rules_for("  quoted-strings: enable\n"))
            .expect("format-owned config parses");
    let flagged: BTreeSet<&str> =
        lint_str(input, synthetic_path(), &cfg, synthetic_base_dir())
            .iter()
            .filter_map(|problem| problem.rule)
            .collect();
    assert_eq!(
        flagged,
        BTreeSet::from(FORMAT_OWNED_RULES),
        "the dirty input must engage every format-owned rule"
    );
    for pass in format_passes() {
        assert_ne!(
            (pass.format)(input),
            input,
            "pass '{}' must have work to do",
            pass.name
        );
        check_pass(pass, input).unwrap_or_else(|violation| panic!("{violation}"));
    }
}

#[test]
fn every_pass_keeps_a_trailing_comment_at_the_end_of_its_document() {
    for input in ["a: 1\n# tail\n", "a: 1\n\n# tail\n", "a: [1]\n# tail"] {
        for pass in format_passes() {
            check_pass(pass, input).unwrap_or_else(|violation| panic!("{violation}"));
        }
    }
}

#[test]
fn representation_tells_apart_what_value_preservation_forbids() {
    for (left, right) in [
        ("a: 1\n", "a: '1'\n"),
        ("a: 1\na: 2\n", "a: 1\n"),
        ("a: 1\nb: 2\n", "b: 2\na: 1\n"),
        ("a: &x 1\nb: *x\n", "a: &x 1\nb: 1\n"),
        ("a: !!str 1\n", "a: '1'\n"),
        ("a: !local x\n", "a: x\n"),
        ("%YAML 1.1\n---\na: 'no'\n", "%YAML 1.1\n---\na: no\n"),
        ("a: 1\n", "a: 1\n---\na: 1\n"),
        ("a: '9223372036854775808'\n", "a: 9223372036854775808\n"),
        (
            "%YAML 1.1\n---\na: !!int 011\n",
            "%YAML 1.1\n---\na: !!int 11\n",
        ),
        ("a: [1]\n", "a: {1: }\n"),
    ] {
        assert_ne!(
            representation(left),
            representation(right),
            "{left:?} and {right:?} must differ"
        );
    }
}

#[test]
fn representation_ignores_layout() {
    for (left, right) in [
        ("a: 'x'\n", "a: x\n"),
        ("a: \"x\"\n", "a: 'x'\n"),
        ("a: 1\n", "---\na: 1\n...\n"),
        ("a: {b: 1}\n", "a:\n  b: 1\n"),
        ("a: True\n", "a: true\n"),
        ("%YAML 1.2\n---\na: 'no'\n", "%YAML 1.2\n---\na: no\n"),
        ("a: &x 1\nb: *x\n", "a: &y 1\nb: *y\n"),
        ("%YAML 1.1\n---\na: 1e3\n", "%YAML 1.1\n---\na: '1e3'\n"),
    ] {
        assert_eq!(
            representation(left),
            representation(right),
            "{left:?} and {right:?} must be equal"
        );
    }
}

#[test]
fn a_deliberately_broken_pass_fails_the_suite() {
    let input = "%YAML 1.1\n---\n# lead\na: &x 'no'  # note\nb: *x\nc: 1\n";
    type Broken = fn(&str) -> String;
    let broken: [(&str, Broken, &str); 7] = [
        (
            "idempotence",
            |s| format!("{s}\n"),
            "appends a blank line each run",
        ),
        (
            "parse-preservation",
            |s| format!("{s}d: [\n"),
            "breaks the parse",
        ),
        (
            "value-preservation",
            |s| s.replace("'no'", "no"),
            "unquotes under 1.1",
        ),
        (
            "value-preservation",
            |s| s.replace("c: 1\n", ""),
            "drops an entry",
        ),
        (
            "comment/anchor fidelity",
            |s| s.replace("  # note", ""),
            "drops a comment",
        ),
        (
            "comment/anchor fidelity",
            |s| {
                s.replace(
                    "# lead\na: &x 'no'  # note\n",
                    "a: &x 'no'  # note\n# lead\n",
                )
            },
            "moves a comment to another node",
        ),
        (
            "comment/anchor fidelity",
            |s| s.replace("&x", "&y").replace("*x", "*y"),
            "renames an anchor",
        ),
    ];
    let broken_spacing: [(&str, (&str, Broken, &str)); 2] = [
        (
            "-   a:\n    b: 1\n",
            (
                "value-preservation",
                |s| s.replace("-   ", "- "),
                "collapses a dash's spaces",
            ),
        ),
        (
            "-   a: 1\n    b: 2\n",
            (
                "left-alone fidelity",
                |s| s.replace("-   a: 1\n    b: 2", "- a: 1\n  b: 2"),
                "re-indents a compact mapping",
            ),
        ),
    ];
    let cases = broken
        .into_iter()
        .map(|case| (input, case))
        .chain(broken_spacing);
    for (input, (invariant, pass, what)) in cases {
        check_invariants(&|s: &str| s.to_string(), input)
            .expect("identity pass keeps the guarantee");
        let violation = check_invariants(&pass, input)
            .expect_err(&format!("a pass that {what} must fail the suite"));
        assert!(
            violation.starts_with(invariant),
            "a pass that {what} must break {invariant}, got {violation}"
        );
    }
}
