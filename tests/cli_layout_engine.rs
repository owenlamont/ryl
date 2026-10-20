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
        ("cached_lexical_tapes", 0),
        ("line_table_builds", 2),
        ("token_normalizations", 2),
    ] {
        assert_eq!(statistics[name], count, "{name}: {statistics}");
    }
}

#[test]
fn projected_unicode_endpoints_survive_nested_gaps_and_scalar_collapse() {
    let input =
        "🦀: { é : [ '世',x ] }\r\nnext: ['first\r\n  last', café, { tail : z }]\r\n";
    let (output, statistics) = format_with_statistics(input, "[format]\n");
    assert_eq!(
        output,
        "🦀: {é: [世, x]}\nnext: [first last, café, {tail: z}]\n"
    );
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
    assert_eq!(statistics["line_table_builds"], 2, "{statistics}");
    assert!(statistics["projected_coordinate_queries"].as_u64().unwrap() > 0);
    assert_eq!(format_with_statistics(&output, "[format]\n").0, output);
}

#[test]
fn a_comment_ending_in_a_dash_cannot_join_the_mapping_below() {
    let (output, statistics) =
        format_with_statistics("- # -\n        key: value\n", "[format]\n");
    assert_eq!(output, "-  # -\n  key: value\n");
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
}

#[test]
fn a_directive_keyword_inside_a_scalar_does_not_cache_tokens() {
    let (output, statistics) =
        format_with_statistics("word: 'ryl message'\n", "[format]\n");
    assert_eq!(output, "word: ryl message\n");
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
    assert_eq!(statistics["cached_lexical_tapes"], 0, "{statistics}");
}

