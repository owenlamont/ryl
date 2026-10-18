use std::path::Path;
use std::process::Command;

use granit_parser::Parser;
use ryl::config::{SourceKind, YamlLintConfig};
use ryl::fix::{Rewrite, rewrite_str};

const INPUT: &str = "---\nflow: [a,\nb,\nc]\n";
const OUTPUT: &str = "---\nflow: [a,\n       b,\n       c]\n";

#[test]
fn lint_enforces_strict_indentation_for_flow_scalars_and_properties() {
    let cfg = YamlLintConfig::from_toml_str("[lint.rules.document-start]").unwrap();
    for input in [
        INPUT,
        "key: [first\nsecond]\n",
        "key: \"first\\\nsecond\"\n",
        "? &anchor\nkey\n: value\n",
        "missing: *unknown\nflow: [a,\nb]\n",
    ] {
        let diagnostics =
            ryl::lint::lint_str(input, Path::new("case.yaml"), &cfg, Path::new("."));
        assert_eq!(diagnostics.len(), 1, "{input:?}: {diagnostics:?}");
        assert!(diagnostics[0].rule.is_none(), "{input:?}: {diagnostics:?}");
        assert!(
            diagnostics[0].message.contains("syntax"),
            "{input:?}: {diagnostics:?}"
        );
    }
}

#[test]
fn check_rejects_9c9n_until_format_or_fix_repairs_indentation() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("9C9N.yaml");
    for mode in [vec!["format"], vec!["check", "--fix"]] {
        std::fs::write(&file, INPUT).unwrap();
        let check = Command::new(env!("CARGO_BIN_EXE_ryl"))
            .args(["check", "-d", "{rules: {document-start: enable}}"])
            .arg(&file)
            .output()
            .unwrap();
        assert_eq!(check.status.code(), Some(1));
        assert!(
            String::from_utf8_lossy(&check.stdout).contains("syntax")
                || String::from_utf8_lossy(&check.stderr).contains("syntax"),
            "{check:?}"
        );
        let preview = Command::new(env!("CARGO_BIN_EXE_ryl"))
            .args([
                "format",
                "--check",
                "-d",
                "{rules: {document-start: enable}}",
            ])
            .arg(&file)
            .output()
            .unwrap();
        assert_eq!(preview.status.code(), Some(1), "{preview:?}");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), INPUT);
        assert!(
            String::from_utf8_lossy(&preview.stderr).contains("repaired indentation")
        );
        let fixed = Command::new(env!("CARGO_BIN_EXE_ryl"))
            .args(mode)
            .args(["-d", "{rules: {document-start: enable}}"])
            .arg(&file)
            .output()
            .unwrap();
        assert!(fixed.status.success(), "{fixed:?}");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), OUTPUT);
        assert!(
            String::from_utf8_lossy(&fixed.stderr)
                .contains("input was YAML the spec rejects")
        );
    }
}

#[test]
fn check_explains_repairable_invalid_indentation() {
    let temp = tempfile::tempdir().unwrap();
    let file = temp.path().join("input.yaml");
    std::fs::write(&file, "---\nkey:\n  args: [\n    \"a\",\n  ]\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ryl"))
        .args([
            "check",
            "-f",
            "standard",
            "-d",
            "[lint.rules.document-start]",
        ])
        .arg(&file)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        format!(
            "{}\n  5:3       error    syntax error: invalid indentation (syntax); help: ryl check --fix or ryl format can repair the indentation\n\n",
            file.display()
        )
    );
}

#[test]
fn refused_indentation_repair_has_no_help() {
    let cfg = YamlLintConfig::from_toml_str("[lint.rules.document-start]").unwrap();
    let diagnostics = ryl::lint::lint_str(
        "a: 'a\r\n'\r\n\r\n",
        Path::new("case.yaml"),
        &cfg,
        Path::new("."),
    );
    assert_eq!(diagnostics.len(), 1);
    assert!(!diagnostics[0].message.contains("help:"), "{diagnostics:?}");
}

#[test]
fn lint_hint_names_only_the_available_repair() {
    let cfg = YamlLintConfig::from_toml_str(
        "[lint]\nfixable = []\n[lint.rules.document-start]\n",
    )
    .unwrap();
    let diagnostics =
        ryl::lint::lint_str(INPUT, Path::new("case.yaml"), &cfg, Path::new("."));
    assert_eq!(diagnostics.len(), 1);
    assert!(
        diagnostics[0]
            .message
            .ends_with("help: ryl format can repair the indentation"),
        "{diagnostics:?}"
    );
}

#[test]
fn check_diff_previews_a_strictly_valid_repair() {
    let cfg = YamlLintConfig::from_toml_str("[lint.rules.document-start]").unwrap();
    let result = ryl::fix::diff_outcome(
        INPUT,
        &cfg,
        Path::new("case.yaml"),
        Path::new("."),
        SourceKind::Yaml,
        Rewrite::Fix,
    );
    assert!(result.changed);
    assert!(result.diff.unwrap().contains("+       b,"));
    assert_eq!(result.skipped.len(), 1);
    assert!(result.skipped[0].message.contains("repaired indentation"));
}

