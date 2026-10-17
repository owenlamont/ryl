//! The formatter's guarantee as properties: every pass in `passes::format_passes` must be
//! idempotent, parse-preserving, value-preserving at the representation level (tags,
//! aliases, duplicate keys and entry order included, resolved against the declared YAML
//! version and under YAML 1.1 too), and must keep every comment beside
//! its node and every anchor and alias name. Folding also keeps the two
//! parser-independent properties in `fold`: only lone spaces become line breaks, and
//! every continuation is deeper than its scalar's owner.
//! G11 excludes complementary line-length lint: docs/formatter.md defines a soft target.
//! G11 exempts value-bearing scalar whitespace: the repair (including a final block marker's newline) must change the independently loaded value.
//! G11 exempts diagnostics matching an actual formatter refusal notice in position and concern.

#[path = "property_safe_fix/ast.rs"]
mod ast;
#[path = "property_safe_fix/config.rs"]
#[expect(
    dead_code,
    reason = "shared with the safe-fix suite, which uses every item"
)]
mod config;
#[path = "property_format/consistency.rs"]
mod consistency;
#[path = "common/encoding.rs"]
mod encoding;
#[path = "property_format/fold.rs"]
mod fold;
#[path = "property_format/passes.rs"]
mod passes;
#[path = "property_format/properties.rs"]
mod properties;
#[path = "property_format/representation.rs"]
mod representation;
#[path = "property_format/settings.rs"]
mod settings;
#[path = "property_fix_convergence/stack.rs"]
mod stack;
#[path = "property_safe_fix/strategy.rs"]
mod strategy;

use std::collections::BTreeSet;

use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use ryl::config::YamlLintConfig;
use ryl::fix::apply_safe_fixes;
use ryl::format::file_indent_width;
use ryl::lint::lint_str;
use ryl::rules::indentation::{self, IndentSequencesSetting, SpacesSetting};
use similar::{DiffTag, TextDiff};

use config::{synthetic_base_dir, synthetic_path};
use fold::{arb_fold_document, inserted_breaks, under_indented_break};
use passes::{
    FOLD_TARGETS, FORMAT_OWNED_RULES, FormatPass, fold_alone, format_passes,
    named_pass, yaml_rules_for,
};
use properties::arb_document_with_properties;
use representation::{
    Node, Value, annotations, check_annotations, check_values,
    check_yaml_1_1_preserved, representation,
};

#[test]
fn reference_block_scalar_matrix_keeps_each_value() {
    #[derive(serde::Deserialize)]
    struct Case {
        family: String,
        input: String,
        expected: String,
        event: String,
    }
    let cases: Vec<Case> =
        serde_json::from_str(include_str!("fixtures/reference_block_scalars.json"))
            .expect("reference parser matrix is valid JSON");
    let format_cfg = YamlLintConfig::from_toml_str("[format]").unwrap();
    let fix_cfg =
        YamlLintConfig::from_toml_str("[lint.rules]\ntrailing-spaces = 'enable'\n")
            .unwrap();
    for case in cases {
        let expected =
            Value::Core(ryl::yaml_dom::ScalarOwned::String(case.expected.clone()));
        let has_value = |text: &str| {
            representation(text).is_some_and(|nodes| {
                nodes.iter().any(|node| matches!(node, Node::Scalar { value, .. } if value == &expected))
            })
        };
        assert!(
            has_value(&case.input),
            "{}: {:?}: {}: {:?}",
            case.family,
            case.input,
            case.event,
            representation(&case.input)
        );
        if case.expected.contains(' ') {
            assert!(
                ryl::rules::trailing_spaces::check(&case.input).is_empty(),
                "{}: {:?}: {}",
                case.family,
                case.input,
                case.event
            );
        }
        let formatted =
            ryl::format::format_str(&case.input, &format_cfg, synthetic_path(), &[]);
        assert!(
            has_value(&formatted),
            "{}: {:?} -> {:?}: {}",
            case.family,
            case.input,
            formatted,
            case.event
        );
        check_values(&case.input, &formatted).unwrap();
        let fixed = apply_safe_fixes(
            &case.input,
            &fix_cfg,
            synthetic_path(),
            synthetic_base_dir(),
        );
        assert!(
            has_value(&fixed),
            "{}: {:?} -> {:?}: {}",
            case.family,
            case.input,
            fixed,
            case.event
        );
        check_values(&case.input, &fixed).unwrap();
    }
}

#[test]
fn block_scalar_tab_after_indent_keeps_reference_value() {
    let input = "k0: |\n  aaa aaa\n  \taaa |\n";
    let expected = Value::Core(ryl::yaml_dom::ScalarOwned::String(
        "aaa aaa\n\taaa |\n".into(),
    ));
    assert!(
        representation(input).is_some_and(|nodes| {
            nodes.iter().any(
                |node| matches!(node, Node::Scalar { value, .. } if value == &expected),
            )
        }),
        "{:?}",
        representation(input)
    );
    check_pass(named_pass("fix/yamllint-default"), input).unwrap();
}

#[test]
fn root_explicit_block_scalar_properties_keep_reference_value() {
    for (input, expected) in [
        (
            "&a |2\n  x\n  ",
            Value::Core(ryl::yaml_dom::ScalarOwned::String(" x\n \n".into())),
        ),
        ("!foo |2\n  x\n  ", Value::Application(" x\n \n".into())),
    ] {
        let nodes = representation(input).expect("reference root scalar parses");
        assert!(
            nodes.iter().any(
                |node| matches!(node, Node::Scalar { value, .. } if value == &expected)
            ),
            "{input:?}: reference event has scalar value {:?}; oracle {nodes:?}",
            expected
        );
        let cfg = YamlLintConfig::from_toml_str("[format]").unwrap();
        let output = ryl::format::format_str(input, &cfg, synthetic_path(), &[]);
        check_values(input, &output).unwrap();
    }
}

#[test]
fn nested_blank_block_scalar_before_dedented_mapping_is_not_rewritten() {
    let input = "a:\n  b: |-\n    \n\nc: x\n";
    let format_cfg =
        YamlLintConfig::from_toml_str("[format]\nmax-blank-lines = 0\n").unwrap();
    let fix_cfg = YamlLintConfig::from_toml_str(
        "[lint.rules]\ntrailing-spaces = 'enable'\nempty-lines = 'enable'\n",
    )
    .unwrap();
    assert_eq!(
        ryl::format::format_str(input, &format_cfg, synthetic_path(), &[]),
        input
    );
    assert_eq!(
        apply_safe_fixes(input, &fix_cfg, synthetic_path(), synthetic_base_dir()),
        input
    );
    assert!(
        ryl::format::refusals(input, &format_cfg)
            .iter()
            .any(|notice| {
                notice.message
                    == "cannot format this file safely: block scalar value may change"
            })
    );
}

#[test]
fn explicit_blank_scalar_before_document_end_uses_key_indent() {
    let input = "? a\n:  a: |3+\n    \n\n\n...\n# gap\nvz: 1\n";
    assert!(ryl::rules::trailing_spaces::check(input).is_empty());
    assert!(ryl::rules::trailing_spaces::fix(input).is_none());
    let expected = Value::Core(ryl::yaml_dom::ScalarOwned::String("\n\n\n\n".into()));
    assert!(
        representation(input).is_some_and(|nodes| {
            nodes.iter().any(
                |node| matches!(node, Node::Scalar { value, .. } if value == &expected),
            )
        }),
        "{:?}",
        representation(input)
    );
    let cfg = YamlLintConfig::from_toml_str("[format]\n").unwrap();
    let output = ryl::format::format_str(input, &cfg, synthetic_path(), &[]);
    check_values(input, &output).unwrap();
}

#[test]
fn compact_mapping_blank_scalar_before_document_end_keeps_reference_value() {
    let input = "a:\n  - a: |\n      \n...\na: 1\n";
    let expected = Value::Core(ryl::yaml_dom::ScalarOwned::String("      \n".into()));
    assert!(representation(input).is_some_and(|nodes| {
        nodes.iter().any(
            |node| matches!(node, Node::Scalar { value, .. } if value == &expected),
        )
    }));
    let cfg = YamlLintConfig::from_toml_str("indent-width = 76\n[format]\n").unwrap();
    let output = ryl::format::format_str(input, &cfg, synthetic_path(), &[]);
    check_values(input, &output).unwrap();
}

#[test]
fn explicit_key_sequence_blank_scalar_uses_the_dash_indent() {
    for input in [
        "? a\n: - |1   \n    \n\n\n\n",
        "---\n? a\n: - |1\n    \n\n\n\n...\n",
    ] {
        let expected = Value::Core(ryl::yaml_dom::ScalarOwned::String(" \n".into()));
        let nodes = representation(input).expect("explicit key scalar parses");
        assert!(
            nodes.iter().any(
                |node| matches!(node, Node::Scalar { value, .. } if value == &expected),
            ),
            "{input:?}: {nodes:?}"
        );
    }
}

#[test]
fn compact_explicit_key_scalar_refuses_marker_shift() {
    let input = "? a\r\n:   -   !!str >  \r\n    \r\n    -   !local\r\n      a: [p_0 ,tRUE ,'True']\r\n";
    let format_cfg = YamlLintConfig::from_toml_str("[format]\n").unwrap();
    assert_eq!(
        ryl::format::format_str(input, &format_cfg, synthetic_path(), &[]),
        input
    );
    assert!(
        ryl::format::refusals(input, &format_cfg)
            .iter()
            .any(|notice| {
                notice.message
                    == "cannot format this file safely: block scalar value may change"
            })
    );
    let fix_cfg = YamlLintConfig::from_toml_str(
        "[lint.rules]\ncolons = 'enable'\nhyphens = 'enable'\ntrailing-spaces = 'enable'\n",
    )
    .unwrap();
    assert_eq!(
        apply_safe_fixes(input, &fix_cfg, synthetic_path(), synthetic_base_dir()),
        input
    );
}

#[test]
fn nested_blank_scalar_before_sibling_key_refuses_reindent() {
    let input =
        "? a\n: -\n    a: &blk >\n      \n        \n\n         \n\n\n    a: x\n";
    let expected = Value::Core(ryl::yaml_dom::ScalarOwned::String(
        " \n   \n\n    \n".into(),
    ));
    assert!(representation(input).is_some_and(|nodes| {
        nodes.iter().any(
            |node| matches!(node, Node::Scalar { value, .. } if value == &expected),
        )
    }));
    let cfg = YamlLintConfig::from_toml_str("[format]\n").unwrap();
    assert_eq!(
        ryl::format::format_str(input, &cfg, synthetic_path(), &[]),
        input
    );
    assert!(ryl::format::refusals(input, &cfg).iter().any(|notice| {
        notice.message
            == "cannot format this file safely: block scalar value may change"
    }));
}

