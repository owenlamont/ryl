//! Property tests that the safe-fix pipeline converges: no fixer needs the
//! `RULE_FIX_MAX_ITERATIONS` cap to stop, the pipeline reaches a fixed point within
//! `FIX_PIPELINE_MAX_PASSES` without revisiting an earlier state (a cycle between
//! fixers), and so one `apply_safe_fixes` call leaves nothing for a second to change.
//!
//! The pipeline is probed by calling each rule's public `fix` in pipeline order,
//! iterating each rule and then the whole pass to a fixed point, so a fixer or pass the
//! caps would silently truncate fails here. The probe must reproduce `apply_safe_fixes`
//! byte-for-byte, which pins its rule table to the production order. The generator
//! (`stack`) stacks file-shape issues around the safe-fix suite's entries so fixers
//! genuinely interact; a deterministic sibling pins a stacked input that several fixers
//! rewrite, so the property cannot pass vacuously.

#[path = "property_safe_fix/ast.rs"]
#[allow(
    dead_code,
    reason = "shared with the safe-fix suite, which uses every item"
)]
mod ast;
#[path = "property_safe_fix/config.rs"]
#[allow(
    dead_code,
    reason = "shared with the safe-fix suite, which uses every item"
)]
mod config;
#[path = "property_fix_convergence/stack.rs"]
mod stack;
#[path = "property_safe_fix/strategy.rs"]
mod strategy;

use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use ryl::config::YamlLintConfig;
use ryl::fix::{FIX_PIPELINE_MAX_PASSES, RULE_FIX_MAX_ITERATIONS, apply_safe_fixes};
use ryl::rules::{
    braces, brackets, commas, comments, comments_indentation, document_end,
    document_start, empty_lines, new_line_at_end_of_file, new_lines, quoted_strings,
    trailing_spaces,
};

use config::{
    SAFE_FIX_RULES, named_config, parse_for_compare, safe_fix_configs,
    synthetic_base_dir, synthetic_path,
};
use stack::{Decoration, Filler, StackedDocument, arb_stacked_document};

