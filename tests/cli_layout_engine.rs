use std::fs;
use std::process::Command;

use serde_json::Value;
mod common;

fn format_with_statistics(input: &str, config: &str) -> (String, Value) {
    format_with_workers(input, config, 2)
}

#[test]
fn comment_only_documents_use_layout_without_document_nodes() {
    let (output, statistics) =
        format_with_statistics("#note  \r\n\r\n\r\n", "[format]\n");
    assert_eq!(output, "# note\n");
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
}

#[test]
fn tabbed_comment_prefixes_are_removed_without_indentation_refusals() {
    let (output, statistics) = format_with_statistics("\t#note \n", "[format]\n");
    assert_eq!(output, "# note\n");
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
}

#[test]
fn empty_documents_complete_a_clean_layout_plan() {
    let (output, statistics) = format_with_statistics("", "[format]\n");
    assert_eq!(output, "");
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
    assert_eq!(statistics["planning_invocations"], 1, "{statistics}");
}

#[test]
fn tabbed_scalar_documents_keep_their_original_indentation() {
    let (output, statistics) = format_with_statistics(
        "first :\n    body: |\n      \tinside\n    list: [ a,b ] #note\n",
        "[format]\n",
    );
    assert_eq!(
        output,
        "first:\n    body: |\n      \tinside\n    list: [a, b]  # note\n"
    );
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
}

#[test]
fn inconsistent_sibling_documents_keep_their_width_without_a_rollback() {
    let (output, statistics) = format_with_statistics(
        "first :\n    body: |\n      \tinside\n    list: [ a,b ] #note\n---\nsecond :\n    mapping:\n        key: 'café'\n",
        "[format]\n",
    );
    assert_eq!(
        output,
        "first:\n    body: |\n      \tinside\n    list: [a, b]  # note\n---\nsecond:\n    mapping:\n        key: café\n"
    );
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
    assert_eq!(statistics["rollback"], 0, "{statistics}");
}