#[test]
fn repair_refuses_disabled_indentation_or_changed_events() {
    for (input, config, rewrite) in [
        (
            "# ryl disable rule:indentation\nflow: [a,\nb]\n",
            "[format]",
            Rewrite::Format,
        ),
        (
            "z: [a,\nb]\na: 1\n",
            "[lint.rules.key-ordering]",
            Rewrite::Fix,
        ),
        ("flow: [a,\nb\n", "[format]", Rewrite::Format),
        (
            "flow: [a,\nb]\n",
            "[lint.rules.document-start]\n[[lint.per-line-ignores]]\nregex = '^b'\nrules = ['ALL']\n",
            Rewrite::Fix,
        ),
        (
            INPUT,
            "[lint]\nfixable = []\n[lint.rules.document-start]\n",
            Rewrite::Fix,
        ),
        (
            INPUT,
            "[lint]\nfixable = ['comments']\n[lint.rules.document-start]\n",
            Rewrite::Fix,
        ),
        (
            INPUT,
            "[lint.rules.indentation]\nignore = ['case.yaml']\n",
            Rewrite::Fix,
        ),
        (
            INPUT,
            "[lint.rules.document-start]\n[lint.per-file-ignores]\n'case.yaml' = ['ALL']\n",
            Rewrite::Fix,
        ),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("config.toml");
        std::fs::write(&config_path, config).unwrap();
        let context = ryl::config::discover_config(
            &[],
            &ryl::config::Overrides {
                config_file: Some(config_path),
                config_data: None,
            },
        )
        .unwrap();
        let path = temp.path().join("case.yaml");
        let (output, notices) = rewrite_str(
            input,
            &context.config,
            &path,
            &context.base_dir,
            SourceKind::Yaml,
            rewrite,
        );
        assert!(output.is_none(), "{input:?}: {output:?}");
        assert!(
            notices
                .iter()
                .any(|notice| notice.message.contains("syntax")),
            "{notices:?}"
        );
    }
}

#[test]
fn an_under_indented_closing_quote_is_refused_without_changing_values() {
    let input = "a: 'a\r\n'\r\n\r\n";
    let cfg = YamlLintConfig::from_yaml_str("extends: default").unwrap();
    for rewrite in [Rewrite::Fix, Rewrite::Format] {
        let (output, notices) = rewrite_str(
            input,
            &cfg,
            Path::new("case.yaml"),
            Path::new("."),
            SourceKind::Yaml,
            rewrite,
        );
        assert!(output.is_none());
        assert_eq!(notices.len(), 1);
        assert!(notices[0].message.contains("syntax"));
    }
}

#[test]
fn formatting_keeps_flow_closers_deeper_than_the_enclosing_block() {
    let cfg = YamlLintConfig::from_toml_str("[format]").unwrap();
    for (input, expected) in [
        (" \na: [a\n ]", "a: [a\n ]\n"),
        ("a: [a\n]\n", "a: [a\n ]\n"),
        ("- key: {a: b\n}\n", "- key: {a: b\n   }\n"),
        ("[a\n]\n", "[a\n]\n"),
    ] {
        let output = ryl::format::format_str(input, &cfg, Path::new("case.yaml"), &[]);
        assert_eq!(output, expected);
        assert!(
            Parser::new_from_str_with_options(
                &output,
                granit_parser::options! { strict_indentation: true }
            )
            .collect::<Result<Vec<_>, _>>()
            .is_ok()
        );
    }
}

#[test]
fn lint_repair_uses_the_configured_formatter_indent_width() {
    let input = "outer:\n    flow: [a,\nb]\n";
    let cfg = YamlLintConfig::from_toml_str(
        "indent-width = 4\n[lint.rules.document-start]\n",
    )
    .unwrap();
    let output =
        ryl::fix::apply_safe_fixes(input, &cfg, Path::new("case.yaml"), Path::new("."));
    assert_eq!(output, "---\nouter:\n    flow: [a,\n           b]\n");
}

#[test]
fn lint_fix_selection_does_not_gate_formatter_repair() {
    let cfg =
        YamlLintConfig::from_toml_str("[lint]\nfixable = []\n[format]\n").unwrap();
    assert_eq!(
        ryl::format::format_str(INPUT, &cfg, Path::new("case.yaml"), &[]),
        OUTPUT
    );
}

#[test]
fn markdown_repair_notices_do_not_report_a_syntax_skip() {
    let input = format!("```yaml\n{INPUT}```\n");
    let cfg = YamlLintConfig::from_toml_str("[format]").unwrap();
    for rewrite in [Rewrite::Fix, Rewrite::Format] {
        let preview = ryl::fix::diff_outcome(
            &input,
            &cfg,
            Path::new("case.md"),
            Path::new("."),
            SourceKind::Markdown,
            rewrite,
        );
        assert!(preview.changed);
        assert_eq!(preview.skipped.len(), 1, "{:?}", preview.skipped);
        assert!(preview.skipped[0].message.contains("repaired indentation"));
        let (output, notices) = rewrite_str(
            &input,
            &cfg,
            Path::new("case.md"),
            Path::new("."),
            SourceKind::Markdown,
            rewrite,
        );
        assert_eq!(output.unwrap(), format!("```yaml\n{OUTPUT}```\n"));
        assert_eq!(notices.len(), 1, "{notices:?}");
        assert!(notices[0].message.contains("repaired indentation"));
    }
}

