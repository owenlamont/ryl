use std::fmt::Debug;

use ryl::rules::braces::{
    self, Config as BracesConfig, Forbid, Violation as BracesViolation,
};
use ryl::rules::brackets::{
    self, Config as BracketsConfig, Violation as BracketsViolation,
};

fn assert_clean<C, V>(cfg: &C, input: &str, check: fn(&str, &C) -> Vec<V>)
where
    V: Debug,
{
    let diagnostics = check(input, cfg);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics: {diagnostics:?}"
    );
}

fn assert_hits<C, V>(
    cfg: &C,
    input: &str,
    check: fn(&str, &C) -> Vec<V>,
    expected: Vec<V>,
) where
    V: Debug + PartialEq,
{
    let diagnostics = check(input, cfg);
    assert_eq!(diagnostics, expected);
}

#[test]
fn braces_rule_suite() {
    let defaults = BracesConfig::new_for_tests(Forbid::None, 0, 0, -1, -1);
    assert_clean(&defaults, "", braces::check);
    assert_clean(&defaults, "object: {key: 1}\n", braces::check);
    assert_clean(&defaults, "mapping: {\n  key: value\n}\n", braces::check);
    assert_clean(&defaults, "value: \"{ not a mapping }\"\n", braces::check);
    assert_clean(
        &defaults,
        "object: {key: value, # comment\n  other: 2}\n",
        braces::check,
    );
    assert_clean(
        &defaults,
        "object: {key: 1,\r\n  other: 2}\r\n",
        braces::check,
    );
    assert_clean(&defaults, "outer: {{inner: 1}}\n", braces::check);
    assert_clean(&defaults, "}\n", braces::check);
    assert_clean(&defaults, "value: {\n", braces::check);
    assert_clean(&defaults, "value: {# comment\n", braces::check);
    assert_clean(&defaults, "value: {# comment\r\n", braces::check);
    assert_clean(&defaults, "value: {{ missing\n", braces::check);

    assert_hits(
        &defaults,
        "object: { key: 1}\n",
        braces::check,
        vec![BracesViolation {
            line: 1,
            column: 10,
            message: "too many spaces inside braces".to_string(),
        }],
    );
    assert_hits(
        &defaults,
        "object: {key: 1 }\n",
        braces::check,
        vec![BracesViolation {
            line: 1,
            column: 16,
            message: "too many spaces inside braces".to_string(),
        }],
    );

    let forbid_all = BracesConfig::new_for_tests(Forbid::All, 0, 0, -1, -1);
    assert_hits(
        &forbid_all,
        "object: {key: 1}\n",
        braces::check,
        vec![BracesViolation {
            line: 1,
            column: 10,
            message: "forbidden flow mapping".to_string(),
        }],
    );

    let forbid_non_empty = BracesConfig::new_for_tests(Forbid::NonEmpty, 0, 0, -1, -1);
    assert_clean(&forbid_non_empty, "object: {}\n", braces::check);
    assert_hits(
        &forbid_non_empty,
        "object: {key: 1}\n",
        braces::check,
        vec![BracesViolation {
            line: 1,
            column: 10,
            message: "forbidden flow mapping".to_string(),
        }],
    );

    let min_inside = BracesConfig::new_for_tests(Forbid::None, 1, -1, -1, -1);
    assert_hits(
        &min_inside,
        "object: {key: 1}\n",
        braces::check,
        vec![
            BracesViolation {
                line: 1,
                column: 10,
                message: "too few spaces inside braces".to_string(),
            },
            BracesViolation {
                line: 1,
                column: 16,
                message: "too few spaces inside braces".to_string(),
            },
        ],
    );

    let max_inside = BracesConfig::new_for_tests(Forbid::None, 0, 1, -1, -1);
    assert_hits(
        &max_inside,
        "object: {  key: 1   }\n",
        braces::check,
        vec![
            BracesViolation {
                line: 1,
                column: 11,
                message: "too many spaces inside braces".to_string(),
            },
            BracesViolation {
                line: 1,
                column: 20,
                message: "too many spaces inside braces".to_string(),
            },
        ],
    );

    let empty_spacing = BracesConfig::new_for_tests(Forbid::None, 0, 0, 1, 2);
    assert_hits(
        &empty_spacing,
        "object: {}\n",
        braces::check,
        vec![BracesViolation {
            line: 1,
            column: 10,
            message: "too few spaces inside empty braces".to_string(),
        }],
    );
    assert_hits(
        &empty_spacing,
        "object: {    }\n",
        braces::check,
        vec![BracesViolation {
            line: 1,
            column: 13,
            message: "too many spaces inside empty braces".to_string(),
        }],
    );
}