#[test]
fn keep_scalar_with_comment_shaped_content_refuses_rewrite() {
    let input = "a:\n  - |+\n    #x \n\n  - y\n";
    let cfg = YamlLintConfig::from_toml_str("[format]\n").unwrap();
    assert_eq!(
        ryl::format::format_str(input, &cfg, synthetic_path(), &[]),
        input
    );
    assert!(ryl::format::refusals(input, &cfg).iter().any(|notice| {
        notice.message
            == "cannot format this file safely: block scalar value may change"
    }));
}

#[test]
fn cr_only_scalar_refusal_exempts_unchanged_lint_findings() {
    let input = "a:\r   - |+  \r     #grj  \r     paa5114   \r\r\r";
    let cfg = YamlLintConfig::from_toml_str(&consistency::agreeing_lint("[format]\n"))
        .unwrap();
    let (output, notices) = ryl::fix::rewrite_str(
        input,
        &cfg,
        synthetic_path(),
        synthetic_base_dir(),
        ryl::config::SourceKind::Yaml,
        ryl::fix::Rewrite::Format,
    );
    assert!(output.is_none());
    let problems = lint_str(input, synthetic_path(), &cfg, synthetic_base_dir());
    assert!(
        problems.iter().all(|problem| {
            consistency::refused(input, input, problem, &notices, &problems)
        }),
        "{notices:?}: {problems:?}"
    );
}

#[test]
fn folded_keep_terminal_blank_line_matches_reference_value() {
    for (input, expected) in [
        ("a: >+2\n  ", "\n\n"),
        ("a: >+2\n\n", "\n\n"),
        ("a: >+\n  x\n  ", "x\n\n\n"),
        ("a: >+\n  \n  ", "\n\n\n\n"),
        ("a: >+\n  \n\n", "\n\n\n\n"),
        ("a: >1+\n   \n\n", "  \n\n\n\n"),
        ("- >+2\n ", "\n\n"),
        ("- >+2\n  ", "\n\n"),
        ("- >+2\n   ", " \n"),
        ("'it''s': >1+\n    \n", "   \n"),
        ("'it''s': >1+\n    \n\n", "   \n\n\n\n"),
        ("---\nit's: >1+\n    \n\n...\n", "   \n\n\n\n"),
    ] {
        let nodes = representation(input).expect("folded keep scalar parses");
        assert!(nodes.iter().any(|node| matches!(node, Node::Scalar { value: Value::Core(ryl::yaml_dom::ScalarOwned::String(value)), .. } if value == expected)),
            "{input:?}: expected reference value {expected:?}, got {nodes:?}");
    }
}

#[test]
fn explicit_terminal_content_line_matches_reference_value() {
    let input = "a: |1\n \n le4 ";
    let expected = Value::Core(ryl::yaml_dom::ScalarOwned::String("\nle4 \n".into()));
    assert!(representation(input).is_some_and(|nodes| {
        nodes.iter().any(
            |node| matches!(node, Node::Scalar { value, .. } if value == &expected),
        )
    }));
}

#[test]
fn wide_indent_with_blank_scalar_reports_a_refusal() {
    let input = "a:\r\n  -  a :  !!str |3- \r\n       \r\n         \r\n\r\n\r\n     # h3\r\n     h0_: {  '<<' : [  kc''1 ,\"0x-1\" ,tail \t ,'a:,!['  ]  }\r\n";
    let table = "indent-width = 188\nline-length = 18613\n[format]\nquote-style = 'preserve'\nline-ending = 'cr-lf'\ndocument-start = 'add'\ndocument-end = 'preserve'\nfold-long-lines = false\nbrace-spacing = false\npreview = false\ncomment-spacing = 145\ncomment-starting-space = 'preserve'\nmax-blank-lines = 137\nsequence-style = 'flow'\nmapping-style = 'preserve'\nindent-sequences = true\ndash-on-own-line = false\n";
    let agreeing = consistency::agreeing_lint(table);
    let cfg = YamlLintConfig::from_toml_str(&agreeing).unwrap();
    let (formatted, notices) = ryl::fix::rewrite_str(
        input,
        &cfg,
        synthetic_path(),
        synthetic_base_dir(),
        ryl::config::SourceKind::Yaml,
        ryl::fix::Rewrite::Format,
    );
    let output = formatted.unwrap_or_else(|| input.to_string());
    let before = lint_str(input, synthetic_path(), &cfg, synthetic_base_dir());
    let mut problems = lint_str(&output, synthetic_path(), &cfg, synthetic_base_dir());
    problems.retain(|problem| {
        !consistency::content_whitespace(
            &output,
            problem.rule,
            problem.line,
            problem.column,
        ) && !consistency::refused(input, &output, problem, &notices, &before)
    });
    assert!(
        problems.is_empty(),
        "output {output:?}; problems {problems:?}; notices {notices:?}"
    );
}

#[test]
fn literal_clip_blank_only_eof_matches_reference_value() {
    for input in ["a: |\n\n\n", "---\na: |\n\n\n...\n"] {
        let nodes = representation(input).expect("literal clip scalar parses");
        assert!(nodes.iter().any(|node| matches!(node, Node::Scalar { value: Value::Core(ryl::yaml_dom::ScalarOwned::String(value)), .. } if value == "\n")),
            "{input:?}: reference event is =VAL |\\n, oracle {nodes:?}");
        check_pass(named_pass("fix/yamllint-default"), input).unwrap();
    }
}

#[test]
fn one_terminal_blank_after_clipped_literal_is_removable() {
    let input = "a:\r  a: |\r\r";
    let cfg = YamlLintConfig::from_toml_str(&consistency::agreeing_lint("[format]\n"))
        .unwrap();
    let output = ryl::format::format_str(input, &cfg, synthetic_path(), &[]);
    assert!(!output.ends_with("\n\n"), "{output:?}");
    check_values(input, &output).unwrap();
    assert!(lint_str(&output, synthetic_path(), &cfg, synthetic_base_dir()).is_empty());
}

#[test]
fn leading_space_before_header_only_scalar_is_fixable() {
    let input = " \na: |";
    let cfg = YamlLintConfig::from_toml_str("[format]\n").unwrap();
    let output = ryl::format::format_str(input, &cfg, synthetic_path(), &[]);
    assert!(!output.starts_with(" \n"), "{output:?}");
    check_values(input, &output).unwrap();
}

#[test]
fn tagged_header_only_scalar_keeps_its_yaml_1_1_value() {
    check_yaml_1_1_preserved("a: !!str |", "a: !!str |\n...\n").unwrap();
}

/// yamllint's `default` preset, bar the lint-owned `truthy` and the preserved
/// `document-start`, and the quoted-strings options the formatter page documents as
/// accepting zero-config output.
fn profile_anchor_configs() -> &'static [YamlLintConfig; 2] {
    static CONFIGS: std::sync::LazyLock<[YamlLintConfig; 2]> = std::sync::LazyLock::new(
        || {
            [
                YamlLintConfig::from_yaml_str(
                    "extends: default\nrules:\n  truthy: disable\n  document-start: disable\n",
                )
                .expect("the default preset loads"),
                YamlLintConfig::from_toml_str(
                    "[lint.rules.quoted-strings]\nquote-type = 'single'\n\
                     required = 'only-when-needed'\n\
                     allow-double-quotes-for-escaping = true\nallow-quoted-quotes = true\n",
                )
                .expect("the quoted-strings config loads"),
            ]
        },
    );
    &CONFIGS
}

fn check_preserved(input: &str, output: &str) -> Result<(), String> {
    check_values(input, output)?;
    check_annotations(input, output)
}

fn check_invariants(
    format: &dyn Fn(&str) -> String,
    input: &str,
) -> Result<(), String> {
    let once = format(input);
    check_preserved(input, &once)?;
    let twice = format(&once);
    if twice != once {
        return Err(format!("idempotence: once {once:?}; twice {twice:?}"));
    }
    Ok(())
}

fn check_yaml_1_1_values(
    format: &dyn Fn(&str) -> String,
    input: &str,
) -> Result<(), String> {
    let once = format(input);
    check_yaml_1_1_preserved(input, &once)
}

fn check_pass(pass: &FormatPass, input: &str) -> Result<(), String> {
    check_invariants(&pass.format, input)
        .and_then(|()| check_yaml_1_1_values(&pass.format, input))
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
    fn random_format_config_keeps_the_guarantee(
        document in arb_document_with_properties(),
        table in settings::arb_format_config(),
    ) {
        let cfg = YamlLintConfig::from_toml_str(&table).expect(&table);
        let pass = FormatPass {
            name: table,
            cfg: cfg.clone(),
            format: Box::new(move |input| {
                ryl::format::format_str(input, &cfg, synthetic_path(), &[])
            }),
        };
        check_pass(&pass, &document.render()).map_err(TestCaseError::fail)?;
    }

    #[test]
    fn root_explicit_block_scalar_keeps_reference_value(
        style in prop::sample::select(vec!['|', '>']),
        chomp in prop::sample::select(vec!["", "-", "+"]),
        indent in 1usize..=4,
        marker in any::<bool>(),
    ) {
        let input = format!("{}{style}{indent}{chomp}\n{}x\n{}\n",
            if marker { "---\n" } else { "" },
            " ".repeat(indent), " ".repeat(indent));
        let cfg = YamlLintConfig::from_toml_str("[format]").unwrap();
        check_invariants(&|text| ryl::format::format_str(text, &cfg, synthetic_path(), &[]), &input)
            .map_err(TestCaseError::fail)?;
    }

    #[test]
    fn blank_only_block_scalar_at_eof_keeps_its_value(
        header in prop::sample::select(vec!["|", "|-", "|+", ">", ">-", ">+", "|2", "|2-", "|+2", "|2+", ">+2", ">2+"]),
        width in 1usize..=5,
        newline in prop::sample::select(vec!["\n", "\r\n"]),
        marker in any::<bool>(),
    ) {
        let mut input = format!("- {header}{newline}{}", " ".repeat(width));
        if marker {
            input.push_str(newline);
            input.push_str("...");
            input.push_str(newline);
        }
        for pass in [named_pass("format/default"), named_pass("format/non-defaults")] {
            check_pass(pass, &input).map_err(TestCaseError::fail)?;
        }
    }

}