type RuleFix = (&'static str, Box<dyn Fn(&str) -> Option<String>>);

/// The rules one `FixContext::pass` runs, in its order. Every matrix config
/// enables `new-lines`, so the final newline is always the configured one.
fn pipeline_rules(cfg: &YamlLintConfig) -> Vec<RuleFix> {
    let newline = new_lines::expected_newline(
        new_lines::Config::resolve(cfg),
        new_lines::platform_newline(),
    )
    .into_owned();
    let new_lines_cfg = new_lines::Config::resolve(cfg);
    let comments_cfg = comments::Config::resolve(cfg);
    let comments_indentation_cfg = comments_indentation::Config::resolve(cfg);
    let commas_cfg = commas::Config::resolve(cfg);
    let braces_cfg = braces::Config::resolve(cfg);
    let brackets_cfg = brackets::Config::resolve(cfg);
    let quoted_strings_cfg = quoted_strings::Config::resolve(cfg);
    let document_start_cfg = document_start::Config::resolve(cfg);
    let document_end_cfg = document_end::Config::resolve(cfg);
    let empty_lines_cfg = empty_lines::Config::resolve(cfg);
    vec![
        (
            new_lines::ID,
            Box::new(move |buffer| {
                new_lines::fix(buffer, new_lines_cfg, new_lines::platform_newline())
            }),
        ),
        (
            comments::ID,
            Box::new(move |buffer| comments::fix(buffer, &comments_cfg)),
        ),
        (
            comments_indentation::ID,
            Box::new(move |buffer| {
                comments_indentation::fix(buffer, &comments_indentation_cfg)
            }),
        ),
        (
            commas::ID,
            Box::new(move |buffer| commas::fix(buffer, &commas_cfg)),
        ),
        (
            braces::ID,
            Box::new(move |buffer| braces::fix(buffer, &braces_cfg)),
        ),
        (
            brackets::ID,
            Box::new(move |buffer| brackets::fix(buffer, &brackets_cfg)),
        ),
        (
            new_line_at_end_of_file::ID,
            Box::new(move |buffer| new_line_at_end_of_file::fix(buffer, &newline)),
        ),
        (
            quoted_strings::ID,
            Box::new(move |buffer| quoted_strings::fix(buffer, &quoted_strings_cfg)),
        ),
        (trailing_spaces::ID, Box::new(trailing_spaces::fix)),
        (
            document_start::ID,
            Box::new(move |buffer| document_start::fix(buffer, &document_start_cfg)),
        ),
        (
            document_end::ID,
            Box::new(move |buffer| document_end::fix(buffer, &document_end_cfg)),
        ),
        (
            empty_lines::ID,
            Box::new(move |buffer| empty_lines::fix(buffer, &empty_lines_cfg)),
        ),
    ]
}

fn fix_rule_to_fixed_point(
    rule: &str,
    fix: &dyn Fn(&str) -> Option<String>,
    input: &str,
    cfg_name: &str,
) -> Result<String, TestCaseError> {
    let mut current = input.to_string();
    for _ in 0..RULE_FIX_MAX_ITERATIONS {
        match fix(&current) {
            Some(next) if next != current => current = next,
            _ => return Ok(current),
        }
    }
    Err(TestCaseError::fail(format!(
        "'{rule}' fix still changing after {RULE_FIX_MAX_ITERATIONS} iterations under config '{cfg_name}'; input {input:?}; last {current:?}"
    )))
}

/// One `apply_safe_fixes` pass rebuilt from the rules' own `fix` functions. Mirrors the
/// production gate that leaves unparsable input untouched; generated input carries no
/// aliases, so `parse_for_compare` agrees with that gate's stricter parse.
fn probe_pass(
    state: &str,
    rules: &[RuleFix],
    cfg_name: &str,
) -> Result<String, TestCaseError> {
    if parse_for_compare(state).is_none() {
        return Ok(state.to_string());
    }
    rules
        .iter()
        .try_fold(state.to_string(), |content, (rule, fix)| {
            fix_rule_to_fixed_point(rule, fix.as_ref(), &content, cfg_name)
        })
}

/// Re-runs the probe pass until it changes nothing, failing on a revisited state or on
/// exceeding the production pass cap, then checks one `apply_safe_fixes` call lands on
/// that fixed point and stays there. Returns the number of passes that changed the input.
fn assert_converges(
    input: &str,
    cfg: &YamlLintConfig,
    cfg_name: &str,
) -> Result<usize, TestCaseError> {
    let rules = pipeline_rules(cfg);
    let fixed = apply_safe_fixes(input, cfg, synthetic_path(), synthetic_base_dir());
    let mut seen = vec![input.to_string()];
    for pass in 0..FIX_PIPELINE_MAX_PASSES {
        let state = &seen[pass];
        let next = probe_pass(state, &rules, cfg_name)?;
        if &next == state {
            prop_assert_eq!(
                &fixed,
                state,
                "rule-by-rule probe disagrees with apply_safe_fixes under config '{}'; input {:?}",
                cfg_name,
                input
            );
            let refixed =
                apply_safe_fixes(&fixed, cfg, synthetic_path(), synthetic_base_dir());
            prop_assert_eq!(
                &refixed,
                &fixed,
                "a second apply_safe_fixes still changes the output under config '{}'; input {:?}",
                cfg_name,
                input
            );
            return Ok(pass);
        }
        prop_assert!(
            !seen.contains(&next),
            "fix pipeline cycles after {} passes under config '{}'; input {:?}; states {:?}",
            pass + 1,
            cfg_name,
            input,
            seen
        );
        seen.push(next);
    }
    Err(TestCaseError::fail(format!(
        "fix pipeline did not converge within {FIX_PIPELINE_MAX_PASSES} passes under config '{cfg_name}'; input {input:?}; states {seen:?}"
    )))
}

proptest! {
    #![proptest_config(ProptestConfig {
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
            "tests/proptest-regressions/property_fix_convergence.txt",
        ))),
        ..ProptestConfig::default()
    })]

    #[test]
    fn safe_fix_pipeline_converges(document in arb_stacked_document()) {
        let input = document.render();
        for prepared in safe_fix_configs() {
            assert_converges(&input, &prepared.cfg, prepared.name)?;
        }
    }
}

