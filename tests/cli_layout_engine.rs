use std::fs;
use std::process::Command;

use serde_json::Value;
mod common;

fn format_with_statistics(input: &str, config: &str) -> (String, Value) {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("layout.yaml");
    let configuration = directory.path().join("ryl.toml");
    let statistics = directory.path().join("statistics.jsonl");
    fs::write(&file, input).unwrap();
    fs::write(&configuration, config).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_ryl"))
        .args(["format", "--no-cache", "-c"])
        .arg(&configuration)
        .arg(&file)
        .env("RYL_LAYOUT_STATS", &statistics)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let rows: Vec<Value> = fs::read_to_string(statistics)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let row = rows
        .into_iter()
        .find(|row| row["path"] == file.to_str().unwrap())
        .unwrap();
    (fs::read_to_string(file).unwrap(), row)
}

#[test]
fn multiline_quotes_share_layout_and_a_dry_plan() {
    let input = "é:\n    value: 'folded first\n      continuation café' #note\n    necessary: \"**Required unless you provide email**\n      to invite café.\" #note\n    after: x\n";
    let (output, statistics) = format_with_statistics(input, "[format]\n");
    assert_eq!(
        output,
        "é:\n  value: folded first continuation café  # note\n  necessary: '**Required unless you provide email**\n    to invite café.'  # note\n  after: x\n"
    );
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
    for (name, count) in [
        ("analyses", 2),
        ("lexical_scans", 2),
        ("streamed_dry_scans", 1),
        ("planning_invocations", 2),
        ("emissions", 1),
        ("strict_parses", 2),
        ("verification_parses", 1),
        ("rollback", 0),
    ] {
        assert_eq!(statistics[name], count, "{name}: {statistics}");
    }
}

#[test]
fn markers_own_blank_runs_before_empty_line_limits() {
    let config = "[format]\ndocument-start = 'add'\ndocument-end = 'add'\n";
    for (input, expected) in [
        ("\n\na: 1\n", "---\n\n\na: 1\n...\n"),
        ("a: 1\n\n\n", "---\na: 1\n\n\n...\n"),
        ("a: 1\n  \n\nb: 2\n", "---\na: 1\n\n\nb: 2\n...\n"),
    ] {
        let (output, statistics) = format_with_statistics(input, config);
        assert_eq!(output, expected, "{input:?}");
        assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
        assert_eq!(format_with_statistics(&output, config).0, output);
    }
}

#[test]
fn every_indent_width_and_dash_policy_uses_the_layout_engine() {
    let input = "é:\n    seq:\n    - key: 'café'\n      next: [ a,b ] #note\n";
    for width in 1..=4 {
        for sequences in [false, true] {
            for own_line in [false, true] {
                let config = format!(
                    "indent-width = {width}\n[format]\nindent-sequences = {sequences}\ndash-on-own-line = {own_line}\n"
                );
                let (output, statistics) = format_with_statistics(input, &config);
                assert_eq!(
                    statistics["fallback"],
                    Value::Null,
                    "{config}: {statistics}"
                );
                assert!(output.contains("next: [a, b]  # note"));
                assert!(output.contains("key: café"));
                assert_eq!(format_with_statistics(&output, &config).0, output);
            }
        }
    }
}

#[test]
fn conversion_and_suppression_restart_the_ordered_driver() {
    for (input, config, reason, expected) in [
        (
            "é: [ café,世 ]\n",
            "[format]\nsequence-style = 'block'\n",
            "Conversion",
            "é:\n  - café\n  - 世\n",
        ),
        (
            "# ryl disable rule:quoted-strings\né : 'café'\n",
            "[format]\n",
            "Suppression",
            "# ryl disable rule:quoted-strings\né: 'café'\n",
        ),
    ] {
        let (output, statistics) = format_with_statistics(input, config);
        assert_eq!(output, expected);
        assert_eq!(statistics["fallback"], reason, "{statistics}");
        assert_eq!(format_with_statistics(&output, config).0, output);
    }
}

#[test]
fn unchanged_bom_and_endings_need_no_candidate_or_verification() {
    for input in ["café: 世\n", "\u{feff}café: 世\r\n"] {
        let (output, statistics) =
            format_with_statistics(input, "[format]\nline-ending = 'auto'\n");
        assert_eq!(output, input);
        assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
        assert_eq!(statistics["planning_invocations"], 1);
        assert_eq!(statistics["emissions"], 0);
        assert_eq!(statistics["verification_parses"], 0);
    }
}

#[test]
fn structural_gap_previews_keep_indentation_provenance() {
    let (status, _, diagnostics, _) = common::cli::ryl_on(
        Some("[format]\n"),
        "-   name: value\n    next: x\n",
        &["format", "--check", "--output-format", "parsable"],
    );
    assert_eq!(status, 1);
    assert!(diagnostics.contains("(indentation)"), "{diagnostics}");
    assert!(!diagnostics.contains("(hyphens)"), "{diagnostics}");
}

#[test]
fn collapsed_scalar_tail_keeps_trimming_and_preview_provenance() {
    let input = "é: 'first\n  second'  \nnext: café\n";
    let (output, statistics) = format_with_statistics(input, "[format]\n");
    assert_eq!(output, "é: first second\nnext: café\n");
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
    let (status, _, diagnostics, _) = common::cli::ryl_on(
        Some("[format]\n"),
        input,
        &["format", "--check", "--output-format", "parsable"],
    );
    assert_eq!(status, 1, "{diagnostics}");
    assert!(
        diagnostics.contains(":2:10: [error] trailing spaces (trailing-spaces)"),
        "{diagnostics}"
    );
}

#[test]
fn width_one_folds_use_the_projected_implicit_sequence_owner() {
    let input = "- type: parse_json\n- type: exclude_logs\n  match_any:\n  - 'jsonPayload.field: \"pattern\"'\n";
    let (output, statistics) = format_with_statistics(
        input,
        "line-length = 12\nindent-width = 1\n[format]\nfold-long-lines = true\n",
    );
    assert_eq!(
        output,
        "- type: parse_json\n- type: exclude_logs\n  match_any:\n   - 'jsonPayload.field:\n     \"pattern\"'\n"
    );
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
}

#[test]
fn markers_do_not_terminate_a_blank_line_erased_by_indentation() {
    let (output, statistics) = format_with_statistics(
        "  [1, 2, 3]  \n  ",
        "[format]\ndocument-start = 'add'\ndocument-end = 'add'\n",
    );
    assert_eq!(output, "---\n[1, 2, 3]\n...\n");
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
}

#[test]
fn folded_unterminated_blank_tail_retains_ordered_refusal_history() {
    let (output, statistics) = format_with_statistics(
        "  >\n  abc def ghi jkl\n    more\n\n\n  ",
        "line-length = 12\nindent-width = 1\n[format]\nfold-long-lines = true\n",
    );
    assert_eq!(output, ">\n abc def\n ghi jkl\n   more\n");
    assert_eq!(statistics["fallback"], "ScalarInteraction", "{statistics}");
}

#[test]
fn an_erased_unterminated_line_is_not_an_empty_line_edit() {
    let (status, _, diagnostics, _) = common::cli::ryl_on(
        Some("[format]\n"),
        "  [1, 2, 3]\n  ",
        &["format", "--check", "--output-format", "parsable"],
    );
    assert_eq!(status, 1, "{diagnostics}");
    assert!(!diagnostics.contains("(empty-lines)"), "{diagnostics}");
}