#[test]
fn repaired_output_preserves_tag_anchor_comment_and_events() {
    let input = "flow: &items !local [a, # keep\nb]\ncopy: *items\n";
    let cfg = YamlLintConfig::from_toml_str("[format]").unwrap();
    let output = ryl::format::format_str(input, &cfg, Path::new("case.yaml"), &[]);
    assert_ne!(output, input);
    let before: Vec<_> = Parser::new_from_str(input)
        .map(|item| item.unwrap().0)
        .collect();
    let after: Vec<_> = Parser::new_from_str_with_options(
        &output,
        granit_parser::options! { strict_indentation: true },
    )
    .map(|item| item.unwrap().0)
    .collect();
    assert_eq!(before, after);
    assert_eq!(
        ryl::format::format_str(&output, &cfg, Path::new("case.yaml"), &[]),
        output
    );
}

#[cfg(feature = "lsp")]
#[test]
fn lsp_diagnoses_and_formats_9c9n() {
    use ryl::lsp::{analysis, encoding::PositionEncoding};
    let cfg = YamlLintConfig::from_toml_str("[lint.rules.document-start]").unwrap();
    let path = Path::new("case.yaml");
    let diagnostics = analysis::diagnostics(
        INPUT,
        path,
        &cfg,
        Path::new("."),
        SourceKind::Yaml,
        PositionEncoding::Utf16,
    );
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].message.contains("syntax"));
    let edit = analysis::rewrite_edit(
        INPUT,
        path,
        &cfg,
        Path::new("."),
        SourceKind::Yaml,
        PositionEncoding::Utf16,
        Rewrite::Format,
    )
    .unwrap();
    assert_eq!(edit.new_text, OUTPUT);
    assert!(
        analysis::diagnostics(
            &edit.new_text,
            path,
            &cfg,
            Path::new("."),
            SourceKind::Yaml,
            PositionEncoding::Utf16
        )
        .is_empty()
    );
    let cfg = YamlLintConfig::from_toml_str("[lint.rules.comments]\n").unwrap();
    assert!(
        analysis::fix_rule_edit(
            "flow: [a, #bad\nb]\n",
            path,
            &cfg,
            Path::new("."),
            SourceKind::Yaml,
            PositionEncoding::Utf16,
            "comments"
        )
        .is_none()
    );
}

#[test]
fn refused_repair_previews_report_only_the_syntax_error() {
    let cfg = YamlLintConfig::from_toml_str("[format]").unwrap();
    let input = "flow: [a,\nb, # ryl disable-line rule:indentation\nc]\n";
    for rewrite in [Rewrite::Fix, Rewrite::Format] {
        let preview = ryl::fix::diff_outcome(
            input,
            &cfg,
            Path::new("case.yaml"),
            Path::new("."),
            SourceKind::Yaml,
            rewrite,
        );
        assert!(!preview.changed);
        assert_eq!(preview.skipped.len(), 1);
        assert!(preview.skipped[0].message.contains("syntax"));
    }
}

#[test]
fn markdown_preview_keeps_empty_and_invalid_regions_beside_a_formatted_region() {
    let cfg = YamlLintConfig::from_toml_str("[format]").unwrap();
    let input = "```yaml\n\n```\n```yaml\nbad: [\n```\n```yaml\nx: 'one'\n```\n";
    let preview = ryl::fix::diff_outcome(
        input,
        &cfg,
        Path::new("case.md"),
        Path::new("."),
        SourceKind::Markdown,
        Rewrite::Format,
    );
    assert!(preview.changed);
    let diff = preview.diff.unwrap();
    assert!(diff.contains("+x: one"));
    assert!(!diff.contains("-bad: ["));
    assert_eq!(preview.skipped.len(), 1);
    assert!(preview.skipped[0].message.contains("syntax"));
}

#[test]
fn per_line_ignores_cannot_leave_a_repair_with_invalid_syntax() {
    let temp = tempfile::tempdir().unwrap();
    let config = temp.path().join("config.toml");
    let file = temp.path().join("case.yaml");
    std::fs::write(
        &config,
        "[lint.rules.document-start]\n[[lint.per-line-ignores]]\nregex = '^b,'\nrules = ['indentation']\n",
    )
    .unwrap();
    std::fs::write(&file, INPUT).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ryl"))
        .args(["check", "--diff", "-c"])
        .arg(config)
        .arg(&file)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(std::fs::read_to_string(file).unwrap(), INPUT);
    assert!(!String::from_utf8_lossy(&output.stdout).contains("@@"));
    assert!(String::from_utf8_lossy(&output.stderr).contains("syntax"));
}