#[test]
fn brackets_rule_suite() {
    let defaults = BracketsConfig::new_for_tests(Forbid::None, 0, 0, -1, -1);
    assert_clean(&defaults, "", brackets::check);
    assert_clean(&defaults, "object: [1, 2]\n", brackets::check);
    assert_clean(&defaults, "seq: [\n  1,\n  2\n]\n", brackets::check);
    assert_clean(
        &defaults,
        "value: \"[ not a sequence ]\"\n",
        brackets::check,
    );
    assert_clean(&defaults, "object: [1, # comment\n  2]\n", brackets::check);
    assert_clean(&defaults, "object: [1,\r\n  2]\r\n", brackets::check);
    assert_clean(&defaults, "outer: [[1]]\n", brackets::check);
    assert_clean(&defaults, "]\n", brackets::check);

    assert_hits(
        &defaults,
        "object: [ 1, 2]\n",
        brackets::check,
        vec![BracketsViolation {
            line: 1,
            column: 10,
            message: "too many spaces inside brackets".to_string(),
        }],
    );
    assert_hits(
        &defaults,
        "object: [1, 2 ]\n",
        brackets::check,
        vec![BracketsViolation {
            line: 1,
            column: 14,
            message: "too many spaces inside brackets".to_string(),
        }],
    );

    let forbid_all = BracketsConfig::new_for_tests(Forbid::All, 0, 0, -1, -1);
    assert_hits(
        &forbid_all,
        "object: [1, 2]\n",
        brackets::check,
        vec![BracketsViolation {
            line: 1,
            column: 10,
            message: "forbidden flow sequence".to_string(),
        }],
    );

    let forbid_non_empty =
        BracketsConfig::new_for_tests(Forbid::NonEmpty, 0, 0, -1, -1);
    assert_clean(&forbid_non_empty, "object: []\n", brackets::check);
    assert_hits(
        &forbid_non_empty,
        "object: [1]\n",
        brackets::check,
        vec![BracketsViolation {
            line: 1,
            column: 10,
            message: "forbidden flow sequence".to_string(),
        }],
    );

    let min_inside = BracketsConfig::new_for_tests(Forbid::None, 1, -1, -1, -1);
    assert_hits(
        &min_inside,
        "object: [1, 2]\n",
        brackets::check,
        vec![
            BracketsViolation {
                line: 1,
                column: 10,
                message: "too few spaces inside brackets".to_string(),
            },
            BracketsViolation {
                line: 1,
                column: 14,
                message: "too few spaces inside brackets".to_string(),
            },
        ],
    );

    let max_inside = BracketsConfig::new_for_tests(Forbid::None, 0, 1, -1, -1);
    assert_hits(
        &max_inside,
        "object: [  1, 2   ]\n",
        brackets::check,
        vec![
            BracketsViolation {
                line: 1,
                column: 11,
                message: "too many spaces inside brackets".to_string(),
            },
            BracketsViolation {
                line: 1,
                column: 18,
                message: "too many spaces inside brackets".to_string(),
            },
        ],
    );

    let empty_spacing = BracketsConfig::new_for_tests(Forbid::None, 0, 0, 1, 2);
    assert_hits(
        &empty_spacing,
        "object: []\n",
        brackets::check,
        vec![BracketsViolation {
            line: 1,
            column: 10,
            message: "too few spaces inside empty brackets".to_string(),
        }],
    );
    assert_hits(
        &empty_spacing,
        "object: [    ]\n",
        brackets::check,
        vec![BracketsViolation {
            line: 1,
            column: 13,
            message: "too many spaces inside empty brackets".to_string(),
        }],
    );
}
