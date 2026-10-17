//! Property-based tests for `apply_safe_fixes`.
//!
//! Submodules:
//!  * `config`: the named-config matrix that the suite runs each invariant
//!    against, plus shared parse/lint helpers.
//!  * `ast`: the synthetic YAML AST (`Document`, `Node`, `Scalar`, …) used
//!    by the generator, together with rendering.
//!  * `strategy`: proptest strategies that build random `Document` values.
//!
//! These invariants assert that `apply_safe_fixes` is *sound*, not *complete*:
//! a safe fix must never change a document's meaning, but it may leave some
//! reported violations unfixed (e.g. an occurrence in a context the fixer must
//! not rewrite). This file holds four `proptest!` soundness invariants
//! (idempotence, parse preservation, the leading-`# ryl disable` no-op, and that
//! any reported progress, a drop in diagnostic count, corresponds to a real
//! change to the file) plus deterministic regressions that pin known-dirty inputs
//! and production-bug patterns (escape sequences, YAML metachars, BOM
//! preservation), including effectiveness (`safe_fix_rule_diagnostics` clears on
//! concrete inputs the fixer fully cleans).

#[path = "property_safe_fix/ast.rs"]
mod ast;
#[path = "property_safe_fix/config.rs"]
mod config;
#[path = "quote_cache.rs"]
mod quote_cache;
#[path = "property_safe_fix/strategy.rs"]
mod strategy;

use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use ryl::config::YamlLintConfig;
use ryl::fix::apply_safe_fixes;

use ast::{
    BlockBodyLine, BlockEntry, BlockScalarSpec, ColonGap, Document, FlowStyle,
    InlineComment, Layout, NewlineStyle, Node, Scalar, SeqBody,
};
use config::{
    named_config, parse_for_compare, reference_safe_eof_append, safe_fix_configs,
    safe_fix_rule_diagnostics, synthetic_base_dir, synthetic_path,
};
use strategy::arb_document;

#[test]
fn repeated_quote_probe_matches_first_parse() {
    for value in ["word", "", "a: b", "#hash", "can't", "[x]", "é"] {
        quote_cache::assert_cached_quote_matches_fresh(value);
    }
}

proptest! {
    #[test]
    fn quoted_payload_cache_matches_first_parse(value in strategy::arb_quoted_payload()) {
        quote_cache::assert_cached_quote_matches_fresh(&value);
    }
}

