use ryl::config::YamlLintConfig;
use ryl::rules::comments::{self, Config, Violation};

fn build_config(yaml: &str) -> Config {
    let cfg = YamlLintConfig::from_yaml_str(yaml).expect("config parses");
    Config::resolve(&cfg)
}

#[test]
fn missing_space_reports_violation() {
    let resolved = build_config("rules:\n  comments: {}\n");
    let hits = comments::check("#comment\n", &resolved);
    assert_eq!(
        hits,
        vec![Violation {
            line: 1,
            column: 2,
            message: "missing starting space in comment".to_string(),
        }]
    );
}

#[test]
fn allows_hash_art_lines() {
    let resolved = build_config("rules:\n  comments: {}\n");
    let hits = comments::check("########################\n", &resolved);
    assert!(hits.is_empty(), "hash art should be allowed: {hits:?}");
}

#[test]
fn enforces_min_spacing_for_inline_comments() {
    let resolved = build_config(
        "rules:\n  comments:\n    require-starting-space: true\n    min-spaces-from-content: 2\n",
    );
    let hits = comments::check("key: value # comment\n", &resolved);
    assert_eq!(
        hits,
        vec![Violation {
            line: 1,
            column: 12,
            message: "too few spaces before comment: expected 2".to_string(),
        }]
    );
}

#[test]
fn disables_min_spacing_with_negative_one() {
    let resolved = build_config(
        "rules:\n  comments:\n    require-starting-space: true\n    min-spaces-from-content: -1\n",
    );
    let hits = comments::check("key: value # comment\n", &resolved);
    assert!(
        hits.is_empty(),
        "min spacing of -1 should disable the check: {hits:?}"
    );
}

#[test]
fn shebang_respected_when_ignored() {
    let resolved = build_config(
        "rules:\n  comments:\n    require-starting-space: true\n    ignore-shebangs: true\n",
    );
    let hits = comments::check("#!/usr/bin/env foo\n", &resolved);
    assert!(hits.is_empty(), "shebang should be ignored: {hits:?}");
}

#[test]
fn shebang_flagged_when_not_ignored() {
    let resolved = build_config(
        "rules:\n  comments:\n    require-starting-space: true\n    ignore-shebangs: false\n",
    );
    let hits = comments::check("#!/usr/bin/env foo\n", &resolved);
    assert_eq!(
        hits,
        vec![Violation {
            line: 1,
            column: 2,
            message: "missing starting space in comment".to_string(),
        }]
    );
}

#[test]
fn inline_comment_reports_both_issues() {
    let resolved = build_config("rules:\n  comments: {}\n");
    let hits = comments::check("value: foo #bar\n", &resolved);
    assert_eq!(
        hits,
        vec![
            Violation {
                line: 1,
                column: 12,
                message: "too few spaces before comment: expected 2".to_string(),
            },
            Violation {
                line: 1,
                column: 13,
                message: "missing starting space in comment".to_string(),
            },
        ]
    );
}

#[test]
fn url_fragment_does_not_count_as_comment() {
    let resolved = build_config("rules:\n  comments: {}\n");
    let hits = comments::check("link: https://example.com/#anchor\n", &resolved);
    assert!(
        hits.is_empty(),
        "URL fragments should not be treated as comments: {hits:?}"
    );
}

#[test]
fn ignores_hash_characters_inside_quotes() {
    let resolved = build_config("rules:\n  comments: {}\n");
    let hits = comments::check("string: \"value #not comment\" # comment\n", &resolved);
    assert_eq!(
        hits,
        vec![Violation {
            line: 1,
            column: 30,
            message: "too few spaces before comment: expected 2".to_string(),
        }]
    );
}

#[test]
fn ignores_hash_inside_single_quotes() {
    let resolved = build_config("rules:\n  comments: {}\n");
    let hits = comments::check("string: 'value #not comment' # comment\n", &resolved);
    assert_eq!(
        hits,
        vec![Violation {
            line: 1,
            column: 30,
            message: "too few spaces before comment: expected 2".to_string(),
        }]
    );
}

#[test]
fn handles_escaped_hash_inside_double_quotes() {
    let resolved = build_config("rules:\n  comments: {}\n");
    let hits = comments::check("string: \"path\\#not comment\" # comment\n", &resolved);
    assert_eq!(
        hits,
        vec![Violation {
            line: 1,
            column: 29,
            message: "too few spaces before comment: expected 2".to_string(),
        }]
    );
}

#[test]
fn handles_crlf_newlines() {
    let resolved = build_config("rules:\n  comments: {}\n");
    let hits = comments::check("key: value # comment\r\n#comment\r\n", &resolved);
    assert_eq!(hits.len(), 2, "expected two violations: {hits:?}");
    assert_eq!(hits[0].line, 1);
    assert_eq!(hits[1].line, 2);
}

#[test]
fn multiline_quoted_scalars_ignore_hashes() {
    let resolved = build_config("rules:\n  comments: {}\n");
    let input = "value: \"first line\\n  second # still scalar\\n  third\"\n";
    let hits = comments::check(input, &resolved);
    assert!(
        hits.is_empty(),
        "quoted scalar hash should not be comment: {hits:?}"
    );
}

#[test]
fn block_scalar_lines_do_not_trigger_comment_checks() {
    let resolved = build_config("rules:\n  comments: {}\n");
    let input = "job:\n  run: |\n    line one\n   line two\n    line three\n  next: value\n";
    let hits = comments::check(input, &resolved);
    assert!(
        hits.is_empty(),
        "block scalar content must be ignored: {hits:?}"
    );
}

#[test]
fn empty_block_scalar_is_ignored() {
    let resolved = build_config("rules:\n  comments: {}\n");
    let input = "job:\n  run: |\n  next: value\n";
    let hits = comments::check(input, &resolved);
    assert!(
        hits.is_empty(),
        "empty block scalar should reset tracker: {hits:?}"
    );
}