#[test]
fn probe_covers_every_safe_fix_rule() {
    let probed: Vec<&str> = pipeline_rules(named_config("best-practice"))
        .iter()
        .map(|(rule, _)| *rule)
        .collect();
    let mut sorted = probed.clone();
    sorted.sort_unstable();
    let mut expected = SAFE_FIX_RULES.to_vec();
    expected.sort_unstable();
    assert_eq!(
        sorted, expected,
        "probe rule table must match SAFE_FIX_RULES: {probed:?}"
    );
}

#[test]
fn stacked_input_engages_several_fixers_and_converges() {
    let document = dirty_stacked_document();
    let input = document.render();
    assert!(
        parse_for_compare(&input).is_some(),
        "stacked input must parse: {input:?}"
    );
    for prepared in safe_fix_configs() {
        let engaged: Vec<&str> = pipeline_rules(&prepared.cfg)
            .iter()
            .filter(|(_, fix)| fix(&input).is_some_and(|fixed| fixed != input))
            .map(|(rule, _)| *rule)
            .collect();
        assert!(
            engaged.len() >= 6,
            "stacked input must engage several fixers under config '{}': {engaged:?}",
            prepared.name
        );
        let passes = assert_converges(&input, &prepared.cfg, prepared.name)
            .unwrap_or_else(|err| panic!("{err}"));
        assert_eq!(
            passes, 1,
            "pipeline must settle in one pass under '{}'",
            prepared.name
        );
    }
}

#[test]
fn comment_left_by_joined_plain_scalar_is_fixed_in_one_call() {
    let passes = assert_converges(
        "a: b\n  c\n  # x\nd: e\n",
        named_config("yamllint-default"),
        "yamllint-default",
    )
    .unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(
        passes, 2,
        "quoted-strings joins the scalar after comments-indentation has run, so the \
         comment is only re-indented on a second pass"
    );
}

fn dirty_stacked_document() -> StackedDocument {
    use ast::{
        BlockEntry, Document, FlowStyle, InlineComment, NewlineStyle, Node, Scalar,
    };
    let plain = |text: &str| Node::Scalar(Scalar::Plain(text.to_string()));
    let entry = |key: &str, value, comment| BlockEntry {
        key: key.to_string(),
        value,
        trailing_inline_comment: comment,
    };
    let dirty_flow = FlowStyle {
        inner_padding: 1,
        spaces_before_comma: 1,
        spaces_after_comma: 2,
        space_after_colon: true,
    };
    StackedDocument {
        document: Document {
            version_directive: None,
            entries: vec![
                entry(
                    "items",
                    Node::FlowSeq(vec![plain("a"), plain("b")], dirty_flow),
                    Some(InlineComment {
                        spaces_after_hash: 0,
                        text: "note".to_string(),
                    }),
                ),
                entry(
                    "name",
                    Node::Scalar(Scalar::DoubleQuoted("x".to_string())),
                    None,
                ),
            ],
            newline: NewlineStyle::Crlf,
            has_final_newline: false,
        },
        decorations: vec![
            Decoration {
                leading: Vec::new(),
                trailing_spaces: 2,
            },
            Decoration {
                leading: vec![
                    Filler::Blank { spaces: 1 },
                    Filler::Blank { spaces: 0 },
                    Filler::Blank { spaces: 0 },
                    Filler::Comment {
                        indent: 2,
                        spaces_after_hash: 0,
                        text: "lead".to_string(),
                    },
                ],
                trailing_spaces: 0,
            },
        ],
        start_marker: false,
        end_marker: false,
        trailing_blank_lines: 2,
    }
}