#[test]
fn a_dash_at_the_end_of_a_key_does_not_own_the_colon_gap() {
    let (status, _, diagnostics, _) = common::cli::ryl_on(
        Some("[format]\n"),
        "café- : x\n",
        &["format", "--check", "--output-format", "parsable"],
    );
    assert_eq!(status, 1);
    assert!(
        diagnostics.contains(":1:6: [error] too many spaces before colon (colons)"),
        "{diagnostics}"
    );
    assert!(!diagnostics.contains("(hyphens)"), "{diagnostics}");
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

fn assert_indentation_layout(config: &str, expected: &str) {
    let input = "é:\n    seq:\n    - key: 'café'\n      next: [ a,b ] #note\n";
    let (output, statistics) = format_with_statistics(input, config);
    assert_eq!(output, expected);
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
    assert_eq!(format_with_statistics(&output, config).0, output);
}

#[test]
fn width_one_preserves_unindented_sequence_ownership() {
    assert_indentation_layout(
        "indent-width = 1\n[format]\nindent-sequences = false\ndash-on-own-line = false\n",
        "é:\n seq:\n - key: café\n   next: [a, b]  # note\n",
    );
}

#[test]
fn indented_sequences_move_the_mapping_with_the_dash() {
    assert_indentation_layout(
        "indent-width = 2\n[format]\nindent-sequences = true\ndash-on-own-line = false\n",
        "é:\n  seq:\n    - key: café\n      next: [a, b]  # note\n",
    );
}

#[test]
fn standalone_dashes_separate_mapping_indentation_from_sequence_indentation() {
    assert_indentation_layout(
        "indent-width = 4\n[format]\nindent-sequences = true\ndash-on-own-line = true\n",
        "é:\n    seq:\n        -\n            key: café\n            next: [a, b]  # note\n",
    );
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

#[test]
fn quoted_flow_key_gap_conflicts_restart_the_ordered_driver() {
    let (output, statistics) = format_with_statistics(
        "{\"foo\"\n: bar}\n",
        "[format]\nbrace-spacing = true\n",
    );
    assert_eq!(output, "{ foo\n  : bar }\n");
    assert_eq!(statistics["fallback"], "Overlap", "{statistics}");
}

#[test]
fn a_dirty_dry_plan_rolls_back_before_the_ordered_driver() {
    let (output, statistics) =
        format_with_statistics("---\n{ \"foo\"\n  :bar }\n", "[format]\n");
    assert_eq!(output, "---\n{foo\n : bar}\n");
    assert_eq!(statistics["fallback"], "DryEdits", "{statistics}");
    assert_eq!(statistics["rollback"], 1, "{statistics}");
}

#[test]
fn an_unsettled_root_quote_restarts_the_ordered_driver() {
    let (output, statistics) = format_with_statistics("---\n\"foo\"\n", "[format]\n");
    assert_eq!(output, "---\nfoo\n");
    assert_eq!(statistics["fallback"], "unsettled-quote:4", "{statistics}");
}

#[test]
fn empty_flow_scalar_verification_rolls_back_before_the_ordered_driver() {
    let (output, statistics) =
        format_with_statistics("empty:\n-\n- null\nmap: {a:,b: null}\n", "[format]\n");
    assert_eq!(output, "empty:\n-\n- null\nmap: {a:, b: null}\n");
    assert_eq!(statistics["fallback"], "Verification", "{statistics}");
    assert_eq!(statistics["rollback"], 1, "{statistics}");
}

#[test]
fn a_collapsed_root_scalar_folds_through_the_ordered_driver() {
    let (output, statistics) = format_with_statistics(
        "--- \"a\n...x\nb\"\n",
        "[format]\nfold-long-lines = true\n",
    );
    assert_eq!(output, "--- a ...x b\n");
    assert_eq!(statistics["fallback"], "ScalarInteraction", "{statistics}");
}

#[test]
fn statistics_write_failure_keeps_the_verified_output() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("layout.yaml");
    let configuration = directory.path().join("ryl.toml");
    fs::write(&file, "a : b\n").unwrap();
    fs::write(&configuration, "[format]\n").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_ryl"))
        .args(["format", "--no-cache", "-c"])
        .arg(configuration)
        .arg(&file)
        .env("RYL_LAYOUT_STATS", directory.path())
        .output()
        .unwrap();
    assert!(result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("layout statistics:"));
    assert_eq!(fs::read_to_string(file).unwrap(), "a: b\n");
}

fn format_with_skipped_rules(input: &str, config: &str, skip: &[&str]) -> String {
    let cfg = ryl::config::YamlLintConfig::from_toml_str(config).unwrap();
    ryl::format::format_str(input, &cfg, std::path::Path::new("layout.yaml"), skip)
}

#[test]
fn skip_finishing_matches_ordered_formatter() {
    let output = format_with_skipped_rules(
        "a : b  \n\n\n",
        "[format]\n",
        &[
            "comments-indentation",
            "trailing-spaces",
            "empty-lines",
            "new-lines",
        ],
    );
    assert_eq!(output, "a: b  \n\n\n");
}

#[test]
fn skip_final_newline_matches_ordered_formatter() {
    let output = format_with_skipped_rules(
        "a: |+\n  a\n  ",
        "[format]\n",
        &["new-line-at-end-of-file", "new-lines"],
    );
    assert_eq!(output, "a: |+\n  a\n\n");
}

#[test]
fn skip_ending_markers_matches_ordered_formatter() {
    let output = format_with_skipped_rules(
        "a : b\r\n",
        "[format]\ndocument-start='add'\ndocument-end='add'\n",
        &["new-lines"],
    );
    assert_eq!(output, "---\r\na: b\r\n...\r\n");
}

#[test]
fn overlapping_collapses_matches_ordered_formatter() {
    let output = format_with_skipped_rules(
        "a: ['first\n  second', 'third\n  fourth']\n",
        "[format]\n",
        &[],
    );
    assert_eq!(output, "a: [first second, third fourth]\n");
}

#[test]
fn joined_quote_matches_ordered_formatter() {
    let output =
        format_with_skipped_rules("-\n  a: 'first\n    second'\n", "[format]\n", &[]);
    assert_eq!(output, "- a: first second\n");
}

#[test]
fn collapsed_trailing_matches_ordered_formatter() {
    let output = format_with_skipped_rules("a: 'one  \n  two'\n", "[format]\n", &[]);
    assert_eq!(output, "a: one two\n");
}

#[test]
fn underindented_quoted_matches_ordered_formatter() {
    let output = format_with_skipped_rules(
        "a: 'alpha beta\n gamma delta epsilon'\n",
        "indent-width=1\nline-length=12\n[format]\nquote-style='preserve'\nfold-long-lines=true\n",
        &[],
    );
    assert_eq!(output, "a: 'alpha beta\n gamma delta epsilon'\n");
}

#[test]
fn folded_blank_matches_ordered_formatter() {
    let output = format_with_skipped_rules(
        "é: >+\n  alpha beta gamma\n  \n  delta epsilon zeta\n",
        "line-length=12\n[format]\nfold-long-lines=true\n",
        &[],
    );
    assert_eq!(
        output,
        "é: >+\n  alpha beta\n  gamma\n\n  delta\n  epsilon\n  zeta\n"
    );
}

#[test]
fn directive_fold_matches_ordered_formatter() {
    let output = format_with_skipped_rules(
        "é: alpha beta gamma delta # ryl disable-line rule:truthy\n",
        "line-length=12\n[format]\nfold-long-lines=true\n",
        &[],
    );
    assert_eq!(
        output,
        "é: alpha beta gamma delta  # ryl disable-line rule:truthy\n"
    );
}

#[test]
fn bom_fold_matches_ordered_formatter() {
    let output = format_with_skipped_rules(
        "﻿é: alpha beta gamma delta\n",
        "line-length=12\n[format]\nfold-long-lines=true\n",
        &[],
    );
    assert_eq!(output, "﻿é: alpha\n  beta gamma\n  delta\n");
}

#[test]
fn gap_fold_matches_ordered_formatter() {
    let output = format_with_skipped_rules(
        "-   alpha beta gamma delta\n",
        "line-length=12\n[format]\nfold-long-lines=true\n",
        &[],
    );
    assert_eq!(output, "- alpha beta\n  gamma\n  delta\n");
}

#[test]
fn erased_dash_tail_matches_ordered_formatter() {
    let output = format_with_skipped_rules("-   \n  a: 1\n", "[format]\n", &[]);
    assert_eq!(output, "- a: 1\n");
}

#[test]
fn unicode_flow_gap_matches_ordered_formatter() {
    let output =
        format_with_skipped_rules("[😀😀, ?   key: value]\n", "[format]\n", &[]);
    assert_eq!(output, "[😀😀, ? key: value]\n");
}

#[test]
fn unicode_flow_gap_space_matches_ordered_formatter() {
    let output = format_with_skipped_rules(
        "[😀😀, ?   key: value]\n",
        "[format]\nbrace-spacing=true\n",
        &[],
    );
    assert_eq!(output, "[😀😀, ? key: value]\n");
}

#[test]
fn skipped_indentation_retains_prefix() {
    let output =
        format_with_skipped_rules("a:\n    b : c\n", "[format]\n", &["indentation"]);
    assert_eq!(output, "a:\n    b: c\n");
}

#[test]
fn fold_collapsed_scalar_before_comment() {
    let output = format_with_skipped_rules(
        "a: 'alpha\n  beta gamma delta' # tail\n",
        "line-length=12\n[format]\nfold-long-lines=true\n",
        &[],
    );
    assert_eq!(output, "a: alpha\n  beta gamma\n  delta  # tail\n");
}

#[test]
fn fold_after_indentation_closes_a_skipped_hyphen_gap() {
    let output = format_with_skipped_rules(
        "-   alpha beta gamma delta\n",
        "line-length=12\n[format]\nfold-long-lines=true\n",
        &["hyphens"],
    );
    assert_eq!(output, "- alpha beta\n  gamma\n  delta\n");
}

#[test]
fn fold_after_indentation_closes_a_skipped_colon_gap() {
    let output = format_with_skipped_rules(
        "? key\n:   alpha beta gamma delta\n",
        "line-length=12\n[format]\nfold-long-lines=true\n",
        &["colons"],
    );
    assert_eq!(output, "? key\n: alpha beta\n  gamma\n  delta\n");
}

#[test]
fn a_commented_mapping_keeps_its_dash_during_dry_planning() {
    let (output, statistics) =
        format_with_statistics("- a : b # tail\n", "[format]\ndash-on-own-line=true\n");
    assert_eq!(output, "- a: b  # tail\n");
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
    assert_eq!(statistics["streamed_dry_scans"], 1);
}

#[test]
fn an_empty_folded_scalar_does_not_fold_the_next_root_node() {
    let (output, statistics) = format_with_statistics(
        "a: >\nb: alpha beta gamma delta\n",
        "line-length=12\n[format]\nfold-long-lines=true\n",
    );
    assert_eq!(output, "a: >\nb: alpha\n  beta gamma\n  delta\n");
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
}

#[test]
fn a_blank_folded_scalar_does_not_fold_the_next_root_node() {
    let (output, statistics) = format_with_statistics(
        "a: >\n\nb: alpha beta gamma delta\n",
        "line-length=12\n[format]\nfold-long-lines=true\n",
    );
    assert_eq!(output, "a: >\n\nb: alpha\n  beta gamma\n  delta\n");
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
}

#[test]
fn an_empty_flow_string_rolls_back_inconsistent_dry_indentation() {
    let (output, statistics) = format_with_statistics("[\n  \"\",\n]\n", "[format]\n");
    assert_eq!(output, "[\n  '',\n]\n");
    assert_eq!(statistics["fallback"], "Refusal", "{statistics}");
    assert_eq!(statistics["rollback"], 1, "{statistics}");
    assert_eq!(format_with_statistics(&output, "[format]\n").0, output);
}
