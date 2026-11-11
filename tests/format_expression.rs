use std::path::Path;

use ryl::config::YamlLintConfig;
use ryl::format::format_str;

pub(super) const ENTRY: &str = r#"
[[format.prose]]
files = ['*.yaml']
path = '$.expression'
break-before = ['&&', '||']
protected-patterns = ['"([^"\\]|\\.)*"', "'([^'\\\\]|\\\\.)*'"]
"#;

fn config(width: u16) -> YamlLintConfig {
    crate::common::format_config::load(&format!(
        "line-length = {width}\n[format]\nprose-wrap = 'always'\nquote-style = 'preserve'\n{ENTRY}"
    ))
}

fn assert_format(input: &str, expected: &str, width: u16) {
    let cfg = config(width);
    let output = format_str(input, &cfg, Path::new("case.yaml"), &[]);
    assert_eq!(output, expected);
    assert_eq!(
        format_str(&output, &cfg, Path::new("case.yaml"), &[]),
        output
    );
    crate::property::reference::check_strict_events(input, &output).unwrap();
}

#[test]
fn reporter_cel_expression_wraps_into_three_whole_clauses() {
    let expected = "expression: >-\n  has(object.spec.securityContext)\n  && object.spec.securityContext.runAsNonRoot == true\n  && object.spec.securityContext.runAsUser >= 1000\n";
    let input = "expression: >-\n  has(object.spec.securityContext) && object.spec.securityContext.runAsNonRoot == true && object.spec.securityContext.runAsUser >= 1000\n";
    assert_format(input, expected, 60);
    assert_format(
        "expression: >-\n  a\n  && b\n",
        "expression: >-\n  a && b\n",
        60,
    );
}

#[test]
fn protected_single_double_and_escaped_literals_keep_their_operators() {
    for text in [
        r#"a == "x && y || z""#,
        r"a == 'x && y || z'",
        r#"a == "x\" && y""#,
        r"a == 'x\' || y'",
        r#"a == 'x && y' + "z || w""#,
        r#"a == "'x && y' || z""#,
    ] {
        let input = format!("expression: >-\n  {text} && next\n");
        let expected = format!("expression: >-\n  {text}\n  && next\n");
        assert_format(&input, &expected, 12);
    }
}

#[test]
fn overlapping_protected_patterns_cover_their_combined_span() {
    let cfg = crate::common::format_config::load(
        "line-length = 12\n[format]\nprose-wrap = 'always'\n[[format.prose]]\nfiles = ['*']\npath = '$.expression'\nbreak-before = ['&&']\nprotected-patterns = ['beta && gamma', 'alpha && beta']\n",
    );
    let input = "expression: >-\n  alpha && beta && gamma && next\n";
    let output = format_str(input, &cfg, Path::new("case.yaml"), &[]);
    assert_eq!(
        output,
        "expression: >-\n  alpha && beta && gamma\n  && next\n"
    );
    assert_eq!(
        format_str(&output, &cfg, Path::new("case.yaml"), &[]),
        output
    );
    crate::property::reference::check_strict_events(input, &output).unwrap();
}

#[test]
fn boundaries_before_and_after_protected_matches_remain_available() {
    assert_format(
        "expression: >-\n  first && \"x || y\" && next\n",
        "expression: >-\n  first\n  && \"x || y\"\n  && next\n",
        12,
    );
}

#[test]
fn unmatched_delimiters_have_only_user_defined_protection() {
    assert_format(
        "expression: >-\n  a == \"open && next\n",
        "expression: >-\n  a == \"open\n  && next\n",
        12,
    );
}

#[test]
fn adjacent_operators_tabs_and_double_spaces_do_not_create_boundaries() {
    for text in ["a&&b||c", "a  && b", "a\t&& b", "a \t&& b"] {
        let input = format!("expression: >-\n  {text}\n");
        assert_format(&input, &input, 4);
    }
}

#[test]
fn overlong_clauses_do_not_fall_back_to_word_wrapping() {
    assert_format(
        "expression: >-\n  a very long comparison == true && another long comparison >= 1000\n",
        "expression: >-\n  a very long comparison == true\n  && another long comparison >= 1000\n",
        12,
    );
}