proptest! {
    #![proptest_config(ProptestConfig {
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
            "tests/proptest-regressions/property_format_consistency.txt",
        ))),
        ..ProptestConfig::default()
    })]

    #[test]
    fn random_format_config_agrees_with_lint_and_conflicts(
        document in arb_document_with_properties(),
        table in settings::arb_format_config(),
    ) {
        let agreeing = consistency::agreeing_lint(&table);
        let cfg = YamlLintConfig::from_toml_str(&agreeing).expect(&agreeing);
        let input = document.render();
        let (formatted, refusals) = ryl::fix::rewrite_str(
            &input, &cfg, synthetic_path(), synthetic_base_dir(),
            ryl::config::SourceKind::Yaml, ryl::fix::Rewrite::Format,
        );
        let output = formatted.unwrap_or(input.clone());
        let before = lint_str(&input, synthetic_path(), &cfg, synthetic_base_dir());
        let mut problems = lint_str(&output, synthetic_path(), &cfg, synthetic_base_dir());
        problems.retain(|problem| !consistency::content_whitespace(&output, problem.rule, problem.line, problem.column) && !consistency::refused(&input, &output, problem, &refusals, &before));
        let conflicts = ryl::format::conflicts(&cfg);
        if !problems.is_empty() {
            prop_assert!(!conflicts.is_empty(), "missed conflict: {agreeing}\ninput {input:?}\noutput {output:?}\nproblems {problems:?}");
        }
        prop_assert!(problems.is_empty(), "{agreeing}\ninput {input:?}\noutput {output:?}\nproblems {problems:?}\nconflicts {conflicts:?}");
        prop_assert!(conflicts.is_empty(), "{agreeing}\nconflicts {conflicts:?}");
        let disagreeing = agreeing.replace(
            "[lint.rules.colons]\nmax-spaces-before = 0\nmax-spaces-after = 1",
            "[lint.rules.colons]\nmax-spaces-before = 0\nmax-spaces-after = 0",
        );
        let cfg = YamlLintConfig::from_toml_str(&disagreeing).expect(&disagreeing);
        let mut problems = lint_str(&output, synthetic_path(), &cfg, synthetic_base_dir());
        let before = lint_str(&input, synthetic_path(), &cfg, synthetic_base_dir());
        problems.retain(|problem| !consistency::content_whitespace(&output, problem.rule, problem.line, problem.column) && !consistency::refused(&input, &output, problem, &refusals, &before));
        if !problems.is_empty() {
            prop_assert!(!ryl::format::conflicts(&cfg).is_empty(), "{disagreeing}\noutput {output:?}\nproblems {problems:?}");
        }
    }

}

