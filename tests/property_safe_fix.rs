//! Property-based tests for `apply_safe_fixes`.
//!
//! Submodules:
//!  * `config` — the named-config matrix that the suite runs each invariant
//!    against, plus shared parse/lint helpers.
//!  * `ast` — the synthetic YAML AST (`Document`, `Node`, `Scalar`, …) used
//!    by the generator, together with rendering and the
//!    "is this input expected to leave residue under a partial safe fix?"
//!    predicate.
//!  * `strategy` — proptest strategies that build random `Document` values.
//!
//! This file holds the `proptest!` invariants (idempotence, residual
//! diagnostics, parse preservation) and a handful of deterministic
//! regressions that pin known-dirty inputs and production-bug patterns
//! (issues #184, #206, BOM preservation) through the same machinery.

#[path = "property_safe_fix/ast.rs"]
mod ast;
#[path = "property_safe_fix/config.rs"]
mod config;
#[path = "property_safe_fix/strategy.rs"]
mod strategy;

use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use ryl::config::YamlLintConfig;
use ryl::fix::apply_safe_fixes;

use ast::{BlockEntry, Document, FlowStyle, InlineComment, NewlineStyle, Node, Scalar};
use config::{
    named_config, parse_for_compare, safe_fix_configs, safe_fix_rule_diagnostics,
    synthetic_base_dir, synthetic_path,
};
use strategy::arb_document;

proptest! {
    #![proptest_config(ProptestConfig {
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
            "tests/proptest-regressions/property_safe_fix.txt",
        ))),
        ..ProptestConfig::default()
    })]

    #[test]
    fn safe_fix_is_idempotent(document in arb_document()) {
        let input = document.render();
        for prepared in safe_fix_configs() {
            let cfg_name = prepared.name;
            let cfg = &prepared.cfg;
            let once =
                apply_safe_fixes(&input, cfg, synthetic_path(), synthetic_base_dir());
            let twice =
                apply_safe_fixes(&once, cfg, synthetic_path(), synthetic_base_dir());
            prop_assert_eq!(
                &once,
                &twice,
                "applying safe fixes is not idempotent under config '{}' for input {:?}; once -> {:?}; twice -> {:?}",
                cfg_name,
                input,
                once,
                twice
            );
        }
    }

    #[test]
    fn safe_fix_leaves_no_safe_fix_rule_diagnostics(document in arb_document()) {
        if document.has_partial_safe_fix_residue() {
            return Ok(());
        }
        let input = document.render();
        for prepared in safe_fix_configs() {
            let cfg_name = prepared.name;
            let cfg = &prepared.cfg;
            let fixed =
                apply_safe_fixes(&input, cfg, synthetic_path(), synthetic_base_dir());
            if parse_for_compare(&fixed).is_none() {
                continue;
            }
            let remaining = safe_fix_rule_diagnostics(&fixed, cfg);
            prop_assert!(
                remaining.is_empty(),
                "safe-fix-rule diagnostics survived fix under config '{}' for input {:?}; fixed {:?}; diagnostics {:?}",
                cfg_name,
                input,
                fixed,
                remaining
            );
        }
    }

    #[test]
    fn safe_fix_preserves_parsed_value(document in arb_document()) {
        let input = document.render();
        let Some(before) = parse_for_compare(&input) else {
            return Ok(());
        };
        for prepared in safe_fix_configs() {
            let cfg_name = prepared.name;
            let cfg = &prepared.cfg;
            let fixed =
                apply_safe_fixes(&input, cfg, synthetic_path(), synthetic_base_dir());
            let after = parse_for_compare(&fixed).ok_or_else(|| {
                TestCaseError::fail(format!(
                    "safe fix broke a previously-parseable document under config '{cfg_name}'; input {input:?}; fixed {fixed:?}"
                ))
            })?;
            prop_assert_eq!(
                &before,
                &after,
                "safe fix changed parsed YAML value under config '{}'; input {:?}; fixed {:?}",
                cfg_name,
                input,
                fixed
            );
        }
    }
}