#[test]
fn plain_and_quoted_styles_keep_values_and_astral_characters() {
    for (input, expected) in [
        (
            "expression: 🦀 == true && next\n",
            "expression: 🦀 == true\n  && next\n",
        ),
        (
            "expression: '🦀 == true && next'\n",
            "expression: '🦀 == true\n  && next'\n",
        ),
        (
            "expression: \"🦀 == true && next\"\n",
            "expression: \"🦀 == true\n  && next\"\n",
        ),
        (
            "expression: \"a == \\\"x && y\\\" && next\"\n",
            "expression: \"a == \\\"x && y\\\"\n  && next\"\n",
        ),
        (
            "expression: 'a == ''x && y'' && next'\n",
            "expression: 'a == ''x && y''\n  && next'\n",
        ),
    ] {
        assert_format(input, expected, 16);
    }
}

#[test]
fn yaml_hex_escapes_are_decoded_before_protected_patterns_match() {
    for quote in [r"\x22", r"\u0022", r"\U00000022"] {
        let input = format!("expression: \"a == {quote}x && y{quote} && next\"\n");
        let expected =
            format!("expression: \"a == {quote}x && y{quote}\n  && next\"\n");
        assert_format(&input, &expected, 16);
    }
    assert_format(
        "expression: \"a \\u0026\\u0026 next\"\n",
        "expression: \"a\n  \\u0026\\u0026 next\"\n",
        12,
    );
}

#[test]
fn literal_flow_and_disabled_scalars_are_excluded() {
    for input in [
        "expression: |\n  alpha && beta\n",
        "expression: {value: alpha && beta}\n",
        "# ryl disable rule:line-length\nexpression: >-\n  alpha && beta\n",
        "# ryl disable rule:line-length\nexpression: \"alpha\\\n  beta && gamma\"\n",
    ] {
        assert_format(input, input, 8);
    }
}

#[test]
fn astral_key_prefix_keeps_byte_positions_aligned() {
    let cfg = crate::common::format_config::load(
        "line-length = 12\n[format]\nprose-wrap = 'always'\n[[format.prose]]\nfiles = ['*']\npath = '$.🦀'\nbreak-before = ['&&']\n",
    );
    let input = "🦀: alpha && beta\n";
    let output = format_str(input, &cfg, Path::new("case.yaml"), &[]);
    assert_eq!(output, "🦀: alpha\n  && beta\n");
    crate::property::reference::check_strict_events(input, &output).unwrap();
}

#[test]
fn crlf_chomping_paragraphs_and_more_indented_content_survive() {
    let cfg = crate::common::format_config::load(&format!(
        "line-length = 12\n[format]\nprose-wrap = 'always'\nline-ending = 'cr-lf'\n{ENTRY}"
    ));
    let input =
        "expression: >+\r\n  alpha && beta\r\n\r\n    literal && untouched\r\n\r\n";
    let output = format_str(input, &cfg, Path::new("case.yaml"), &[]);
    assert_eq!(
        output,
        "expression: >+\r\n  alpha\r\n  && beta\r\n\r\n    literal && untouched\r\n\r\n"
    );
    assert_eq!(
        format_str(&output, &cfg, Path::new("case.yaml"), &[]),
        output
    );
    crate::property::reference::check_strict_events(input, &output).unwrap();
}

#[test]
fn first_matching_file_and_scalar_selector_wins_without_merging() {
    let entries = "[[format.prose]]\nfiles = ['*.yaml', '!skip.yaml']\npath = '$.spec.validations[*].expression'\nbreak-before = ['&&']\n[[format.prose]]\nfiles = ['*.yaml']\npath = '$.spec.*[*].expression'\nbreak-before = ['||']\n";
    let cfg = crate::common::format_config::load(&format!(
        "line-length = 20\n[format]\nprose-wrap = 'always'\n{entries}"
    ));
    let input =
        "spec:\n  validations:\n    - expression: a == true || b == true && next\n";
    let output = format_str(input, &cfg, Path::new("case.yaml"), &[]);
    assert_eq!(
        output,
        "spec:\n  validations:\n    - expression: a == true || b == true\n        && next\n"
    );
    assert_eq!(
        format_str(input, &cfg, Path::new("skip.yaml"), &[]),
        "spec:\n  validations:\n    - expression: a == true\n        || b == true && next\n"
    );
    crate::property::reference::check_strict_events(input, &output).unwrap();
}