proptest! {
    #![proptest_config(ProptestConfig {
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
            "tests/proptest-regressions/property_format.txt",
        ))),
        ..ProptestConfig::default()
    })]

    #[test]
    fn encoded_previews_match_decoded_formatting(
        document in arb_document_with_properties(),
        width in prop::sample::select(vec![1usize, 2, 4]),
        little in any::<bool>(),
        bom in any::<bool>(),
    ) {
        let input = document.render();
        let bytes = encoding::encoded(&input, width, little, bom);
        let decoded = ryl::decoder::decode_bytes_lossless(&bytes).unwrap();
        let cfg = YamlLintConfig::from_toml_str("[format]").unwrap();
        let formatted = ryl::format::format_str(&input, &cfg, synthetic_path(), &[]);
        let outcome = ryl::fix::decoded_diff_outcome(
            &decoded, &cfg, synthetic_path(), synthetic_base_dir(),
            ryl::config::SourceKind::Yaml, ryl::fix::Rewrite::Format,
        );
        prop_assert_eq!(outcome.changed, input != formatted);
        prop_assert_eq!(decoded.encode(&formatted), encoding::encoded(&formatted, width, little, bom));
        if !decoded.is_plain_utf8() {
            prop_assert!(outcome.diff.is_none());
        }
    }

    #[test]
    fn zero_config_output_is_what_the_yamllint_default_preset_asks_for(
        document in arb_document_with_properties()
    ) {
        let output = (named_pass("format/default").format)(&document.render());
        for cfg in profile_anchor_configs() {
            let fixed = apply_safe_fixes(&output, cfg, synthetic_path(), synthetic_base_dir());
            prop_assert_eq!(&fixed, &output, "the preset's fixes still change the output");
        }
    }

    #[test]
    fn every_format_pass_keeps_the_guarantee(document in arb_document_with_properties()) {
        let input = document.render();
        for pass in format_passes() {
            check_pass(pass, &input).map_err(TestCaseError::fail)?;
        }
    }

    #[test]
    fn explicit_keys_do_not_hide_inconsistent_detected_indentation(
        width in 2usize..=8,
        compact in any::<bool>(),
        own_line in any::<bool>(),
        nested in any::<bool>(),
        indent_sequences in any::<bool>(),
        key in prop::sample::select(vec!["a", "[a, b]", "|\n  key"]),
    ) {
        let body = if compact {
            format!("? {key}\n: -  a: a\n     b: b\nb:\n{}- c\n", " ".repeat(width))
        } else {
            format!("? {key}\n: a\nb:\n{}- c\n", " ".repeat(width))
        };
        let input = if nested {
            format!("root:\n{}", body.lines().map(|line| format!("  {line}\n")).collect::<String>())
        } else {
            body
        };
        let cfg = YamlLintConfig::from_toml_str(&format!(
            "[format]\ndash-on-own-line = {own_line}\nindent-sequences = {indent_sequences}\n\
             [lint.rules.indentation]\nindent-sequences = {indent_sequences}\n"
        )).unwrap();
        let format = |text: &str| ryl::format::format_str(text, &cfg, synthetic_path(), &[]);
        check_invariants(&format, &input).map_err(TestCaseError::fail)?;
        prop_assert!(lint_str(&format(&input), synthetic_path(), &cfg, synthetic_base_dir()).is_empty());
    }

    #[test]
    fn converted_collections_keep_detected_width_lint_clean(
        width in 3usize..=8,
        mapping in any::<bool>(),
        folded in any::<bool>(),
        word in prop::sample::select(vec!["x", "long word", "café"]),
    ) {
        let scalar = if folded { '>' } else { '|' };
        let (style, collection) = if mapping {
            ("mapping", "{d: e}")
        } else {
            ("sequence", "[d, e]")
        };
        let input = format!(
            "a:\n- v: {scalar}\n{}{word}\n  ? x\n  : y\nb:\n- c: {collection}\n",
            " ".repeat(width + 2)
        );
        let cfg = YamlLintConfig::from_toml_str(&format!(
            "[format]\n{style}-style = 'block'\nindent-sequences = false\n\
             [lint.rules.indentation]\nindent-sequences = false\n"
        )).unwrap();
        let output = ryl::format::format_str(&input, &cfg, synthetic_path(), &[]);
        check_values(&input, &output).map_err(TestCaseError::fail)?;
        prop_assert!(
            lint_str(&output, synthetic_path(), &cfg, synthetic_base_dir()).is_empty(),
            "width {width}, {style}, {scalar}: {output:?}"
        );
    }

    /// Narrow or delete this property once a preview style exists.
    #[test]
    fn preview_matches_stable_while_no_preview_style_exists(
        document in arb_document_with_properties()
    ) {
        let input = document.render();
        prop_assert_eq!(
            (named_pass("format/preview").format)(&input),
            (named_pass("format/default").format)(&input)
        );
    }

    #[test]
    fn reindent_places_each_line_or_leaves_its_document(
        document in arb_document_with_properties(),
        width in 1usize..=4,
    ) {
        check_reindent(&document.render(), width).map_err(TestCaseError::fail)?;
    }

    #[test]
    fn a_file_consistent_at_a_width_keeps_it_without_an_indent_width(
        document in arb_document_with_properties(),
        width in 2u8..=8,
    ) {
        check_detection(&document.render(), width).map_err(TestCaseError::fail)?;
    }

    #[test]
    fn inferred_width_matches_candidate_scoring(input in arb_nested_mapping()) {
        let cfg = YamlLintConfig::default();
        prop_assert_eq!(file_indent_width(&cfg, &input), scored_indent_width(&input));
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

/// `line` past its indentation and the `-`, `?` and `:` indicators leading it, with the
/// indicators: what closing the gaps after them leaves alone.
fn past_indicators(line: &str) -> (String, &str) {
    let mut indicators = String::new();
    let mut rest = line.trim_start();
    while let Some(after) = rest
        .strip_prefix(['-', '?', ':'])
        .filter(|after| after.is_empty() || after.starts_with(' '))
    {
        indicators.push_str(&rest[..1]);
        rest = after.trim_start();
    }
    (indicators, rest)
}

fn lines(text: &str) -> Vec<&str> {
    text.split("\r\n")
        .flat_map(|part| part.split(['\r', '\n']))
        .collect()
}

/// Re-indent's own guarantee on top of the pipeline's: only leading spaces and the gaps
/// after indicators change, a refused document keeps its bytes, and every other line is
/// where `indentation` expects. An input granit cannot parse is left to the
/// parse-preservation check.
fn check_reindent(input: &str, width: usize) -> Result<(), String> {
    if representation(input).is_none() {
        return Ok(());
    }
    let cfg = indentation::Config::new(
        SpacesSetting::Fixed(width),
        IndentSequencesSetting::True,
        false,
    );
    let out = indentation::reindent(input, &cfg);
    let (before, after) = (lines(input), lines(&out.text));
    let refused = |line: usize| out.refused.iter().any(|doc| doc.lines.contains(&line));
    if before.len() != after.len() {
        return Err(format!(
            "surgical: width {width}, {input:?} -> {:?}",
            out.text
        ));
    }
    for (line, (old, new)) in (1..).zip(before.iter().zip(&after)) {
        if past_indicators(old) != past_indicators(new) || (refused(line) && old != new)
        {
            return Err(format!(
                "surgical: line {line}, width {width}, {input:?} -> {:?}",
                out.text
            ));
        }
    }
    match indentation::check(&out.text, &cfg)
        .into_iter()
        .find(|hit| !refused(hit.line))
    {
        Some(hit) => Err(format!(
            "completeness: {hit:?}, width {width}, {input:?} -> {:?}",
            out.text
        )),
        None => Ok(()),
    }
}

fn reindent_target(width: u8) -> indentation::Config {
    indentation::Config::new(
        SpacesSetting::Fixed(usize::from(width)),
        IndentSequencesSetting::True,
        false,
    )
    .with_dash_on_own_line(false)
}

/// One-level nested mappings, mostly at one shared width, the shape width inference decides.
fn arb_nested_mapping() -> impl Strategy<Value = String> {
    let entry = (
        prop_oneof![4 => Just(None), 1 => (1usize..=10).prop_map(Some)],
        "[a-z_]{1,3}",
        prop::sample::select(
            &[
                "v",
                "'q'",
                "[a,b]",
                "v  # c",
                "v  # ryl disable-line rule:indentation",
                "[open",
            ][..],
        ),
    );
    (
        1usize..=10,
        prop::collection::vec(prop::collection::vec(entry, 1..4), 1..5),
    )
        .prop_map(|(width, sections)| {
            let mut text = String::new();
            for (index, entries) in sections.iter().enumerate() {
                text += &format!("k{index}:\n");
                for (own, key, value) in entries {
                    text += &format!(
                        "{}{key}: {value}\n",
                        " ".repeat(own.unwrap_or(width))
                    );
                }
            }
            text
        })
}

fn scored_indent_width(input: &str) -> u8 {
    if indentation::reindent(input, &reindent_target(2)).text == input {
        return 2;
    }
    let before: Vec<&str> = input.split_inclusive('\n').collect();
    let scores: Vec<(u8, usize)> = (2..=8)
        .map(|width| {
            let mut text = indentation::reindent(input, &reindent_target(width)).text;
            let mut previous = input.to_string();
            for _ in 0..ryl::fix::FIX_PIPELINE_MAX_PASSES {
                if text == previous {
                    break;
                }
                let next = indentation::reindent(&text, &reindent_target(width)).text;
                previous = std::mem::replace(&mut text, next);
            }
            let after: Vec<&str> = text.split_inclusive('\n').collect();
            let changed = TextDiff::from_slices(&before, &after)
                .ops()
                .iter()
                .filter(|op| op.tag() != DiffTag::Equal)
                .map(|op| op.old_range().len().max(op.new_range().len()))
                .sum();
            (width, changed)
        })
        .collect();
    let fewest = scores.iter().map(|&(_, count)| count).min();
    let winners: Vec<_> = scores
        .iter()
        .filter(|&&(_, count)| Some(count) == fewest)
        .collect();
    match winners[..] {
        [&(width, _)] => width,
        _ => 2,
    }
}

#[test]
fn uniform_nested_mapping_keeps_observed_width() {
    let input = "first:\n    key: value\n    other: [a,b]\nsecond:\n    key: value\n";
    let cfg = YamlLintConfig::default();
    assert_eq!(scored_indent_width(input), 4);
    assert_eq!(file_indent_width(&cfg, input), 4);
}

#[test]
fn inferred_width_falls_back_for_refusals_and_large_indents() {
    let cfg = YamlLintConfig::default();
    for input in [
        "a:\n    b: value  # ryl disable-line rule:indentation\n".to_string(),
        format!("a:\n{}b: value\n", " ".repeat(256)),
        "a:\n    b: [unclosed\n".to_string(),
    ] {
        assert_eq!(file_indent_width(&cfg, &input), scored_indent_width(&input));
    }
}

/// `input` re-indented as `ryl format` would at `width`, to a fixed point since a dash join
/// can enable another, is left alone by a format with no `indent-width`.
fn check_detection(input: &str, width: u8) -> Result<(), String> {
    if representation(input).is_none() {
        return Ok(());
    }
    let mut text = input.to_string();
    for _ in 0..8 {
        let consistent = indentation::reindent(&text, &reindent_target(width));
        if !consistent.refused.is_empty() {
            return Ok(());
        }
        if consistent.text == text {
            break;
        }
        text = consistent.text;
    }
    let consistent = indentation::Config::new(
        SpacesSetting::Consistent,
        IndentSequencesSetting::True,
        false,
    );
    let width_probe = format!(
        "{text}\n---\nprobe:\n{}child: value\n",
        " ".repeat(usize::from(width))
    );
    if !indentation::check(&width_probe, &consistent).is_empty() {
        return Ok(());
    }
    let detected = file_indent_width(&YamlLintConfig::default(), &text);
    if indentation::reindent(&text, &reindent_target(detected)).text != text {
        return Err(format!(
            "detected {detected}, consistent at {width}: {text:?}"
        ));
    }
    Ok(())
}

#[test]
fn mixed_width_flow_conversion_keeps_consistent_indent_lint_clean() {
    for width in 3..=8 {
        for (style, value) in [("mapping", "{d: e}"), ("sequence", "[d, e]")] {
            for scalar in ['>', '|'] {
                let input = format!(
                    "a:\n- v: {scalar}\n{}x\n  ? x\n  : y\nb:\n- c: {value}\n",
                    " ".repeat(width + 2)
                );
                let cfg = YamlLintConfig::from_toml_str(&format!(
                    "[format]\n{style}-style = 'block'\nindent-sequences = false\n\
                     [lint.rules.indentation]\nindent-sequences = false\n"
                ))
                .unwrap();
                assert_eq!(file_indent_width(&cfg, &input), width as u8);
                let output =
                    ryl::format::format_str(&input, &cfg, synthetic_path(), &[]);
                check_values(&input, &output).unwrap();
                if width == 3 && style == "mapping" && scalar == '>' {
                    assert_eq!(output, input);
                    assert_consistent_indent_refusal(&output, &cfg);
                }
                assert!(
                    lint_str(&output, synthetic_path(), &cfg, synthetic_base_dir())
                        .is_empty(),
                    "width {width}, {style}, {scalar}: {output:?}"
                );
            }
        }
    }
}

#[test]
fn detected_width_refuses_dash_split_that_would_fail_consistent_indent() {
    let input =
        "a:\n   - !!map\n     ? a\n     : [b]\n   -  c: [d]\n      ? e\n      : [f]\n";
    let cfg = YamlLintConfig::from_toml_str(
        "[format]\nsequence-style = 'flow'\nmapping-style = 'flow'\n\
         indent-sequences = false\ndash-on-own-line = true\n\
         [lint.rules.indentation]\nindent-sequences = false\n\
         [lint.rules.hyphens]\nmax-spaces-after = 1\ndash-on-own-line = true\n",
    )
    .unwrap();
    let (formatted, notices) = ryl::fix::rewrite_str(
        input,
        &cfg,
        synthetic_path(),
        synthetic_base_dir(),
        ryl::config::SourceKind::Yaml,
        ryl::fix::Rewrite::Format,
    );
    let output = formatted.as_deref().unwrap_or(input);
    check_values(input, output).unwrap();
    let before = lint_str(input, synthetic_path(), &cfg, synthetic_base_dir());
    let problems = lint_str(output, synthetic_path(), &cfg, synthetic_base_dir());
    assert!(problems.iter().all(|problem| {
        consistency::refused(input, output, problem, &notices, &before)
    }));
}

#[test]
fn clean_document_reports_a_detected_width_refusal() {
    let mut input =
        "a:\n- !!map\n  ? a\n  : [b]\n- c: [d]\n  ? e\n  : [f]\n".to_string();
    for index in 1..=6 {
        input.push_str(&format!("x{index}: >\n        value\n"));
    }
    let cfg = YamlLintConfig::from_toml_str(
        "[format]\nindent-sequences = false\ndash-on-own-line = true\n\
         [lint.rules.indentation]\nindent-sequences = false\n",
    )
    .unwrap();
    assert!(lint_str(&input, synthetic_path(), &cfg, synthetic_base_dir()).is_empty());
    let (formatted, notices) = ryl::fix::rewrite_str(
        &input,
        &cfg,
        synthetic_path(),
        synthetic_base_dir(),
        ryl::config::SourceKind::Yaml,
        ryl::fix::Rewrite::Format,
    );
    assert!(formatted.is_none());
    assert!(notices.iter().any(|notice| {
        notice.line == 1 && notice.message == "cannot re-indent this document safely"
    }));
}

#[test]
fn refused_dash_split_reports_nested_colon_gap() {
    let input = "!!str :\n  - a: b #a\n    ? a\n    :  -   b: {r_o: c}\n           ha_r: {d: e} # z\n";
    let cfg = YamlLintConfig::from_toml_str(
        "[format]\nindent-sequences = false\ndash-on-own-line = true\n\
         [lint.rules.indentation]\nindent-sequences = false\n\
         [lint.rules.colons]\nmax-spaces-after = 1\n",
    )
    .unwrap();
    let (formatted, notices) = ryl::fix::rewrite_str(
        input,
        &cfg,
        synthetic_path(),
        synthetic_base_dir(),
        ryl::config::SourceKind::Yaml,
        ryl::fix::Rewrite::Format,
    );
    let output = formatted.as_deref().unwrap_or(input);
    check_values(input, output).unwrap();
    let before = lint_str(input, synthetic_path(), &cfg, synthetic_base_dir());
    let problems = lint_str(output, synthetic_path(), &cfg, synthetic_base_dir());
    assert!(problems.iter().all(|problem| {
        consistency::refused(input, output, problem, &notices, &before)
    }));
}

fn assert_consistent_indent_refusal(output: &str, cfg: &YamlLintConfig) {
    assert!(
        ryl::format::refusals(output, cfg)
            .iter()
            .any(|problem| problem.message.contains("fail consistent indentation"))
    );
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
        ("%YAML 1.1\n---\nk: 'no'\n", "\nk: 'no'\n"),
        ("%YAML 1.1\n---\nk: '0b101'\n", "\nk: '0b101'\n"),
    ];
    for pass_name in ["fix/best-practice", "format/default"] {
        let pass = named_pass(pass_name);
        for (input, expected) in cases {
            check_pass(pass, input).unwrap_or_else(|violation| panic!("{violation}"));
            let output = (pass.format)(input);
            assert!(
                format!("\n{output}").contains(expected),
                "pass '{pass_name}' on {input:?} must emit {expected:?}, got {output:?}"
            );
        }
    }
}