fn last_block_scalar(node: &Node) -> Option<&BlockScalarSpec> {
    match node {
        Node::BlockScalar(spec) => Some(spec),
        Node::BlockMap(entries) => entries
            .last()
            .and_then(|entry| last_block_scalar(&entry.value)),
        Node::BlockSeq(items) => items.last().and_then(|item| match &item.body {
            SeqBody::BlockScalar(spec) => Some(spec),
            SeqBody::Inline(node) => last_block_scalar(node),
            SeqBody::TaggedMap(_, entries) | SeqBody::CompactMap(entries) => entries
                .last()
                .and_then(|entry| last_block_scalar(&entry.value)),
            SeqBody::CompactSeq(items) => {
                items.last().and_then(|(_, node)| last_block_scalar(node))
            }
            _ => None,
        }),
        _ => None,
    }
}

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

    /// A leading `# ryl disable` mutes every rule, so `apply_safe_fixes` must not
    /// touch a single line: the output equals the input byte-for-byte under every
    /// config. This is the strongest form of "--fix never rewrites a disabled line".
    #[test]
    fn safe_fix_is_a_noop_under_a_leading_disable(document in arb_document()) {
        let input = format!("# ryl disable\n{}", document.render());
        for prepared in safe_fix_configs() {
            let cfg_name = prepared.name;
            let cfg = &prepared.cfg;
            let fixed =
                apply_safe_fixes(&input, cfg, synthetic_path(), synthetic_base_dir());
            prop_assert_eq!(
                &fixed,
                &input,
                "leading `# ryl disable` must make safe fixes a no-op under config '{}'; input {:?}; fixed {:?}",
                cfg_name,
                input,
                fixed
            );
        }
    }

    #[test]
    fn safe_fix_preserves_parsed_value(document in arb_document()) {
        let input = document.render();
        // granit counts this terminal blank; the reference parser clips it.
        let clipped_blank_at_eof = !document.has_final_newline
            && document.entries.last().is_some_and(|entry| {
                matches!(&entry.value, Node::BlockScalar(spec)
                    if matches!(spec.style, '|' | '>')
                        && spec.chomp.is_none()
                        && matches!(spec.body.last(), Some(BlockBodyLine::Spaces(_)))
                        && spec.body.iter().any(|line| matches!(line, BlockBodyLine::Content { .. })))
            });
        let last_line = input.rsplit(['\n', '\r']).next().unwrap_or_default();
        let keep_blank_at_eof = !document.has_final_newline
            && !last_line.is_empty()
            && last_line.bytes().all(|byte| byte == b' ')
            && document.entries.last().and_then(|entry| last_block_scalar(&entry.value))
                .is_some_and(|spec| spec.chomp == Some('+') && spec.trailing_comment.is_none());
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
            if before != after && reference_safe_eof_append(&input, &fixed) {
                continue;
            } else if before != after && keep_blank_at_eof {
                // The reference parser retains the keep-chomped EOF blank line that the loader drops.
                let newline = match document.newline {
                    NewlineStyle::Lf => "\n",
                    NewlineStyle::Crlf => "\r\n",
                    NewlineStyle::Cr => "\r",
                };
                let reference_input = format!("{}{newline}", &input[..input.len() - last_line.len()]);
                prop_assert!(
                    parse_for_compare(&reference_input).as_ref() == Some(&after),
                    "safe fix changed reference YAML value under config '{cfg_name}'; input {input:?}; fixed {fixed:?}"
                );
            } else if before != after && clipped_blank_at_eof {
                let reference_equal = noyalib::from_str::<noyalib::Value>(&input)
                    .ok()
                    .zip(noyalib::from_str::<noyalib::Value>(&fixed).ok())
                    .is_some_and(|(before, after)| before == after);
                prop_assert!(
                    reference_equal,
                    "safe fix changed reference YAML value under config '{cfg_name}'; input {input:?}; fixed {fixed:?}"
                );
            } else {
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

    /// Soundness of progress reporting: `ryl --fix` reports "N fixed" as the drop
    /// in diagnostic count (problems before minus after), so any reported fix must
    /// correspond to a real change to the file. If fixing reduces the safe-fix
    /// diagnostic count, the output must differ from the input; a no-op that
    /// reports progress would be a bug.
    #[test]
    fn safe_fix_reporting_progress_implies_mutation(document in arb_document()) {
        let input = document.render();
        for prepared in safe_fix_configs() {
            let cfg_name = prepared.name;
            let cfg = &prepared.cfg;
            let before = safe_fix_rule_diagnostics(&input, cfg).len();
            let fixed =
                apply_safe_fixes(&input, cfg, synthetic_path(), synthetic_base_dir());
            let after = safe_fix_rule_diagnostics(&fixed, cfg).len();
            if after < before {
                prop_assert_ne!(
                    &fixed,
                    &input,
                    "fix reported progress ({} -> {} safe-fix diagnostics) but left the file byte-for-byte unchanged under config '{}'; input {:?}",
                    before,
                    after,
                    cfg_name,
                    input
                );
            }
        }
    }
}

#[test]
fn safe_fix_properties_hold_for_known_dirty_input() {
    let plain = |name: &str| Node::Scalar(Scalar::Plain(name.to_string()));
    let dirty_flow_seq = Document {
        version_directive: None,
        entries: vec![BlockEntry {
            leading_comment: None,
            key: "items".to_string(),
            colon: ColonGap::default(),
            value: Node::FlowSeq(
                vec![plain("TRUE"), plain("false")],
                FlowStyle {
                    inner_padding: 1,
                    spaces_before_comma: 1,
                    spaces_after_comma: 2,
                    spaces_before_colon: 0,
                    spaces_after_colon: 1,
                },
            ),
            trailing_inline_comment: Some(InlineComment {
                whitespace_before_hash: "  ".to_string(),
                spaces_after_hash: 0,
                text: "trailing".to_string(),
            }),
            layout: Layout::default(),
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
        // Zero tolerance still flags the space the grammar requires, which no fix removes.
        let remaining: Vec<_> = safe_fix_rule_diagnostics(&fixed, cfg)
            .into_iter()
            .filter(|problem| {
                cfg_name != "spacing-zero"
                    || !matches!(problem.rule, Some("colons" | "hyphens"))
            })
            .collect();
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
fn keep_scalar_eof_uses_reference_value_when_loader_disagrees() {
    let input = "a: |+\n  \n    ";
    let fixed = apply_safe_fixes(
        input,
        named_config("yamllint-default"),
        synthetic_path(),
        synthetic_base_dir(),
    );
    assert_eq!(fixed, "---\na: |+\n\n\n...\n");
    assert_ne!(parse_for_compare(input), parse_for_compare(&fixed));
    assert_eq!(
        parse_for_compare("a: |+\n  \n\n"),
        parse_for_compare(&fixed)
    );
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
fn best_practice_unquotes_after_multibyte_comment() {
    let input = "# —\nfoo: 'value'\n";
    let cfg = named_config("best-practice");
    let before = parse_for_compare(input).expect("input parses");
    let fixed = apply_safe_fixes(input, cfg, synthetic_path(), synthetic_base_dir());
    let after = parse_for_compare(&fixed)
        .expect("multibyte-comment input must remain parseable after fix");
    assert_eq!(before, after, "parse must be preserved: {fixed:?}");
    assert!(
        fixed.contains("# —\n") && fixed.contains("foo: value\n"),
        "byte offsets after a multibyte comment must not corrupt the fix (issue #232): {fixed:?}"
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