#[test]
fn invalid_expression_configuration_is_rejected_on_load() {
    for entry in [
        "files = []\npath = '$'\nbreak-before = ['&&']",
        "files = ['[']\npath = '$'\nbreak-before = ['&&']",
        "files = ['*']\npath = '$..x'\nbreak-before = ['&&']",
        "files = ['*']\npath = '$'\nbreak-before = []",
        "files = ['*']\npath = '$'\nbreak-before = ['']",
        "files = ['*']\npath = '$'\nbreak-before = ['a b']",
        "files = ['*']\npath = '$'\nbreak-before = ['&&']\nprotected-patterns = ['[']",
    ] {
        let error =
            YamlLintConfig::from_toml_str(&format!("[[format.prose]]\n{entry}\n"))
                .unwrap_err();
        assert!(error.contains("format.prose"), "{error}");
    }
    assert!(YamlLintConfig::from_yaml_str("format:\n  prose: []\n").is_err());
    let (code, _, error, _) = crate::common::cli::ryl_on(
        Some(
            "[[format.prose]]\nfiles = ['*']\npath = '$'\nbreak-before = ['&&']\nprotected-patterns = ['[']\n",
        ),
        "a: 1\n",
        &["format"],
    );
    assert_eq!(code, 2, "{error}");
}

#[test]
fn protected_matches_span_paragraphs_and_only_matching_occurrences() {
    let cfg = crate::common::format_config::load(
        "line-length = 12\n[format]\nprose-wrap = 'always'\n[[format.prose]]\nfiles = ['*']\npath = '$.expression'\nbreak-before = ['&&']\nprotected-patterns = ['(?s)BEGIN.*END']\n",
    );
    let input = "expression: >-\n  BEGIN alpha && beta\n\n  alpha && beta END\n\n  alpha && beta\n";
    let output = format_str(input, &cfg, Path::new("case.yaml"), &[]);
    assert_eq!(
        output,
        "expression: >-\n  BEGIN alpha && beta\n\n  alpha && beta END\n\n  alpha\n  && beta\n"
    );
    assert_eq!(
        format_str(&output, &cfg, Path::new("case.yaml"), &[]),
        output
    );
    crate::property::reference::check_strict_events(input, &output).unwrap();
}

#[test]
fn empty_protected_spans_do_not_hide_boundaries_but_protected_tokens_do() {
    let input = "expression: >-\n  alpha && beta\n";
    for (pattern, expected) in
        [("", "expression: >-\n  alpha\n  && beta\n"), ("&&", input)]
    {
        let cfg = crate::common::format_config::load(&format!(
            "line-length = 8\n[format]\nprose-wrap = 'always'\n[[format.prose]]\nfiles = ['*']\npath = '$.expression'\nbreak-before = ['&&']\nprotected-patterns = ['{pattern}']\n"
        ));
        let output = format_str(input, &cfg, Path::new("case.yaml"), &[]);
        assert_eq!(output, expected);
        crate::property::reference::check_strict_events(input, &output).unwrap();
    }
}

#[test]
fn root_selector_wraps_a_clause_shorter_than_the_continuation_indent() {
    let cfg = crate::common::format_config::load(
        "line-length = 4\n[format]\nprose-wrap = 'always'\n[[format.prose]]\nfiles = ['*']\npath = '$'\nbreak-before = ['&&']\n",
    );
    let input = "a && b\n";
    let output = format_str(input, &cfg, Path::new("case.yaml"), &[]);
    assert_eq!(output, "a\n  && b\n");
    assert_eq!(
        format_str(&output, &cfg, Path::new("case.yaml"), &[]),
        output
    );
    crate::property::reference::check_strict_events(input, &output).unwrap();
}

#[test]
fn check_fix_and_other_prose_policies_ignore_expression_preferences() {
    let input = "expression: >-\n  alpha == true && beta == true\n";
    let cfg = crate::common::format_config::load(&format!(
        "line-length = 12\n[format]\nprose-wrap = 'always'\n{ENTRY}[lint.rules]\ntrailing-spaces = 'enable'\n"
    ));
    let dirty = format!("{input}other: value   \n");
    assert_eq!(
        ryl::fix::apply_safe_fixes(
            &dirty,
            &cfg,
            Path::new("case.yaml"),
            Path::new(".")
        ),
        format!("{input}other: value\n")
    );
    for policy in ["preserve", "never"] {
        let base = format!("line-length = 12\n[format]\nprose-wrap = '{policy}'\n");
        let plain = YamlLintConfig::from_toml_str(&base).unwrap();
        let configured = crate::common::format_config::load(&format!("{base}{ENTRY}"));
        assert_eq!(
            format_str(input, &plain, Path::new("case.yaml"), &[]),
            format_str(input, &configured, Path::new("case.yaml"), &[])
        );
    }
}