/// The formatter keeps quotes any YAML 1.1 reader needs, so its output means the same to
/// PyYAML and Docker Compose.
#[test]
fn format_ladder_keeps_quotes_a_yaml_1_1_reader_needs_whatever_the_directive() {
    let pass = named_pass("format/default");
    let kept = [
        "no",
        "Yes",
        "ON",
        "off",
        "0b101",
        "1_000",
        "1:20",
        "190:20:30.15",
        "2001-12-14",
        "<<",
        "=",
        "y",
        "N",
    ];
    for prelude in ["", "%YAML 1.2\n---\n", "%YAML 1.1\n---\n"] {
        for value in kept {
            for (input, expected) in [
                (
                    format!("{prelude}k: \"{value}\"\n"),
                    format!("\nk: '{value}'\n"),
                ),
                (
                    format!("{prelude}\"{value}\": 1\n"),
                    format!("\n'{value}': 1\n"),
                ),
            ] {
                let output = (pass.format)(&input);
                assert!(
                    format!("\n{output}").contains(&expected),
                    "{input:?} must emit {expected:?}, got {output:?}"
                );
            }
        }
        for value in ["_", "._", "1.2.3"] {
            let output = (pass.format)(&format!("{prelude}k: '{value}'\n"));
            assert!(
                format!("\n{output}").contains(&format!("\nk: {value}\n")),
                "'{value}' is a string to every reader, got {output:?}"
            );
        }
    }
}

/// Under `%YAML 1.1` the ladder drops quotes only a YAML 1.2 reader needs, as
/// `quoted-strings` does, so formatted output passes that rule.
#[test]
fn format_ladder_follows_a_declared_yaml_1_1_as_quoted_strings_does() {
    for (pass_name, quote_type) in [
        ("format/default", "single"),
        ("format/quote-double", "double"),
    ] {
        let pass = named_pass(pass_name);
        let cfg = YamlLintConfig::from_yaml_str(&format!(
            "rules:\n  quoted-strings:\n    quote-type: {quote_type}\n    \
             required: only-when-needed\n    check-keys: true\n"
        ))
        .expect("quoted-strings config parses");
        for value in ["1e3", "-1E+3", "008", "+.5", "-.5"] {
            let input = format!("%YAML 1.1\n---\nk: \"{value}\"\n\"{value}\": 1\n");
            check_pass(pass, &input).unwrap_or_else(|violation| panic!("{violation}"));
            let output = (pass.format)(&input);
            assert!(
                format!("\n{output}").contains(&format!("\nk: {value}\n{value}: 1\n")),
                "pass '{pass_name}' must unquote {value:?} under YAML 1.1, got {output:?}"
            );
            let problems =
                lint_str(&output, synthetic_path(), &cfg, synthetic_base_dir());
            assert!(problems.is_empty(), "{output:?} fails lint: {problems:?}");
            let output = (pass.format)(&format!("k: \"{value}\"\n"));
            assert!(
                !format!("\n{output}").contains(&format!("\nk: {value}\n")),
                "{value:?} is not a string to a YAML 1.2 reader, got {output:?}"
            );
        }
    }
}

#[test]
fn zero_config_indentation_is_what_the_yamllint_default_preset_asks_for() {
    let input = "k:\n    - a\n    - b:\n         c: 1\nm:\n     n: 2\n";
    let output = (named_pass("format/default").format)(input);
    assert_ne!(output, input, "the input must need re-indenting");
    let problems = lint_str(
        &output,
        synthetic_path(),
        &profile_anchor_configs()[0],
        synthetic_base_dir(),
    );
    assert!(problems.is_empty(), "{output:?}: {problems:?}");
}

#[test]
fn quote_ladder_converts_physical_line_breaks_without_changing_folding() {
    let pass = named_pass("format/default");
    for (input, expected) in [
        ("a: \"#h0w\n\n    #t5988\"\n", "a: '#h0w\n\n    #t5988'\n"),
        ("a: \"#first\n    last\"\n", "a: '#first\n    last'\n"),
        (
            "a: \"#first\n\n\n    last\"\n",
            "a: '#first\n\n\n    last'\n",
        ),
        (
            "a: \"#say \\\"hi\\\"\n\n    it's\"\n",
            "a: '#say \"hi\"\n\n    it''s'\n",
        ),
        (
            "? \"#key\n\n    tail\"\n: value\n",
            "? '#key\n\n    tail'\n: value\n",
        ),
        (
            "a: \"#first\\n\n    last\"\n",
            "a: \"#first\\n\n    last\"\n",
        ),
        ("a: \"#first\\\n    last\"\n", "a: \"#first\\\n    last\"\n"),
    ] {
        for newline in ["\n", "\r\n", "\r"] {
            let input = input.replace('\n', newline);
            check_pass(pass, &input).unwrap_or_else(|violation| panic!("{violation}"));
            assert_eq!((pass.format)(&input), expected, "{input:?}");
        }
    }
}

#[test]
fn quote_ladder_escalates_to_double_only_for_escapes() {
    let pass = named_pass("format/default");
    for (input, expected) in [
        ("k: \"line\\n\"\n", "\nk: \"line\\n\"\n"),
        ("k: \"tab\\there\"\n", "\nk: \"tab\\there\"\n"),
        ("k: \"say \\\"hi\\\": x\"\n", "\nk: 'say \"hi\": x'\n"),
    ] {
        check_pass(pass, input).unwrap_or_else(|violation| panic!("{violation}"));
        let output = (pass.format)(input);
        assert!(
            format!("\n{output}").contains(expected),
            "{input:?} must emit {expected:?}, got {output:?}"
        );
    }
}

