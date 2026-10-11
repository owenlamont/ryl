//! The formatter's guarantee as properties: every pass in `passes::format_passes` must be
//! idempotent, parse-preserving, value-preserving at the representation level (tags,
//! aliases, duplicate keys and entry order included, resolved against the declared YAML
//! version and under YAML 1.1 too), and must keep every comment beside
//! its node and every anchor and alias name. Folding also keeps the two
//! parser-independent properties in `fold`: only lone spaces become line breaks, and
//! every continuation is deeper than its scalar's owner.
//! G11 excludes complementary line-length lint: docs/formatter.md defines a soft target.
//! G11 exempts value-bearing scalar whitespace: the repair (including a final block marker's newline) must change the independently loaded value.
//! G11 exempts diagnostics matching an actual formatter refusal notice in line and concern.

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

use config::{synthetic_base_dir, synthetic_path};
use fold::{arb_fold_document, inserted_breaks, under_indented_break};
use passes::{
    FOLD_TARGETS, FORMAT_OWNED_RULES, FormatPass, fold_alone, format_passes,
    named_pass, yaml_rules_for,
};
use properties::arb_document_with_properties;
use representation::{
    annotations, check_annotations, check_values, check_yaml_1_1_preserved,
    representation,
};

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
        let mut problems = lint_str(&output, synthetic_path(), &cfg, synthetic_base_dir());
        problems.retain(|problem| !consistency::content_whitespace(&output, problem.rule, problem.line, problem.column) && !consistency::refused(problem, &refusals));
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
        problems.retain(|problem| !consistency::content_whitespace(&output, problem.rule, problem.line, problem.column) && !consistency::refused(problem, &refusals));
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
        ("a: |+\r\n    ", "a: |+\r\n    \r\n"),
        ("a: |2+\n   ", "a: |2+\n"),
        ("a: |2+\n  |\n   ", "a: |2+\n  |\n"),
        ("a: |+\n  x\n  ", "a: |+\n  y\n"),
    ] {
        assert_ne!(
            representation(left),
            representation(right),
            "{left:?} and {right:?} must differ"
        );
    }
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
        ("a: |+\r\n    ", "a: |+\r\n"),
        ("a: |-\n  ", "a: |-\n"),
        ("a: >+\n\n  ", "a: >+\n\n"),
        ("a: |+\n  x\n  ", "a: |+\n  x\n"),
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
        ("a: |\n  a", "new-line-at-end-of-file", 2, 4),
        ("a: >\n  a \n", "trailing-spaces", 2, 4),
        ("a: | # header\n  a \nb: b\n", "trailing-spaces", 2, 4),
        ("a: |+\n  a\n\n", "empty-lines", 3, 1),
        ("a: |\n  café \n", "trailing-spaces", 2, 7),
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
        ("a: |-\n  a", "new-line-at-end-of-file", 2, 4),
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
        problems
            .iter()
            .all(|problem| consistency::refused(problem, &refusals)),
        "{problems:?}: {refusals:?}"
    );
    let mut other = problems[0].clone();
    other.line += 1;
    assert!(!consistency::refused(&other, &refusals));
    other.line = problems[0].line;
    other.rule = Some("colons");
    assert!(!consistency::refused(&other, &refusals));
    assert!(!consistency::refused(&problems[0], &[]));
}

#[test]
fn g11_scalar_exemptions_require_a_value_change_outside_block_content() {
    use consistency::content_whitespace;
    for (input, rule, line, column) in [
        ("a:\n- a\n\n a", "empty-lines", 3, 1),
        ("a: 'a\n\n  b'\n", "empty-lines", 2, 1),
        ("a: \"a\n\n  b\"\n", "empty-lines", 2, 1),
        ("a: |\n  a", "document-end", 2, 1),
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