#[test]
fn safe_fix_properties_hold_for_known_dirty_input() {
    let plain = |name: &str| Node::Scalar(Scalar::Plain(name.to_string()));
    let dirty_flow_seq = Document {
        entries: vec![BlockEntry {
            key: "items".to_string(),
            value: Node::FlowSeq(
                vec![plain("a"), plain("b")],
                FlowStyle {
                    inner_padding: 1,
                    spaces_before_comma: 1,
                    spaces_after_comma: 2,
                    space_after_colon: true,
                },
            ),
            trailing_inline_comment: Some(InlineComment {
                spaces_after_hash: 0,
                text: "trailing".to_string(),
            }),
        }],
        newline: NewlineStyle::Crlf,
        has_final_newline: false,
    };
    let input = dirty_flow_seq.render();
    let before = parse_for_compare(&input).expect("known dirty input must parse");
    for prepared in safe_fix_configs() {
        let cfg_name = prepared.name;
        let cfg = &prepared.cfg;
        let fixed =
            apply_safe_fixes(&input, cfg, synthetic_path(), synthetic_base_dir());
        assert_ne!(
            input, fixed,
            "renderer must emit inputs that exercise safe fixers under config '{cfg_name}'; input={input:?} fixed={fixed:?}"
        );
        let after = parse_for_compare(&fixed).unwrap_or_else(|| {
            panic!(
                "safe fix broke parseable input under config '{cfg_name}': {fixed:?}"
            )
        });
        assert_eq!(
            before, after,
            "safe fix must preserve parsed value under config '{cfg_name}'"
        );
        let remaining = safe_fix_rule_diagnostics(&fixed, cfg);
        assert!(
            remaining.is_empty(),
            "safe-fix-rule diagnostics must clear after fix under config '{cfg_name}': {remaining:?}"
        );
        let twice =
            apply_safe_fixes(&fixed, cfg, synthetic_path(), synthetic_base_dir());
        assert_eq!(
            fixed, twice,
            "safe fix must be idempotent under config '{cfg_name}'"
        );
    }
}

#[test]
fn best_practice_retains_quotes_around_yaml_metachars() {
    let input = "schedule: '30 21 * * 0'\n";
    let cfg = named_config("best-practice");
    let before = parse_for_compare(input).expect("input parses");
    let fixed = apply_safe_fixes(input, cfg, synthetic_path(), synthetic_base_dir());
    let after = parse_for_compare(&fixed)
        .expect("best-practice fix must keep cron-like input parseable");
    assert_eq!(before, after, "parse must be preserved: {fixed:?}");
    assert!(
        fixed.contains("'30 21 * * 0'"),
        "best-practice must retain quotes around scalars containing YAML metachars (issue #206): {fixed:?}"
    );
}

#[test]
fn best_practice_does_not_break_parse_for_escape_sequences() {
    let input = "message: \"line1\\nline2\"\n";
    let cfg = named_config("best-practice");
    let before = parse_for_compare(input).expect("input parses");
    let fixed = apply_safe_fixes(input, cfg, synthetic_path(), synthetic_base_dir());
    let after = parse_for_compare(&fixed)
        .expect("best-practice fix must keep escape-sequence input parseable");
    assert_eq!(
        before, after,
        "safe fix must not change parsed value of escape-bearing scalars (issue #184): {fixed:?}"
    );
}

#[test]
fn best_practice_preserves_trailing_comment_when_unquoting() {
    let input = "key: 'value'  # important comment\n";
    let cfg = named_config("best-practice");
    let before = parse_for_compare(input).expect("input parses");
    let fixed = apply_safe_fixes(input, cfg, synthetic_path(), synthetic_base_dir());
    let after = parse_for_compare(&fixed)
        .expect("best-practice fix must keep quoted-with-comment input parseable");
    assert_eq!(before, after, "parse must be preserved: {fixed:?}");
    assert!(
        fixed.contains("# important comment"),
        "trailing comment must survive quote removal (issue #206): {fixed:?}"
    );
}

#[test]
fn document_start_fix_keeps_utf8_bom_at_stream_start() {
    let input = "\u{feff}key: value\n";
    let cfg = YamlLintConfig::from_yaml_str("rules:\n  document-start: enable\n")
        .expect("config parses");
    let fixed = apply_safe_fixes(input, &cfg, synthetic_path(), synthetic_base_dir());
    assert_eq!(
        fixed, "\u{feff}---\nkey: value\n",
        "BOM must stay at byte 0 when --- is prepended: {fixed:?}"
    );
}