fn format_with_workers(input: &str, config: &str, workers: usize) -> (String, Value) {
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
        .env("RAYON_NUM_THREADS", workers.to_string())
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
fn multiline_quotes_share_one_layout_plan() {
    let input = "é:\n    value: 'folded first\n      continuation café' #note\n    necessary: \"**Required unless you provide email**\n      to invite café.\" #note\n    after: x\n";
    let (output, statistics) = format_with_statistics(input, "[format]\n");
    assert_eq!(
        output,
        "é:\n  value: folded first continuation café  # note\n  necessary: '**Required unless you provide email**\n    to invite café.'  # note\n  after: x\n"
    );
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
    for (name, count) in [
        ("analyses", 1),
        ("lexical_scans", 0),
        ("planning_invocations", 1),
        ("emissions", 1),
        ("strict_parses", 2),
        ("verification_parses", 0),
        ("rollback", 0),
        ("cached_lexical_tapes", 1),
        ("line_table_builds", 1),
        ("token_normalizations", 1),
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
    assert_eq!(statistics["line_table_builds"], 1, "{statistics}");
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
    assert_eq!(statistics["cached_lexical_tapes"], 1, "{statistics}");
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
fn suppression_filters_only_the_disabled_rules_edits() {
    let input = "# ryl disable rule:quoted-strings\né : 'café'\n";
    let (output, statistics) = format_with_statistics(input, "[format]\n");
    assert_eq!(output, "# ryl disable rule:quoted-strings\né: 'café'\n");
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
}

#[test]
fn suppressed_endings_and_blank_runs_keep_their_bytes() {
    let (output, stats) = format_with_statistics(
        "# ryl disable rule:new-lines rule:empty-lines\né : value \n\n\n",
        "[format]\nline-ending='cr-lf'\n",
    );
    assert_eq!(
        output,
        "# ryl disable rule:new-lines rule:empty-lines\né: value\n\n\n"
    );
    assert_eq!(stats["fallback"], Value::Null, "{stats}");
}

#[test]
fn end_marker_insertion_obeys_the_last_lines_directive() {
    let (output, stats) = format_with_statistics(
        "é : value # ryl disable-line rule:document-end\n",
        "[format]\ndocument-start='add'\ndocument-end='add'\n",
    );
    assert_eq!(
        output,
        "---\né: value  # ryl disable-line rule:document-end\n"
    );
    assert_eq!(stats["fallback"], Value::Null, "{stats}");
}

#[test]
fn partially_suppressed_multiline_quotes_preserve_rollback() {
    let input =
        "other : x\né: 'first\n  last' # ryl disable-line rule:quoted-strings\n";
    let (output, stats) = format_with_statistics(input, "[format]\n");
    assert_eq!(output, input);
    assert_eq!(stats["fallback"], "Verification", "{stats}");
    assert_eq!(stats["rollback"], 1, "{stats}");
}

#[test]
fn suppressed_document_starts_still_allow_spacing_changes() {
    let (output, stats) = format_with_statistics(
        "é : value # ryl disable-line rule:document-start\n",
        "[format]\ndocument-start='add'\n",
    );
    assert_eq!(output, "é: value  # ryl disable-line rule:document-start\n");
    assert_eq!(stats["fallback"], Value::Null, "{stats}");
}

#[test]
fn inserted_markers_keep_suppressed_line_endings() {
    let (output, stats) = format_with_statistics(
        "# ryl disable rule:new-lines\né : value\r\n",
        "[format]\ndocument-end='add'\n",
    );
    assert_eq!(output, "# ryl disable rule:new-lines\né: value\r\n...\r\n");
    assert_eq!(stats["fallback"], Value::Null, "{stats}");
}

#[test]
fn folding_keeps_other_rules_inline_directives_attached() {
    let (output, stats) = format_with_statistics(
        "other : x\né: alpha beta gamma delta # ryl disable-line rule:comments\n",
        "line-length=12\n[format]\nfold-long-lines=true\n",
    );
    assert_eq!(
        output,
        "other: x\né: alpha beta gamma delta # ryl disable-line rule:comments\n"
    );
    assert_eq!(stats["fallback"], Value::Null, "{stats}");
}

#[test]
fn sequence_conversion_keeps_its_verification() {
    let config = "[format]\nsequence-style = 'block'\n";
    let (output, statistics) = format_with_statistics("é: [ café,世 ]\n", config);
    assert_eq!(output, "é:\n  - café\n  - 世\n");
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
    assert!(statistics["verification_parses"].as_u64().unwrap() >= 1);
    assert_eq!(format_with_statistics(&output, config).0, output);
}

#[test]
fn mapping_conversion_uses_layout_after_restyling() {
    let config = "[format]\nmapping-style = 'block'\n";
    let (output, statistics) = format_with_statistics("é: { café: '世' }\n", config);
    assert_eq!(output, "é:\n  café: 世\n");
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
}

#[test]
fn block_sequence_conversion_to_flow_uses_layout() {
    let config = "[format]\nsequence-style = 'flow'\n";
    let (output, statistics) = format_with_statistics("é:\n  - café\n  - 世\n", config);
    assert_eq!(output, "é: [café, 世]\n");
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
    assert_eq!(format_with_statistics(&output, config).0, output);
}

#[test]
fn block_mapping_conversion_to_flow_uses_layout() {
    let config = "[format]\nmapping-style = 'flow'\n";
    let (output, statistics) = format_with_statistics("é:\n  café: 世\n", config);
    assert_eq!(output, "é: {café: 世}\n");
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
    assert_eq!(format_with_statistics(&output, config).0, output);
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
fn folded_unterminated_blank_tail_folds_before_reindenting() {
    let (output, statistics) = format_with_statistics(
        "  >\n  abc def ghi jkl\n    more\n\n\n  ",
        "line-length = 12\nindent-width = 1\n[format]\nfold-long-lines = true\n",
    );
    assert_eq!(output, ">\n abc def\n ghi jkl\n   more\n");
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
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
fn quoted_flow_key_gaps_compose_with_boundary_insertions() {
    let (output, statistics) = format_with_statistics(
        "{\"foo\"\n: bar}\n",
        "[format]\nbrace-spacing = true\n",
    );
    assert_eq!(output, "{ foo\n  : bar }\n");
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
}

fn assert_one_pass(input: &str, config: &str, expected: &str) {
    let (output, statistics) = format_with_statistics(input, config);
    assert_eq!(output, expected);
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
    assert_eq!(format_with_statistics(&output, config).0, output);
}

#[test]
fn a_marker_key_before_a_colon_the_formatter_closes_unquotes_in_one_pass() {
    assert_one_pass("'---' : x\n", "[format]\n", "---: x\n");
}

#[test]
fn a_split_dash_trims_its_blank_keep_body_in_one_pass() {
    assert_one_pass(
        "-   a: |+\n     \n\n    b: x\n",
        "[format]\ndash-on-own-line = true\n",
        "-\n  a: |+\n\n\n  b: x\n",
    );
}

#[test]
fn a_split_dash_drops_its_gap_beside_a_byte_order_mark_in_one_pass() {
    assert_one_pass(
        "a:\n  -  a: d\n         e\n...\n\u{feff}\nb: 1\n",
        "[format]\ndash-on-own-line = true\n",
        "a:\n  -\n    a: d\n        e\n...\n\u{feff}\nb: 1\n",
    );
}

#[test]
fn an_explicit_value_split_from_its_dash_indents_from_its_target_in_one_pass() {
    let config = "indent-width = 4\n[format]\nindent-sequences = false\ndash-on-own-line = true\n";
    assert_one_pass("? d\n:  -   k:   c\n", config, "? d\n: -\n      k: c\n");
    assert_one_pass(
        "? d\n:  -   k:   c\n         p\n",
        config,
        "? d\n: -\n      k: c\n        p\n",
    );
}

#[test]
fn a_comment_before_an_added_document_end_keeps_its_column_in_one_pass() {
    assert_one_pass(
        "a: >\n\nt: |3\n   #s\n\n  # after\n",
        "[format]\ndocument-end = 'add'\n",
        "a: >\n\nt: |3\n   #s\n\n  # after\n...\n",
    );
}

#[test]
fn a_flow_key_before_a_colon_the_formatter_spaces_unquotes_in_one_pass() {
    assert_one_pass(
        "---\n{ \"foo\"\n  :bar }\n",
        "[format]\n",
        "---\n{foo\n : bar}\n",
    );
    assert_one_pass("{\"name\":\"admin\"}\n", "[format]\n", "{name: admin}\n");
}

#[test]
fn a_flow_key_keeps_its_quotes_where_colons_are_disabled() {
    assert_one_pass(
        "{\"name\":admin}  # yamllint disable-line rule:colons\n",
        "[format]\n",
        "{'name':admin}  # yamllint disable-line rule:colons\n",
    );
}

#[test]
fn a_mapping_one_column_after_its_dash_joins_it_in_one_pass() {
    let width_4 = "indent-width = 4\n[format]\n";
    for (input, config, expected) in [
        (
            "-\n foo: bar\n baz: 1\n",
            "[format]\n",
            "- foo: bar\n  baz: 1\n",
        ),
        ("-\n \"foo\": bar\n", "[format]\n", "- foo: bar\n"),
        ("-\n a: x\n", "indent-width = 1\n[format]\n", "-\n a: x\n"),
        ("-\n  a: x\n", "indent-width = 1\n[format]\n", "- a: x\n"),
        (
            "-\n foo: bar  # yamllint disable-line rule:hyphens\n",
            "[format]\n",
            "- foo: bar  # yamllint disable-line rule:hyphens\n",
        ),
        (
            "# yamllint disable rule:hyphens\n-\n a: x\n",
            width_4,
            "# yamllint disable rule:hyphens\n-\n    a: x\n",
        ),
        (
            "-\n a: x\n# yamllint disable rule:hyphens\n-\n a: x\n",
            width_4,
            "-\n    a: x\n# yamllint disable rule:hyphens\n-\n    a: x\n",
        ),
        (
            "# yamllint disable rule:indentation\n-\n a: x  \n",
            "[format]\n",
            "# yamllint disable rule:indentation\n-\n a: x\n",
        ),
        (
            "-\n a: x\n---\n# yamllint disable rule:indentation\n-\n a: x  \n",
            "[format]\n",
            "- a: x\n---\n# yamllint disable rule:indentation\n-\n a: x\n",
        ),
    ] {
        assert_one_pass(input, config, expected);
    }
}

#[test]
fn a_mapping_split_from_its_dash_drops_the_gap_in_one_pass() {
    let flush = "indent-width = 4\n[format]\nindent-sequences = false\ndash-on-own-line = true\n";
    let own_line = "[format]\ndash-on-own-line = true\n";
    for (input, config, expected) in [
        ("-  a: x\n", flush, "-\n    a: x\n"),
        ("-  a:  x\n", flush, "-\n    a: x\n"),
        (
            "# yamllint disable rule:indentation\n-  a: x\n",
            own_line,
            "# yamllint disable rule:indentation\n- a: x\n",
        ),
        (
            "# yamllint disable rule:hyphens rule:indentation\n-  a: x\n",
            own_line,
            "# yamllint disable rule:hyphens rule:indentation\n-  a: x\n",
        ),
    ] {
        assert_one_pass(input, config, expected);
    }
}

#[test]
fn trailing_comments_keep_what_comments_indentation_exempts() {
    for (input, config, expected) in [
        (
            "a: |\n  x\n\n # t\n# u\n",
            "[format]\n",
            "a: |\n  x\n\n # t\n# u\n",
        ),
        (
            "a:\n   b: 1\n# yamllint disable rule:empty-lines\n # c\n",
            "[format]\n",
            "a:\n  b: 1\n# yamllint disable rule:empty-lines\n  # c\n",
        ),
        (
            "a:\n   b: 1\n# yamllint disable rule:comments-indentation\n# c\n",
            "[format]\n",
            "a:\n  b: 1\n# yamllint disable rule:comments-indentation\n# c\n",
        ),
        (
            "a:\n   b: 1\n# c\n",
            "[format]\n[lint.rules.comments-indentation]\nallow-any-open-indent = true\n",
            "a:\n  b: 1\n# c\n",
        ),
    ] {
        assert_one_pass(input, config, expected);
    }
}

#[test]
fn a_root_quote_restyle_settles_its_ladder_in_one_pass() {
    assert_one_pass(
        "\"3 inline\ttab\"\n",
        "[format]\nquote-style = 'single'\n",
        "3 inline\ttab\n",
    );
}

#[test]
fn a_requoted_multiline_scalar_trims_its_trailing_spaces_in_one_pass() {
    assert_one_pass(
        "d: \"  \n  \"\n",
        "[format]\nquote-style = 'single'\n",
        "d: '\n  '\n",
    );
}

#[test]
fn a_marker_on_a_bom_only_line_counts_the_rest_as_blank_in_one_pass() {
    assert_one_pass(
        "a: a\n...\n\u{feff}\na: 1\n",
        "[format]\ndocument-start = 'add'\nmax-blank-lines = 0\n",
        "---\na: a\n...\n\u{feff}---\na: 1\n",
    );
}

#[test]
fn a_tab_only_line_does_not_refuse_reindenting_its_flow_in_one_pass() {
    assert_one_pass("- [\n\t\n foo\n ]\n", "[format]\n", "- [\n\n  foo\n ]\n");
    for kept in [
        "# yamllint disable rule:trailing-spaces\n- [\n\t\n foo\n ]\n",
        "foo: |\n \t\nbar:\n   1\n",
        "a:\n   b: \"x\\\n      \t\n      y\"\n",
    ] {
        assert_one_pass(kept, "[format]\n", kept);
    }
    let crlf = "a:\r\n   b: \"x\\\r\n      \t\r\n      y\"\r\n";
    assert_one_pass(crlf, "[format]\nline-ending = 'cr-lf'\n", crlf);
}

#[test]
fn a_comment_after_the_last_content_aligns_with_its_target_in_one_pass() {
    assert_one_pass(
        "a:\n  b:\n   - c\n# end\n\n # tail\n",
        "[format]\n",
        "a:\n  b:\n    - c\n    # end\n\n    # tail\n",
    );
}

#[test]
fn root_quotes_complete_a_verified_layout_plan() {
    let (output, statistics) = format_with_statistics("---\n\"foo\"\n", "[format]\n");
    assert_eq!(output, "---\nfoo\n");
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
}

#[test]
fn a_comment_reindentation_would_carry_into_a_block_scalar_freezes_its_document() {
    let (output, statistics) =
        format_with_statistics("a:\n -  b: |1\n   \n    # c\n", "[format]\n");
    assert_eq!(output, "a:\n -  b: |1\n\n    # c\n");
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
}

#[test]
fn a_collapsed_root_scalar_folds_after_quote_settling() {
    let (output, statistics) = format_with_statistics(
        "--- \"a\n...x\nb\"\n",
        "[format]\nfold-long-lines = true\n",
    );
    assert_eq!(output, "--- a ...x b\n");
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
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
fn skip_finishing_formats_in_layout() {
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
fn skipped_flow_spacing_keeps_the_original_gaps_and_comments() {
    let input = "items: { a: [ b,c ] } #note\n";
    assert_eq!(
        format_with_skipped_rules(
            input,
            "[format]\n",
            &["braces", "brackets", "commas", "comments"],
        ),
        input
    );
}

#[test]
fn template_text_keeps_the_legacy_bracket_walk() {
    let input = "items: [ a,b ]\nvalue: '{{ x }}'\n";
    let config = "[format]\nquote-style='preserve'\n";
    assert_eq!(
        format_with_skipped_rules(input, config, &[]),
        "items: [a, b]\nvalue: '{{ x }}'\n"
    );
    assert_eq!(
        format_with_skipped_rules(input, config, &["brackets"]),
        "items: [ a, b ]\nvalue: '{{ x }}'\n"
    );
}

#[test]
fn bom_quote_collapses_keep_the_byte_derived_tail_adapter() {
    let output = format_with_skipped_rules(
        "\u{feff}key: ['first\n  second', 'third\n  fourth'] #note\n",
        "[format]\n",
        &[],
    );
    assert_eq!(
        output,
        "\u{feff}key: [first second, third fourth]  # note\n"
    );
}

#[test]
fn bom_flow_key_overlap_restarts_the_ordered_driver() {
    let output = format_with_skipped_rules(
        "\u{feff}{\"foo\"\n: bar}\n",
        "[format]\nbrace-spacing=true\n",
        &[],
    );
    assert_eq!(output, "\u{feff}{ foo\n  : bar }\n");
}

#[test]
fn bom_unsettled_root_quote_restarts_the_ordered_driver() {
    let output = format_with_skipped_rules("\u{feff}---\n\"foo\"\n", "[format]\n", &[]);
    assert_eq!(output, "\u{feff}---\nfoo\n");
}

#[test]
fn bom_root_scalar_fold_restarts_the_ordered_driver() {
    let output = format_with_skipped_rules(
        "\u{feff}--- \"a\n...x\nb\"\n",
        "[format]\nfold-long-lines=true\n",
        &[],
    );
    assert_eq!(output, "\u{feff}--- a ...x b\n");
}

#[test]
fn skip_final_newline_formats_in_layout() {
    let output = format_with_skipped_rules(
        "a: |+\n  a\n  ",
        "[format]\n",
        &["new-line-at-end-of-file", "new-lines"],
    );
    assert_eq!(output, "a: |+\n  a\n\n");
}

#[test]
fn skip_ending_markers_formats_in_layout() {
    let output = format_with_skipped_rules(
        "a : b\r\n",
        "[format]\ndocument-start='add'\ndocument-end='add'\n",
        &["new-lines"],
    );
    assert_eq!(output, "---\r\na: b\r\n...\r\n");
}

#[test]
fn overlapping_collapses_formats_in_layout() {
    let output = format_with_skipped_rules(
        "a: ['first\n  second', 'third\n  fourth']\n",
        "[format]\n",
        &[],
    );
    assert_eq!(output, "a: [first second, third fourth]\n");
}

#[test]
fn joined_quote_formats_in_layout() {
    let output =
        format_with_skipped_rules("-\n  a: 'first\n    second'\n", "[format]\n", &[]);
    assert_eq!(output, "- a: first second\n");
}

#[test]
fn collapsed_trailing_formats_in_layout() {
    let output = format_with_skipped_rules("a: 'one  \n  two'\n", "[format]\n", &[]);
    assert_eq!(output, "a: one two\n");
}

#[test]
fn underindented_quoted_formats_in_layout() {
    let output = format_with_skipped_rules(
        "a: 'alpha beta\n gamma delta epsilon'\n",
        "indent-width=1\nline-length=12\n[format]\nquote-style='preserve'\nfold-long-lines=true\n",
        &[],
    );
    assert_eq!(output, "a: 'alpha beta\n gamma delta epsilon'\n");
}

#[test]
fn folded_blank_formats_in_layout() {
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
fn directive_fold_formats_in_layout() {
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
fn bom_fold_formats_in_layout() {
    let output = format_with_skipped_rules(
        "﻿é: alpha beta gamma delta\n",
        "line-length=12\n[format]\nfold-long-lines=true\n",
        &[],
    );
    assert_eq!(output, "﻿é: alpha\n  beta gamma\n  delta\n");
}

#[test]
fn gap_fold_formats_in_layout() {
    let output = format_with_skipped_rules(
        "-   alpha beta gamma delta\n",
        "line-length=12\n[format]\nfold-long-lines=true\n",
        &[],
    );
    assert_eq!(output, "- alpha beta\n  gamma\n  delta\n");
}

#[test]
fn erased_dash_tail_formats_in_layout() {
    let output = format_with_skipped_rules("-   \n  a: 1\n", "[format]\n", &[]);
    assert_eq!(output, "- a: 1\n");
}

#[test]
fn unicode_flow_gap_formats_in_layout() {
    let output =
        format_with_skipped_rules("[😀😀, ?   key: value]\n", "[format]\n", &[]);
    assert_eq!(output, "[😀😀, ? key: value]\n");
}

#[test]
fn unicode_flow_gap_space_formats_in_layout() {
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
fn a_commented_mapping_keeps_its_dash() {
    let (output, statistics) =
        format_with_statistics("- a : b # tail\n", "[format]\ndash-on-own-line=true\n");
    assert_eq!(output, "- a: b  # tail\n");
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
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
fn an_empty_flow_string_keeps_its_indentation() {
    let (output, statistics) = format_with_statistics("[\n  \"\",\n]\n", "[format]\n");
    assert_eq!(output, "[\n  ''\n]\n");
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
    assert_eq!(format_with_statistics(&output, "[format]\n").0, output);
}

fn threshold_input(bytes: usize, body: &str) -> String {
    format!("# {}\n{body}", "p".repeat(bytes - body.len() - 3))
}

fn assert_threshold(bytes: usize, scans: usize) {
    let input = threshold_input(bytes, "é:  x\n");
    let (output, stats) = format_with_statistics(&input, "[format]\n");
    assert!(
        output == input.replace("é:  x", "é: x"),
        "only the colon gap changes"
    );
    assert_eq!(stats["fallback"], Value::Null);
    assert_eq!(stats["cached_lexical_tapes"], 1);
    for (name, count) in [
        ("analyses", 1),
        ("strict_parses", 2),
        (
            "lexical_scans",
            if std::thread::available_parallelism().unwrap().get() > 1 {
                scans
            } else {
                0
            },
        ),
        ("planning_invocations", 1),
        ("emissions", 1),
    ] {
        assert_eq!(stats[name], count, "{name}: {stats}");
    }
}

#[test]
fn small_epochs_capture_tokens() {
    assert_threshold(1_048_575, 0);
}

#[test]
fn one_mib_epochs_capture_both_tapes() {
    assert_threshold(1_048_576, 1);
}

#[test]
fn large_epochs_keep_parallel_prefetch() {
    assert_threshold(1_048_577, 1);
}

#[test]
fn one_worker_keeps_large_epochs_serial() {
    let input = threshold_input(1_048_580, "é:  x\n");
    let (output, stats) = format_with_workers(&input, "[format]\n", 1);
    assert!(
        output == input.replace("é:  x", "é: x"),
        "only the colon gap changes"
    );
    assert_eq!(stats["cached_lexical_tapes"], 1);
    assert_eq!(stats["lexical_scans"], 0);
}

#[test]
fn large_bom_reuses_cached_tokens() {
    let input = format!("\u{feff}{}", threshold_input(1_048_580, "é:  x\n"));
    let (output, stats) = format_with_workers(&input, "[format]\n", 4);
    assert!(
        output == input.replace("é:  x", "é: x"),
        "only the colon gap changes"
    );
    assert_eq!(
        stats["lexical_scans"],
        usize::from(std::thread::available_parallelism().unwrap().get() > 1),
        "{stats}"
    );
}

#[test]
fn late_parse_error_keeps_original_bytes_and_error() {
    let input = threshold_input(1_048_580, "é:  x\nbroken: [\n");
    let (code, _, diagnostics, output) =
        common::cli::ryl_on(Some("[format]\n"), &input, &["format", "--no-cache"]);
    assert_eq!(output, input);
    assert_eq!(code, 0, "{diagnostics}");
    assert!(diagnostics.contains("skipped"), "{diagnostics}");
}

#[test]
fn a_large_input_freezes_the_document_a_comment_would_carry_into_a_block_scalar() {
    let input = threshold_input(
        1_048_620,
        "map: {a:,b: null}\na:\n -  b: |1\n   \n    # c\n",
    );
    let (output, stats) = format_with_statistics(&input, "[format]\n");
    assert!(
        output
            == input
                .replace("{a:,b:", "{a:, b:")
                .replace("|1\n   \n", "|1\n\n")
    );
    assert_eq!(stats["fallback"], Value::Null, "{stats}");
}

#[test]
fn a_large_fold_leaves_the_comment_after_its_scalar_in_place() {
    let body = "a:\n  b: xxxxxxxxx yyyyyyyyyy # c\n  # d\n";
    let input = threshold_input(1_048_620, body);
    let (output, stats) = format_with_statistics(
        &input,
        "line-length = 20\n[format]\nfold-long-lines = true\n",
    );
    let folded = "a:\n  b: xxxxxxxxx\n    yyyyyyyyyy  # c\n  # d\n";
    assert!(output == input.replace(body, folded));
    assert_eq!(stats["fallback"], Value::Null, "{stats}");
}

#[test]
fn concurrent_input_gates_do_not_charge_another_files_recording() {
    assert_independent_recordings(false);
}

#[test]
fn concurrent_previews_do_not_charge_another_files_recording() {
    assert_independent_recordings(true);
}

#[test]
fn a_fold_without_reindentation_continues_under_its_source_owner() {
    let (code, _, diagnostics, _) = common::cli::ryl_on(
        Some(
            "line-length = 12\n[format]\nfold-long-lines = true\nquote-style = 'single'\n",
        ),
        "spec:\n volumes:\n       lun: <ADD LUN ID Here>\n",
        &["format", "--check", "--output-format", "parsable"],
    );
    assert_eq!(code, 1, "{diagnostics}");
    assert!(
        diagnostics.contains(":3:13: [error] line too long"),
        "{diagnostics}"
    );
    assert!(!diagnostics.contains("would reformat"), "{diagnostics}");
}

fn assert_independent_recordings(preview: bool) {
    let directory = tempfile::tempdir().unwrap();
    let configuration = directory.path().join("ryl.toml");
    let statistics = directory.path().join("statistics.jsonl");
    fs::write(&configuration, "[format]\n").unwrap();
    for (name, body) in [
        ("block.yaml", "keep: |+\n  text\n\nnext:  é\n"),
        ("invalid.yaml", "key: [unfinished\n"),
        ("flow.yaml", "café:  [é,世]\n"),
        ("plain.yaml", "é:  x\n"),
    ] {
        fs::write(
            directory.path().join(name),
            threshold_input(1_048_620, body),
        )
        .unwrap();
    }
    let mut command = Command::new(env!("CARGO_BIN_EXE_ryl"));
    if preview {
        command.arg("format").arg("--check");
    } else {
        command.arg("format");
    }
    let output = command
        .args(["--no-cache", "-c"])
        .arg(configuration)
        .arg(directory.path())
        .env("RYL_LAYOUT_STATS", &statistics)
        .env("RAYON_NUM_THREADS", "4")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(i32::from(preview)));
    let rows: Vec<Value> = fs::read_to_string(statistics)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    if preview {
        assert!(rows.iter().any(|row| {
            row["seeded"] == false && row["planning_invocations"].as_u64().unwrap() > 0
        }));
    }
    let rows: Vec<_> = rows
        .into_iter()
        .filter(|row: &Value| row["seeded"] == true && row["planning_invocations"] == 1)
        .collect();
    assert_eq!(rows.len(), 3);
    for row in rows {
        assert_eq!(row["fallback"], Value::Null, "{row}");
        assert_eq!(row["analyses"], 1, "{row}");
        assert_eq!(row["strict_parses"], 2, "{row}");
        assert_eq!(
            row["lexical_scans"],
            usize::from(std::thread::available_parallelism().unwrap().get() > 1),
            "{row}"
        );
    }
}

#[test]
fn forced_collections_settle_multiline_quote_offsets_before_folding() {
    let config = "line-length = 35\n[format]\nsequence-style = 'block'\nfold-long-lines = true\n";
    let (output, statistics) = format_with_statistics(
        "é: 'The allowed models may change over time\n  and depend on the user''s settings. Supported\n  values include café and many more options.'\nlist: [café,世]\n",
        config,
    );
    assert_eq!(
        output,
        "é: The allowed models may change\n  over time and depend on the\n  user's settings. Supported values\n  include café and many more\n  options.\nlist:\n  - café\n  - 世\n"
    );
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
    assert_eq!(statistics["verification_parses"], 1, "{statistics}");
    assert_eq!(format_with_statistics(&output, config).0, output);
}

#[test]
fn non_ascii_marker_rolls_back_the_collection_stage() {
    let input = "---\u{a0}x\n---\nitems: [a,b]\n";
    let (output, statistics) =
        format_with_statistics(input, "[format]\nsequence-style = 'block'\n");
    assert_eq!(output, input);
    assert_eq!(statistics["fallback"], "Verification", "{statistics}");
}

#[test]
fn non_ascii_marker_falls_back_while_folding_without_quote_edits() {
    let input = "---\u{a0}x\n---\nkey:  value\n";
    let (output, statistics) =
        format_with_statistics(input, "[format]\nfold-long-lines = true\n");
    assert_eq!(output, input);
    assert_eq!(statistics["fallback"], "Verification", "{statistics}");
}

#[test]
fn a_lint_fix_replans_its_output_until_it_settles() {
    let (code, _, stderr, fixed) = common::cli::ryl_on(
        Some("[lint.rules]\nbraces = 'enable'\ncommas = 'enable'\n"),
        "a: { a: a, }\n",
        &["check", "--fix", "--no-cache"],
    );
    assert_eq!((code, fixed.as_str()), (0, "a: {a: a}\n"), "{stderr}");
}

#[test]
fn a_check_reports_the_document_a_comment_would_carry_into_a_block_scalar() {
    let (code, _, diagnostics, _) = common::cli::ryl_on(
        Some("[format]\n"),
        "a:\n -  b: |1\n   \n    # c\n",
        &["format", "--check", "--output-format", "parsable"],
    );
    assert_eq!(code, 1, "{diagnostics}");
    assert!(
        diagnostics.contains(
            ":1:1 indentation not fixed: cannot re-indent this document safely"
        ),
        "{diagnostics}"
    );
}

#[test]
fn a_large_sequence_conversion_keeps_its_prefetched_tokens() {
    let input = threshold_input(1_048_620, "list: [a, b]\n");
    let (output, statistics) =
        format_with_statistics(&input, "[format]\nsequence-style = 'block'\n");
    assert!(output == input.replace("list: [a, b]", "list:\n  - a\n  - b"));
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
}

#[test]
fn a_large_conversion_beside_a_non_ascii_marker_keeps_its_input() {
    let input = threshold_input(1_048_620, "---\u{a0}x\n---\nitems: [a, b]\n");
    let (output, statistics) =
        format_with_statistics(&input, "[format]\nsequence-style = 'block'\n");
    assert!(output == input);
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
}

#[test]
fn non_ascii_marker_rolls_back_the_quote_stage() {
    let input = "---\u{a0}x\n---\nkey: 'value'\n";
    let (output, statistics) = format_with_statistics(
        input,
        "[format]\nsequence-style = 'block'\nfold-long-lines = true\n",
    );
    assert_eq!(output, input);
    assert_eq!(statistics["fallback"], "Verification", "{statistics}");
}

#[test]
fn quote_shortening_reuses_a_dirty_collection_dry_plan() {
    let (output, statistics) = format_with_statistics(
        "key:\n  - 'a'\n  - 'b'\n",
        "line-length = 11\n[format]\nsequence-style = 'flow'\n",
    );
    assert_eq!(output, "key: [a, b]\n");
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
}

#[test]
fn forced_style_settles_root_quotes_in_layout() {
    let (output, statistics) = format_with_statistics(
        "---\n\"foo\"\n",
        "[format]\nsequence-style = 'block'\nfold-long-lines = true\n",
    );
    assert_eq!(output, "---\nfoo\n");
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
}

#[test]
fn indentless_sequences_retry_conversion_after_verified_layout() {
    let input = "café:\n- é\n- 世\n";
    let config = "[format]\nsequence-style = 'flow'\n";
    let (output, statistics) = format_with_statistics(input, config);
    assert_eq!(output, "café: [é, 世]\n");
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
    assert_eq!(format_with_statistics(&output, config).0, output);
}

#[test]
fn folding_settles_multiline_quote_offsets_without_collection_targets() {
    let config = "line-length = 16\n[format]\nfold-long-lines = true\n";
    let (output, statistics) =
        format_with_statistics("é: 'one two three\n  user''s four five six'\n", config);
    assert_eq!(output, "é: one two three\n  user's four\n  five six\n");
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
    assert_eq!(statistics["verification_parses"], 0, "{statistics}");
    assert_eq!(format_with_statistics(&output, config).0, output);
}

#[test]
fn rejected_collection_without_layout_progress_keeps_the_settled_layout() {
    let input = "---\u{a0}x\n---\nitems: [a, b]\n";
    let (output, statistics) =
        format_with_statistics(input, "[format]\nsequence-style = 'block'\n");
    assert_eq!(output, input);
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
}

#[test]
fn block_conversion_keeps_trailing_comments_in_the_ordered_phase() {
    let config = "[format]\nsequence-style = 'block'\n";
    let (output, statistics) =
        format_with_statistics("café:\n  seq: [a, b]\n  # note\nnext: value\n", config);
    assert_eq!(
        output,
        "café:\n  seq:\n    - a\n    - b\n# note\nnext: value\n"
    );
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
    assert_eq!(format_with_statistics(&output, config).0, output);
}

#[test]
fn flow_conversion_settles_misaligned_comments_before_reindentation() {
    let config = "[format]\nmapping-style = 'flow'\n";
    let (output, statistics) =
        format_with_statistics("café:\n  key: value\n   # note\nnext: value\n", config);
    assert_eq!(output, "café: {key: value}\n# note\nnext: value\n");
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
    assert_eq!(format_with_statistics(&output, config).0, output);
}

#[test]
fn converted_trailing_comments_align_before_document_end_insertion() {
    let (output, statistics) = format_with_statistics(
        "- name: café\n  tags: [a, b]\n  tasks:\n  - name: value\n# tail\n",
        "[format]\nsequence-style = 'block'\ndocument-end = 'add'\n",
    );
    assert_eq!(
        output,
        "- name: café\n  tags:\n    - a\n    - b\n  tasks:\n    - name: value\n    # tail\n...\n"
    );
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
}

#[test]
fn trailing_commas_drop_inside_padded_collections() {
    let (output, statistics) =
        format_with_statistics("café: [ one, two, ]\nmap: { a : b , }\n", "[format]\n");
    assert_eq!(output, "café: [one, two]\nmap: {a: b}\n");
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
}

#[test]
fn lf_only_stream_uses_one_plan() {
    let (output, statistics) =
        format_with_statistics(&"\n".repeat(262_144), "[format]\n");
    assert_eq!(output, "");
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
    assert_eq!(statistics["verification_parses"], 0, "{statistics}");
    assert_eq!(statistics["planning_invocations"], 1, "{statistics}");
    assert_eq!(statistics["line_table_builds"], 1, "{statistics}");
    assert_eq!(statistics["emissions"], 1, "{statistics}");
}

#[test]
fn joining_a_mapping_discards_the_dashes_trailing_whitespace() {
    let (output, statistics) = format_with_statistics("- \n  café:", "[format]\n");
    assert_eq!(output, "- café:\n");
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
    assert_eq!(statistics["emissions"], 1, "{statistics}");
}

#[test]
fn splitting_a_mapping_discards_the_old_dash_spacing() {
    let (output, statistics) = format_with_statistics(
        "-  café: ''",
        "indent-width = 4\n[format]\nindent-sequences = false\ndash-on-own-line = true\n",
    );
    assert_eq!(output, "-\n    café: ''\n");
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
}

#[test]
fn strict_invalid_indentation_is_repaired_before_layout() {
    let (output, statistics) =
        format_with_statistics("---\ncafé: [a,\nb,\nc]\n", "[format]\n");
    assert_eq!(output, "---\ncafé: [a,\n       b,\n       c]\n");
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
}

#[test]
fn explicit_indicator_folds_keep_more_indented_lines() {
    let (output, statistics) = format_with_statistics(
        "a: >1\n  café words here and more words here\n regular words here and more words\n",
        "line-length = 20\n[format]\nfold-long-lines = true\n",
    );
    assert_eq!(
        output,
        "a: >1\n  café words here and more words here\n regular words here\n and more words\n"
    );
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
}

#[test]
fn frozen_documents_fold_from_their_original_indentation() {
    let (output, statistics) = format_with_statistics(
        "file:\n  path: \"/tmp/{{ item }}\"\nloop:\n -  café: |1\n   \n    # c\n",
        "line-length = 12\nindent-width = 4\n[format]\nfold-long-lines = true\n",
    );
    assert_eq!(
        output,
        "file:\n  path: /tmp/{{\n      item\n      }}\nloop:\n -  café: |1\n\n    # c\n"
    );
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
}

#[test]
fn tabbed_blank_lines_are_erased() {
    let (output, statistics) = format_with_statistics(" \t\ncafé: 1\n", "[format]\n");
    assert_eq!(output, "café: 1\n");
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
}

#[test]
fn folded_partially_suppressed_quotes_roll_back() {
    let input =
        "a: 'one\n  two'  # ryl disable-line rule:quoted-strings\ncafé: \"x\"\n";
    let (output, statistics) = format_with_statistics(
        input,
        "line-length = 20\n[format]\nfold-long-lines = true\n",
    );
    assert_eq!(output, input);
    assert_eq!(statistics["fallback"], "Verification", "{statistics}");
}

#[test]
fn bom_partially_suppressed_quotes_roll_back() {
    let input = "\u{feff}a: 'one\n  two'  # ryl disable-line rule:quoted-strings\ncafé: \"x\"\n";
    let (output, statistics) = format_with_statistics(input, "[format]\n");
    assert_eq!(output, input);
    assert_eq!(statistics["fallback"], "Verification", "{statistics}");
}

#[test]
fn trailing_commas_drop_beside_tab_spacing() {
    let (output, statistics) =
        format_with_statistics("café: [ x,\ty, z,  ]\n", "[format]\n");
    assert_eq!(output, "café: [x,\ty, z]\n");
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
}

#[test]
fn suppressed_trailing_commas_are_kept() {
    let (output, statistics) = format_with_statistics(
        "a: [x,]  # ryl disable-line rule:commas\ncafé: [y,  ]\n",
        "[format]\n",
    );
    assert_eq!(
        output,
        "a: [x,]  # ryl disable-line rule:commas\ncafé: [y]\n"
    );
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
}

#[test]
fn padded_braces_keep_their_padding_without_the_trailing_comma() {
    let (output, statistics) =
        format_with_statistics("café: {b: 1,  }\n", "[format]\nbrace-spacing = true\n");
    assert_eq!(output, "café: { b: 1 }\n");
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
}

#[test]
fn suppression_guards_comment_alignment() {
    let (output, statistics) = format_with_statistics(
        "a:\n  b: 1\n      # note\n  café: 2  # ryl disable-line rule:colons\n",
        "[format]\n",
    );
    assert_eq!(
        output,
        "a:\n  b: 1\n  # note\n  café: 2  # ryl disable-line rule:colons\n"
    );
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
}

#[test]
fn refused_documents_collapse_quotes_in_a_separate_stage() {
    let (output, statistics) = format_with_statistics(
        "# ryl disable rule:indentation\ncafé: ['first\n  second', 'third\n  fourth']\n",
        "[format]\n",
    );
    assert_eq!(
        output,
        "# ryl disable rule:indentation\ncafé: [first second, third fourth]\n"
    );
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
}

#[test]
fn suppressed_blank_runs_merge_removed_lines() {
    let (output, statistics) = format_with_statistics(
        "a: 1\n\n\n\n\n\ncafé: 2  # ryl disable-line rule:colons\n",
        "[format]\n",
    );
    assert_eq!(
        output,
        "a: 1\n\n\ncafé: 2  # ryl disable-line rule:colons\n"
    );
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
}

#[test]
fn disabled_files_count_a_skip() {
    let input = "# ryl disable-file\ncafé:   1\n";
    let (output, statistics) = format_with_statistics(input, "[format]\n");
    assert_eq!(output, input);
    assert_eq!(statistics["skips"], 1, "{statistics}");
}

#[test]
fn repair_reindents_alongside_inconsistent_mappings() {
    let (output, statistics) = format_with_statistics(
        "a:\n  b: 1\nc:\n    d: 2\ncafé: [x,\ny]\n",
        "[format]\n",
    );
    assert_eq!(output, "a:\n  b: 1\nc:\n  d: 2\ncafé: [x,\n       y]\n");
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
}

#[test]
fn indicator_bodies_follow_their_collection_past_a_spaced_value() {
    let (output, statistics) =
        format_with_statistics("? café\n:  - x2: 1\n   - |1\n    x\n", "[format]\n");
    assert_eq!(output, "? café\n: - x2: 1\n  - |1\n   x\n");
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
}

#[test]
fn consistency_restarts_after_a_staged_epoch_keep_every_document() {
    let input = "a:\n    body: |\n      \tinside\n    list: [ a,b ] #note\n\n---\ncafé:\n    body: >\n      alpha beta gamma delta\n\n ";
    let (output, statistics) = format_with_statistics(
        input,
        "line-length = 12\n[format]\nfold-long-lines = true\n",
    );
    assert_eq!(
        output,
        "a:\n    body: |\n      \tinside\n    list: [a, b]  # note\n\n---\ncafé:\n    body: >\n      alpha\n      beta\n      gamma\n      delta\n"
    );
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
}

#[test]
fn only_the_folded_tail_document_folds_before_reindenting() {
    let (output, statistics) = format_with_statistics(
        "café: >\n    alpha beta gamma\n      extra\n\n \n---\na:\n    body: >\n      alpha beta gamma delta\n\n ",
        "line-length = 12\n[format]\nfold-long-lines = true\n",
    );
    assert_eq!(
        output,
        "café: >\n  alpha beta\n  gamma\n    extra\n\n\n---\na:\n  body: >\n    alpha\n    beta\n    gamma\n    delta\n"
    );
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
}

#[test]
fn a_directive_on_the_next_line_does_not_stop_a_block_fold() {
    let (output, statistics) = format_with_statistics(
        "a: >-\n  more text here\ncafé : 1 # ryl disable-line\n",
        "line-length = 12\n[format]\nfold-long-lines = true\n",
    );
    assert_eq!(
        output,
        "a: >-\n  more text\n  here\ncafé : 1 # ryl disable-line\n"
    );
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
}

#[test]
fn non_lf_whitespace_plans_line_records() {
    let (output, statistics) = format_with_statistics(" \n\t\n", "[format]\n");
    assert_eq!(output, "");
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
    assert_eq!(statistics["line_table_builds"], 1, "{statistics}");
}

#[test]
fn lf_only_collection_conversion_plans_line_records() {
    let (output, statistics) =
        format_with_statistics("\n\n", "[format]\nsequence-style = \"flow\"\n");
    assert_eq!(output, "");
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
    assert_eq!(statistics["line_table_builds"], 1, "{statistics}");
}

#[test]
fn a_trimmed_comment_does_not_freeze_its_document_beside_a_refused_one() {
    let config = "indent-width = 1\n";
    let reindented = "a: |\n #g\n# x\n---\na: |\n  #g\n # x\n";
    let (output, _) =
        format_with_statistics("a: |\n  #g\n # x \n---\na: |\n  #g\n # x\n", config);
    assert_eq!(output, reindented);
    assert_eq!(format_with_statistics(&output, config).0, reindented);
}

#[test]
fn trailing_flow_commas_are_dropped_on_one_line_and_across_lines() {
    let (output, statistics) = format_with_statistics(
        "a: [one, two ,]\nb: {c: 1, }\nd: [\n  e, # note\n]\n",
        "[format]\n",
    );
    assert_eq!(output, "a: [one, two]\nb: {c: 1}\nd: [\n  e  # note\n ]\n");
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
}

#[test]
fn spacing_keeps_the_separation_an_empty_explicit_key_needs() {
    let (output, statistics) =
        format_with_statistics("a:  1\nb: [ ?   , ? ]\nc: {?  , d: 1}\n", "[format]\n");
    assert_eq!(output, "a: 1\nb: [? , ? ]\nc: {? , d: 1}\n");
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
}

#[test]
fn an_unseparated_flow_key_is_reported_then_repaired() {
    let (status, _, diagnostics, _) = common::cli::ryl_on(
        Some("[lint.rules.anchors]\n"),
        "k: [?, ?]\n",
        &["check", "--no-cache"],
    );
    assert_eq!(status, 1, "{diagnostics}");
    assert!(
        diagnostics.contains("help: ryl check --fix or ryl format can repair it"),
        "{diagnostics}"
    );
    let (output, statistics) = format_with_statistics("k: [?, ?]\n", "[format]\n");
    assert_eq!(output, "k: [? , ? ]\n");
    assert_eq!(statistics["fallback"], Value::Null, "{statistics}");
    assert_eq!(format_with_statistics(&output, "[format]\n").0, output);
}