#[test]
fn quote_ladder_avoids_escapes() {
    for (input, single, double) in [
        ("k: \"it's: x\"\n", "k: \"it's: x\"\n", "k: \"it's: x\"\n"),
        ("k: 'it''s: x'\n", "k: \"it's: x\"\n", "k: \"it's: x\"\n"),
        (
            "k: 'say \"hi\": x'\n",
            "k: 'say \"hi\": x'\n",
            "k: 'say \"hi\": x'\n",
        ),
        (
            "k: \"say \\\"hi\\\": x\"\n",
            "k: 'say \"hi\": x'\n",
            "k: 'say \"hi\": x'\n",
        ),
        (
            "k: \"it's \\\"x\\\": y\"\n",
            "k: 'it''s \"x\": y'\n",
            "k: 'it''s \"x\": y'\n",
        ),
        (
            "k: \"back\\\\slash: x\"\n",
            "k: \"back\\\\slash: x\"\n",
            "k: \"back\\\\slash: x\"\n",
        ),
        (
            "k: 'back\\slash: x'\n",
            "k: 'back\\slash: x'\n",
            "k: \"back\\\\slash: x\"\n",
        ),
        (
            "k: 'it''s \\ x: y'\n",
            "k: 'it''s \\ x: y'\n",
            "k: \"it's \\\\ x: y\"\n",
        ),
        ("k: \"a: b\"\n", "k: 'a: b'\n", "k: \"a: b\"\n"),
        (
            "k: 'it''s\n\n  x'\n",
            "k: 'it''s\n\n  x'\n",
            "k: \"it's\\nx\"\n",
        ),
    ] {
        for (pass_name, expected) in
            [("format/default", single), ("format/quote-double", double)]
        {
            let pass = named_pass(pass_name);
            check_pass(pass, input).unwrap_or_else(|violation| panic!("{violation}"));
            let output = (pass.format)(input);
            assert!(
                output.ends_with(expected),
                "{pass_name}: {input:?} must emit {expected:?}, got {output:?}"
            );
        }
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
            format!("\n{output}").contains(expected),
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
fn every_pass_trims_collection_lines_after_block_scalars() {
    for input in [
        "a:\n  - |\n  # a \t\n",
        "a:\n  a: |\n  b: b \t\n",
        "- a: |\n    café \t\n  b: b \t\n",
        "a:\n  a: >\n    café \t\n  b: b \t\n",
        "a:\n  a: |2\n     \n  b: b \t\n",
        "a:\n  - |\n    café \t\n  - b \t\n",
    ] {
        for pass in format_passes() {
            check_pass(pass, input).unwrap();
            let output = (pass.format)(input);
            assert!(
                !output.lines().last().unwrap().ends_with([' ', '\t']),
                "{} left trailing whitespace: {output:?}",
                pass.name
            );
        }
    }
}

#[test]
fn every_pass_preserves_header_only_block_scalars() {
    for header in ["|", ">", "|-", ">-", "|+", ">+", "|2", ">2-", "|+2"] {
        for suffix in ["", " \t", "  # note \t"] {
            let input = format!("a: {header}{suffix}");
            for pass in format_passes() {
                check_pass(pass, &input).unwrap();
            }
        }
    }
}

#[test]
fn every_pass_preserves_unterminated_keep_scalar_break() {
    let input = "- |+\n   ";
    assert!(matches!(
        representation(input).as_deref(),
        Some([_, _, representation::Node::Scalar {
            value: representation::Value::Core(ryl::yaml_dom::ScalarOwned::String(value)),
            ..
        }, ..]) if value == "\n"
    ));
    for input in [input, "a: |+\r\n    ", "a: >+2\n  ", "a: |+\n  x\n  "] {
        check_pass(named_pass("format/default"), input).unwrap();
        for pass in format_passes() {
            check_pass(pass, input).unwrap();
        }
    }
    let cfg = YamlLintConfig::from_toml_str("indent-width = 2\n[format]\n").unwrap();
    let input = "a:\n   b: c\n---\nz: |+\n   ";
    let output = ryl::format::format_str(input, &cfg, synthetic_path(), &[]);
    assert_eq!(output, "a:\n  b: c\n---\nz: |+\n\n");
    check_values(input, &output).unwrap();
}

#[test]
fn keep_scalar_eof_rewrite_preserves_reference_values() {
    for (input, expected, reference, loader) in [
        ("a: |+\n   ", "a: |+\n\n", "\n", ""),
        ("a: >+\n   ", "a: >+\n\n", "\n\n", ""),
        ("a: |+2\n  ", "a: |+2\n\n", "\n", ""),
        ("a: >2+\n  ", "a: >2+\n\n", "\n\n", ""),
        ("a: |+\n  x\n  ", "a: |+\n  x\n\n", "x\n\n", "x\n"),
        ("a: >+\n  x\n  ", "a: >+\n  x\n\n", "x\n\n\n", "x\n"),
    ] {
        assert_ne!(reference, loader, "the parser groups diverge on {input:?}");
        assert_eq!(
            ryl::rules::trailing_spaces::fix(input).as_deref(),
            Some(expected)
        );
        assert_eq!(
            (named_pass("format/default").format)(input),
            expected,
            "reference parser value {reference:?} for {input:?}"
        );
        check_values(input, expected).unwrap();
    }
}

#[test]
fn clip_and_strip_scalar_eof_spaces_remain_fixable() {
    for header in ["|", ">", "|-", ">-", "|2", ">2-"] {
        let input = format!("a: {header}\n  ");
        assert_eq!(
            ryl::rules::trailing_spaces::fix(&input),
            Some(format!("a: {header}\n")),
            "{input:?}"
        );
    }
}

#[test]
fn an_empty_file_needs_no_keep_scalar_eof_rewrite() {
    let cfg = YamlLintConfig::from_toml_str("[format]\n").unwrap();
    assert_eq!(ryl::format::format_str("", &cfg, synthetic_path(), &[]), "");
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
        ("a: |+\r\n    ", "a: |+\r\n"),
        ("a: |2+\n   ", "a: |2+\n"),
        ("- |+\n   ", "- |+\n"),
        ("a: |2+\n  |\n   ", "a: |2+\n  |\n"),
        ("a: |+\n  x\n  ", "a: |+\n  y\n"),
        ("a: >1\n\n\n# tail\n", "a: |1\n\n\n# tail\n"),
        (
            "outer:\n  a: |-1\n   x\n\n\n     ",
            "outer:\n  a: >-1\n   x\n\n\n     ",
        ),
    ] {
        assert_ne!(
            representation(left),
            representation(right),
            "{left:?} and {right:?} must differ"
        );
    }
}

#[test]
fn nested_explicit_indent_uses_document_depth_in_value_oracle() {
    let input = "a:\n  |2\n    x\n  ";
    let trimmed = "a:\n  |2\n    x\n";
    assert_eq!(representation(input), representation(trimmed));
    assert_ne!(representation("|2\n  x\n  "), representation("|2\n  x\n"));
}

#[test]
fn terminal_spaces_only_block_lines_follow_reference_values() {
    for (before, after, index, before_value, after_value) in [
        ("|\n  x\n  ", "|\n  x\n", 0, "x\n", "x\n"),
        (
            "- >\r\n    x\r\n    \r\n    ",
            "- >\r\n    x\r\n    \r\n",
            0,
            "x\n",
            "x\n",
        ),
        (
            "key: |-\n    x\n    \n    ",
            "key: |-\n    x\n    \n",
            1,
            "x",
            "x",
        ),
        (
            "key: >-\r\n    x\r\n    ",
            "key: >-\r\n    x\r\n",
            1,
            "x",
            "x",
        ),
        ("- |+\n    x\n    ", "- |+\n    x\n", 0, "x\n\n", "x\n"),
        (
            ">+\n  x\n  \n  ",
            ">+\n  x\n  \n",
            0,
            "x\n\n\n\n\n\n",
            "x\n\n\n",
        ),
        ("|2\n  x\n  ", "|2\n  x\n", 0, " x\n \n", " x\n"),
        (
            "- >-2\r\n    x\r\n    \r\n    ",
            "- >-2\r\n    x\r\n    \r\n",
            0,
            "  x\n  \n  ",
            "  x\n  ",
        ),
        (
            "key: |+2\n    x\n    ",
            "key: |+2\n    x\n",
            1,
            "  x\n  \n",
            "  x\n",
        ),
        ("- |+\n   ", "- |+\n", 0, "\n", ""),
    ] {
        if before_value == after_value {
            assert_eq!(
                consistency::scalar_value(before, index),
                Some(noyalib::Value::from(before_value)),
                "reference value before: {before:?}"
            );
            assert_eq!(
                consistency::scalar_value(after, index),
                Some(noyalib::Value::from(after_value)),
                "reference value after: {after:?}"
            );
        }
        assert_eq!(
            check_values(before, after).is_ok(),
            before_value == after_value,
            "oracle verdict: {before:?} -> {after:?}"
        );
    }
}

#[test]
fn keep_scalar_before_document_end_follows_reference_parser() {
    for newline in ["\n", "\r\n"] {
        let input = format!("'#hash': |+{newline}{newline}    ");
        let expected = "'#hash': |+\n\n\n...\n";
        let cfg =
            YamlLintConfig::from_toml_str("[format]\ndocument-end = 'add'\n").unwrap();
        assert_eq!(
            ryl::format::format_str(&input, &cfg, synthetic_path(), &[]),
            expected
        );
        check_values(&input, expected).unwrap();
        check_yaml_1_1_preserved(&input, expected).unwrap();
        assert!(check_values(&input, "'#hash': |+\n\n\n\n...\n").is_err());
    }
    assert!(check_values("a: |+\n  x\n    ", "a: |+\n  x\n\n").is_err());
}

#[derive(serde::Deserialize)]
struct ReferenceTerminalBlockPair {
    before: String,
    after: String,
    same: bool,
    before_value: String,
    after_value: String,
}

#[test]
fn reference_parser_terminal_block_eof_matrix() {
    // LF verdicts were checked with yaml-parser --events; CRLF and CR use equivalent breaks.
    fn block_value(nodes: &[representation::Node]) -> Option<&str> {
        nodes.iter().rev().find_map(|node| match node {
            representation::Node::Scalar {
                value: representation::Value::Core(ryl::yaml_dom::ScalarOwned::String(value)),
                ..
            } => Some(value.as_str()),
            _ => None,
        })
    }
    let mut checked = 0;
    for line in include_str!("fixtures/reference_terminal_block_eof.jsonl").lines() {
        let pair: ReferenceTerminalBlockPair = serde_json::from_str(line).unwrap();
        for ending in ["\n", "\r\n", "\r"] {
            let before = pair.before.replace('\n', ending);
            let after = pair.after.replace('\n', ending);
            let before_nodes = representation(&before).unwrap();
            let after_nodes = representation(&after).unwrap();
            assert_eq!(
                block_value(&before_nodes),
                Some(pair.before_value.as_str()),
                "{before:?}"
            );
            assert_eq!(
                block_value(&after_nodes),
                Some(pair.after_value.as_str()),
                "{after:?}"
            );
            assert_eq!(
                check_values(&before, &after).is_ok(),
                pair.same,
                "{before:?} -> {after:?}"
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 2_631);
}

#[test]
fn bare_cr_terminal_spaces_do_not_mask_a_real_value_change() {
    let same_value = "a: |\n  a\n";
    let changed_value = "a: |\n  b\n";

    for before in ["a: |\r  a\r  ", "a: |\r\n  a\r  ", "a: |\n  a\r  "] {
        check_values(before, same_value).unwrap();
        check_yaml_1_1_preserved(before, same_value).unwrap();
        assert!(check_values(before, changed_value).is_err());
        check_pass(named_pass("fix/yamllint-default"), before).unwrap();
    }
}

#[test]
fn cr_only_keep_scalar_eof_preserves_two_breaks() {
    let before = "a:\r  - |+\r\r      ";
    let after = "---\na:\n- |+\n\n\n";
    check_values(before, after).unwrap();
    check_yaml_1_1_preserved(before, after).unwrap();
}

#[test]
fn blank_only_clip_scalar_loses_a_reference_break_when_trimmed() {
    assert!(check_values("a: |\n\n\n", "a: |\n").is_err());
}

#[test]
fn tagged_sequence_scalar_uses_its_content_indent() {
    let before = "a:\n  -  !!str |1 \n   #m9he9o8   \n";
    let after = "a:\n  - !!str |1\n   #m9he9o8   \n...\n";
    for ending in ["\n", "\r\n", "\r"] {
        check_values(&before.replace('\n', ending), &after.replace('\n', ending))
            .unwrap();
    }
}

#[test]
fn folded_interior_blank_line_matches_one_literal_break() {
    let folded = "a: >\n  x\n\n  y\n";
    let changed = "a: |\n  x\n\n  y\n";
    let same = "a: |\n  x\n  y\n";
    for ending in ["\n", "\r\n", "\r"] {
        let folded = folded.replace('\n', ending);
        assert!(check_values(&folded, &changed.replace('\n', ending)).is_err());
        check_values(&folded, &same.replace('\n', ending)).unwrap();
    }
}

#[test]
fn folded_terminal_blank_run_matches_reference() {
    for chomp in ["-", "", "+"] {
        for blanks in 0..=3 {
            let suffix = format!("{}     ", "\n".repeat(blanks));
            let literal = format!("outer:\n  a: |{chomp}1\n   x\n{suffix}");
            let folded = format!("outer:\n  a: >{chomp}1\n   x\n{suffix}");
            for ending in ["\n", "\r\n", "\r"] {
                assert_eq!(
                    check_values(
                        &literal.replace('\n', ending),
                        &folded.replace('\n', ending)
                    )
                    .is_ok(),
                    blanks < 2,
                    "{literal:?} -> {folded:?} with {ending:?}"
                );
            }
        }
    }
    check_values(">\n  x\n\n\n\n  ", ">\n  x\n").unwrap();
    check_values("a: >\n  x\n   y\n\n\n   ", "a: |\n  x\n   y\n\n\n   ").unwrap();
    for pass in format_passes() {
        check_pass(pass, "a: >-\n  x\n\n\n  ").unwrap();
    }
    assert!(
        check_values(
            "outer:\n  a: |-1\n   x\n\n\n     \n",
            "outer:\n  a: >-1\n   x\n\n\n     \n",
        )
        .is_err()
    );
}

#[test]
fn folded_scalar_ignores_outdented_terminal_spaces() {
    let input = "outer:\n  a: >-1\n   x\n\n\n ";
    let cfg = YamlLintConfig::from_toml_str("[format]").unwrap();
    let formatted = ryl::format::format_str(input, &cfg, synthetic_path(), &[]);
    assert_eq!(formatted, "outer:\n  a: >-1\n   x\n");
    check_values(input, &formatted).unwrap();
}

#[test]
fn terminal_spaces_after_a_block_scalar_are_outside_the_correction() {
    let terminal_value = |source, loaded| {
        let (span, style) = representation::scalar_tokens(source).pop().unwrap();
        representation::reference_terminal_block_value(
            source, &span, style, loaded, false, false,
        )
    };
    for before in ["a: |\n  x\n#tail\n  ", "a: |\n  x\n...\n  "] {
        assert_eq!(
            terminal_value(before, ""),
            None,
            "a block scalar ending before EOF must not authorize correction: {before:?}"
        );
    }
    assert_eq!(terminal_value("|\n  x\n  ", "x\n"), Some("x\n".to_owned()));
    assert_eq!(terminal_value("- |+\n   ", ""), Some("\n".to_owned()));
    assert!(check_values("- |+\n   ", "- |+\n").is_err());
}

#[test]
fn a_pass_unquoting_a_yaml_1_1_boolean_fails_only_the_yaml_1_1_check() {
    let input = "k: 'no'\n";
    let unquote = |s: &str| s.replace("'no'", "no");
    check_invariants(&unquote, input).expect("`no` is the same string under YAML 1.2");
    let violation = check_yaml_1_1_values(&unquote, input)
        .expect_err("`no` is a boolean to a YAML 1.1 reader");
    assert!(
        violation.starts_with("yaml-1.1 value-preservation"),
        "{violation}"
    );
    check_yaml_1_1_values(&|s: &str| s.replace("'x'", "x"), "k: 'x'\n")
        .expect("`x` is a string to every reader");
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
        ("a: |+\r\n    ", "a: |+\r\n\r\n"),
        ("a: |-\n  ", "a: |-\n"),
        ("a: >+\n\n  ", "a: >+\n\n\n"),
        ("a: |+\n  x\n  ", "a: |+\n  x\n\n"),
    ] {
        assert_eq!(
            representation(left),
            representation(right),
            "{left:?} and {right:?} must be equal"
        );
    }
}

#[test]
fn a_trailing_comment_keeps_its_node_on_the_key_line() {
    assert_eq!(
        annotations("k0: aaa aaa #\n"),
        annotations("k0: aaa\n  aaa  #\n")
    );
    assert_eq!(
        annotations("k: [a, b]  # c\nj: 1\n"),
        annotations("k:  # c\n  - a\n  - b\nj: 1\n")
    );
    assert_ne!(
        annotations("k: [a, b]  # c\nj: 1\n"),
        annotations("k:\n  - a\n  - b  # c\nj: 1\n")
    );
}

#[test]
fn a_dash_line_comment_survives_sequence_and_mapping_layout_changes() {
    for (toml, input, expected) in [
        (
            "[format]\nindent-sequences = false\n",
            "k:\n  - # c\n    a: b\n",
            "k:\n-  # c\n    a: b\n",
        ),
        (
            "[format]\nmapping-style = 'flow'\n",
            "- a:\n    b: c\n- # c\n",
            "- a: {b: c}\n-  # c\n",
        ),
    ] {
        let cfg = YamlLintConfig::from_toml_str(toml).expect("format config loads");
        let output = ryl::format::format_str(input, &cfg, synthetic_path(), &[]);
        assert_eq!(output, expected);
        check_preserved(input, &output).expect("comment stays on its sequence item");
    }
}

#[test]
fn a_dash_line_comment_moved_to_another_item_is_detected() {
    let input = "- # c\n  a: b\n- d: e\n";
    let moved = "- a: b\n-  # c\n  d: e\n";
    assert_ne!(annotations(input), annotations(moved));
    assert!(check_annotations(input, moved).is_err());
}

#[test]
fn an_empty_block_scalar_leaves_the_next_line_s_comment_to_its_node() {
    for (spread, joined) in [
        ("a: |  # note", "a: |  # note\n...\n"),
        ("|  # note", "|  # note\n...\n"),
        (
            "k:\n- |\n-\n  a: {x: 1}  #c\n",
            "k:\n  - |\n  - a: {x: 1}  # c\n",
        ),
        ("- >-\n-\n  a  # c\n", "- >-\n- a  # c\n"),
    ] {
        assert_eq!(annotations(spread), annotations(joined), "{joined:?}");
    }
    assert_ne!(
        annotations("- |  # c\n- a\n"),
        annotations("- |\n- a  # c\n")
    );
}

#[test]
fn a_deliberately_broken_pass_fails_the_suite() {
    let input = "%YAML 1.1\n---\n# lead\na: &x 'no'  # note\nb: *x\nc: 1\n";
    type Broken = fn(&str) -> String;
    let broken: [(&str, Broken, &str); 8] = [
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
            |s| s.replace("  # note\nb: *x", "\nb: *x  # note"),
            "moves an inline comment to the next line",
        ),
        (
            "comment/anchor fidelity",
            |s| s.replace("&x", "&y").replace("*x", "*y"),
            "renames an anchor",
        ),
    ];
    let broken_spacing: [(&str, (&str, Broken, &str)); 2] = [
        (
            "k:\n    s: |2\n          lead\n        x\n",
            (
                "value-preservation",
                |s| s.replace("    s: |2", "  s: |2"),
                "re-indents a key but not its block scalar's body",
            ),
        ),
        (
            "-   a:\n    b: 1\n",
            (
                "value-preservation",
                |s| s.replace("-   ", "- "),
                "collapses a dash's spaces",
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

/// The corpus gate's verdict on one real-world file before and after `ryl format`:
/// `ok`, the violated invariant, `unparsed` when the original does not parse (the script
/// then requires identical bytes), or `unreadable` when either side is not UTF-8. A value
/// change that also breaks comments or anchors reports the latter, since the script may
/// waive a reviewed value change.
fn corpus_verdict(before: &[u8], after: &[u8]) -> String {
    let (Ok(before), Ok(after)) = (str::from_utf8(before), str::from_utf8(after))
    else {
        return "unreadable".to_string();
    };
    if representation(before).is_none() {
        return "unparsed".to_string();
    }
    let Err(violation) = check_preserved(before, after) else {
        return "ok".to_string();
    };
    let invariant = violation.split(':').next().unwrap_or_default();
    if invariant == "value-preservation" && annotations(before) != annotations(after) {
        return "comment/anchor fidelity".to_string();
    }
    invariant.to_string()
}

#[test]
#[ignore = "driven by scripts/formatter_corpus_check.py over cloned real-world repos"]
fn corpus_pairs_keep_the_guarantee() {
    let pairs = std::env::var("RYL_CORPUS_PAIRS").expect("RYL_CORPUS_PAIRS is set");
    let verdicts =
        std::env::var("RYL_CORPUS_VERDICTS").expect("RYL_CORPUS_VERDICTS is set");
    let pairs = std::fs::read_to_string(pairs).expect("read the pairs file");
    let lines: String = pairs
        .lines()
        .map(|pair| {
            let (before, after) = pair.split_once('\t').expect("a tab-separated pair");
            let read = |path| std::fs::read(path).expect("read a corpus file");
            format!("{}\n", corpus_verdict(&read(before), &read(after)))
        })
        .collect();
    std::fs::write(verdicts, lines).expect("write the verdicts file");
}

#[test]
fn corpus_verdict_names_the_broken_invariant() {
    for (before, after, verdict) in [
        ("a: 'x'  #c\n", "---\na: x  # c\n", "ok"),
        ("###c\na: 1\n", "### c\na: 1\n", "ok"),
        ("## c\na: 1\n", "# c\na: 1\n", "comment/anchor fidelity"),
        ("a: 1\n", "a: '1'\n", "value-preservation"),
        ("a: 1  # c\n", "a: '1'\n", "comment/anchor fidelity"),
        ("a: 1\n", "a: [\n", "parse-preservation"),
        ("a: 1  # c\n", "a: 1\n", "comment/anchor fidelity"),
        ("a: [\n", "a: [\n", "unparsed"),
    ] {
        assert_eq!(
            corpus_verdict(before.as_bytes(), after.as_bytes()),
            verdict,
            "{before:?} -> {after:?}"
        );
    }
    assert_eq!(corpus_verdict(b"a: \xff\n", b"a: 1\n"), "unreadable");
}

#[test]
fn bom_document_marker_keeps_g11_indentation_consistent() {
    let input = "a: a\n...\n\u{feff}\na: 1";
    let cfg = YamlLintConfig::from_toml_str(
        "line-length = 1\n[format]\nquote-style = 'single'\nline-ending = 'lf'\n\
         document-start = 'add'\ndocument-end = 'preserve'\nfold-long-lines = false\n\
         brace-spacing = false\npreview = false\ncomment-spacing = 1\n\
         comment-starting-space = 'preserve'\nmax-blank-lines = 0\n\
         sequence-style = 'preserve'\nmapping-style = 'preserve'\n\
         indent-sequences = false\ndash-on-own-line = false\n\
         [lint.rules.indentation]\nspaces = 'consistent'\n",
    )
    .unwrap();
    let formatted = ryl::format::format_str(input, &cfg, synthetic_path(), &[]);
    assert_eq!(
        indentation::check(&formatted, &indentation::Config::resolve(&cfg)),
        []
    );
    check_invariants(
        &|input| ryl::format::format_str(input, &cfg, synthetic_path(), &[]),
        input,
    )
    .unwrap();
    for pass in format_passes() {
        check_pass(pass, input).unwrap();
    }
}

#[test]
fn g11_content_whitespace_exempts_only_whitespace_repairs_inside_content() {
    use consistency::content_whitespace;
    for (input, rule, line, column) in [
        ("a: >\n  a \n", "trailing-spaces", 2, 4),
        ("a: | # header\n  a \nb: b\n", "trailing-spaces", 2, 4),
        ("a: |+\n  a\n\n", "empty-lines", 3, 1),
        ("a: |\n  café \n", "trailing-spaces", 2, 7),
        ("a: |2\r\n    \r\n\r\n # after\r\n", "trailing-spaces", 2, 1),
        ("a: |2\r\n    \r\n # after\r\n", "trailing-spaces", 2, 1),
        (
            "---\na: |\n  a \n...\n\u{feff}---\n\na: 1\n...\n",
            "trailing-spaces",
            3,
            4,
        ),
    ] {
        assert!(
            content_whitespace(input, Some(rule), line, column),
            "{input:?}"
        );
    }
    for (input, rule, line, column) in [
        ("a: |\r\n \r\nFALSE: a\r\n", "trailing-spaces", 2, 1),
        ("a: |\n\n", "empty-lines", 2, 1),
        ("a: |3\n   \n   a\n", "trailing-spaces", 2, 1),
        ("a: |2\n  \n", "trailing-spaces", 2, 1),
        ("a: |-\n  a", "new-line-at-end-of-file", 2, 4),
        ("a: |\n  a", "new-line-at-end-of-file", 2, 4),
        ("a: |+\n  a", "new-line-at-end-of-file", 2, 4),
        ("a: >2\n  a", "new-line-at-end-of-file", 2, 4),
        ("a: >-2\n  a", "new-line-at-end-of-file", 2, 4),
        ("a: |\n  a\n", "new-line-at-end-of-file", 2, 4),
        ("a: |\n  a", "new-line-at-end-of-file", 2, 1),
        ("a: |\n  a\n\n", "empty-lines", 3, 1),
        ("a: |-\r\n  a\r\n\r\n", "empty-lines", 3, 1),
        ("a: |\n  a\n\n...\n", "empty-lines", 3, 1),
    ] {
        assert!(
            !content_whitespace(input, Some(rule), line, column),
            "{input:?}"
        );
    }
    for (input, rule, line, column) in [
        ("a: plain ", "trailing-spaces", 1, 9),
        ("true ", "trailing-spaces", 1, 6),
        ("null ", "trailing-spaces", 1, 6),
        ("011 ", "trailing-spaces", 1, 5),
        ("a: | \n  a\n", "trailing-spaces", 1, 5),
        ("a: |\n  a\nb: b \n", "trailing-spaces", 3, 5),
        ("a: |\n  a\n# after\n\n", "empty-lines", 4, 1),
        ("a: |\n  a\n...\n\n", "empty-lines", 4, 1),
        ("a: |\n  a \n", "indentation", 2, 4),
        ("a: |\n  a \n", "trailing-spaces", 2, 3),
    ] {
        assert!(
            !content_whitespace(input, Some(rule), line, column),
            "{input:?}"
        );
    }
}

#[test]
fn g11_does_not_report_value_bearing_spaces_after_formatting_header_and_comment() {
    let input = "a: |2  \r\n    \r\n # after \t";
    let cfg = YamlLintConfig::from_toml_str(&consistency::agreeing_lint(
        "[format]\nline-ending = 'cr-lf'\n",
    ))
    .unwrap();
    let (formatted, _) = ryl::fix::rewrite_str(
        input,
        &cfg,
        synthetic_path(),
        synthetic_base_dir(),
        ryl::config::SourceKind::Yaml,
        ryl::fix::Rewrite::Format,
    );
    let output = formatted.unwrap_or_else(|| input.to_owned());
    assert_eq!(output, "a: |2\r\n    \r\n # after\r\n");
    let problems = lint_str(&output, synthetic_path(), &cfg, synthetic_base_dir());
    assert!(!problems.iter().any(|problem| {
        problem.rule == Some("trailing-spaces") && problem.line == 2
    }));
}

#[test]
fn g11_refusal_exempts_only_the_reported_line_and_concern() {
    let table = "[format]\ndash-on-own-line = true\ndocument-start = 'add'\n";
    let cfg =
        YamlLintConfig::from_toml_str(&consistency::agreeing_lint(table)).unwrap();
    let input = "a:\n  - a: a #a\n";
    let (formatted, refusals) = ryl::fix::rewrite_str(
        input,
        &cfg,
        synthetic_path(),
        synthetic_base_dir(),
        ryl::config::SourceKind::Yaml,
        ryl::fix::Rewrite::Format,
    );
    let output = formatted.unwrap_or_else(|| input.to_owned());
    let problems = lint_str(&output, synthetic_path(), &cfg, synthetic_base_dir());
    assert!(!problems.is_empty());
    assert!(
        problems.iter().all(|problem| consistency::refused(
            input,
            &output,
            problem,
            &refusals,
            &lint_str(input, synthetic_path(), &cfg, synthetic_base_dir())
        )),
        "{problems:?}: {refusals:?}"
    );
    let mut other = problems[0].clone();
    other.line += 1;
    assert!(!consistency::refused(
        input,
        &output,
        &other,
        &refusals,
        &[]
    ));
    other.line = problems[0].line;
    other.rule = Some("colons");
    assert!(!consistency::refused(
        input,
        &output,
        &other,
        &refusals,
        &[]
    ));
    assert!(!consistency::refused(
        input,
        &output,
        &problems[0],
        &[],
        &[]
    ));
}

#[test]
fn g11_document_refusal_exempts_an_unchanged_indentation_violation() {
    let table = "indent-width = 155\n[format]\n[lint.rules.indentation]\n";
    let cfg = YamlLintConfig::from_toml_str(table).unwrap();
    let input = "a:\n  - ? a\n    :  - |1\n       #";
    let (formatted, refusals) = ryl::fix::rewrite_str(
        input,
        &cfg,
        synthetic_path(),
        synthetic_base_dir(),
        ryl::config::SourceKind::Yaml,
        ryl::fix::Rewrite::Format,
    );
    let output = formatted.unwrap_or_else(|| input.to_owned());
    let before = lint_str(input, synthetic_path(), &cfg, synthetic_base_dir());
    let after = lint_str(&output, synthetic_path(), &cfg, synthetic_base_dir());
    let problem = after
        .iter()
        .find(|problem| problem.rule == Some("indentation") && problem.line == 2)
        .unwrap();
    assert!(before.iter().any(|old| old == problem));
    assert!(consistency::refused(
        input, &output, problem, &refusals, &before
    ));
}

#[test]
fn g11_document_refusal_does_not_exempt_a_new_violation() {
    let cfg = YamlLintConfig::from_toml_str(
        "[format]\nsequence-style = 'block'\nindent-sequences = false\n\
         [lint.rules.indentation]\nindent-sequences = false\n",
    )
    .unwrap();
    let input = "a: [a]\nb: >\n   x\n  #\n";
    let output = "a:\n  - a\nb: >\n   x\n  #\n";
    let before = lint_str(input, synthetic_path(), &cfg, synthetic_base_dir());
    let after = lint_str(output, synthetic_path(), &cfg, synthetic_base_dir());
    let problem = after
        .iter()
        .find(|problem| problem.rule == Some("indentation") && problem.line == 2)
        .unwrap();
    let mut refusal = problem.clone();
    refusal.line = 1;
    refusal.column = 1;
    refusal.message = "cannot re-indent this document safely".to_owned();
    assert!(
        !before
            .iter()
            .any(|old| old.rule == problem.rule && old.message == problem.message)
    );
    assert!(!consistency::refused(
        input,
        output,
        problem,
        &[refusal],
        &before
    ));
}

#[test]
fn g11_document_refusal_does_not_exempt_a_new_duplicate_line_violation() {
    let cfg = YamlLintConfig::from_toml_str(
        "[format]\nsequence-style = 'block'\nindent-sequences = false\n\
         [lint.rules.indentation]\nindent-sequences = false\n",
    )
    .unwrap();
    let input = "a: [a]\nb:\n  - a\nc: >\n   x\n  #\n";
    let output = "a:\n  - a\nb:\n  - a\nc: >\n   x\n  #\n";
    let before = lint_str(input, synthetic_path(), &cfg, synthetic_base_dir());
    let after = lint_str(output, synthetic_path(), &cfg, synthetic_base_dir());
    let new_problem = after
        .iter()
        .find(|problem| problem.rule == Some("indentation") && problem.line == 2)
        .unwrap();
    assert!(before.iter().any(|problem| {
        problem.rule == new_problem.rule
            && problem.message == new_problem.message
            && problem.column == new_problem.column
            && problem.line == 3
    }));
    let refusals = vec![ryl::lint::LintProblem {
        line: 1,
        column: 1,
        level: ryl::lint::Severity::Error,
        message: "cannot re-indent this document safely".to_owned(),
        rule: Some("indentation"),
    }];
    assert!(!consistency::refused(
        input,
        output,
        new_problem,
        &refusals,
        &before
    ));
}

#[test]
fn g11_scalar_exemptions_require_a_value_change_outside_block_content() {
    use consistency::content_whitespace;
    for (input, rule, line, column) in [
        ("a:\n- a\n\n a", "empty-lines", 3, 1),
        ("a: 'a\n\n  b'\n", "empty-lines", 2, 1),
        ("a: \"a\n\n  b\"\n", "empty-lines", 2, 1),
        ("a:\n  b: |2\n    café \n", "trailing-spaces", 3, 9),
        ("bad: !!bool tRUE\na: |\n  a \n", "trailing-spaces", 3, 4),
    ] {
        assert!(
            content_whitespace(input, Some(rule), line, column),
            "{input:?}"
        );
    }
    for (input, rule, line, column) in [
        ("a: 'a\n  b  \n  c'\n", "trailing-spaces", 2, 4),
        ("a: |-\n  a", "document-end", 2, 1),
        ("a: |\n  a", "document-end", 2, 1),
        ("a: |+\n  a", "document-end", 2, 1),
        ("a: >2\n  a", "document-end", 2, 1),
        ("a: >-2\n  a", "document-end", 2, 1),
        ("a: |\n  a\n", "document-end", 2, 1),
        ("a: plain", "document-end", 1, 1),
        ("a: 'a\n\n b'\n", "indentation", 2, 1),
    ] {
        assert!(
            !content_whitespace(input, Some(rule), line, column),
            "{input:?}"
        );
    }
}

#[test]
fn review_refusal_hides_unrelated_bracket_spacing() {
    let table = consistency::agreeing_lint("[format]\nsequence-style = 'block'\n");
    let cfg = YamlLintConfig::from_toml_str(&table).unwrap();
    let output = "[ x ]: a\n";
    let refusals = ryl::format::refusals(output, &cfg);
    let problems = lint_str(output, synthetic_path(), &cfg, synthetic_base_dir());
    let spacing: Vec<_> = problems
        .iter()
        .filter(|problem| problem.message == "too many spaces inside brackets")
        .collect();
    assert_eq!(spacing.len(), 2);
    assert!(spacing.iter().all(|problem| !consistency::refused(
        output,
        output,
        problem,
        &refusals,
        &[]
    )));
    let mut same_position = spacing[0].clone();
    same_position.column = refusals[0].column;
    assert!(!consistency::refused(
        output,
        output,
        &same_position,
        &refusals,
        &[]
    ));
    same_position.message = ryl::rules::brackets::FORBID_MESSAGE.to_owned();
    assert!(consistency::refused(
        output,
        output,
        &same_position,
        &refusals,
        &[]
    ));
    same_position.column += 1;
    assert!(!consistency::refused(
        output,
        output,
        &same_position,
        &refusals,
        &[]
    ));
    let formatted = ryl::format::format_str(output, &cfg, synthetic_path(), &[]);
    assert_eq!(formatted, "[x]: a\n");
    check_values(output, &formatted).unwrap();
}
