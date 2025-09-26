use ryl::config::YamlLintConfig;
use ryl::rules::line_length::{self, Config, Violation};

fn build_config(yaml: &str) -> Config {
    let cfg = YamlLintConfig::from_yaml_str(yaml).expect("config parses");
    Config::resolve(&cfg)
}

#[test]
fn flags_lines_longer_than_max() {
    let resolved = build_config("rules:\n  line-length: {max: 10}\n");
    let input = "key: alpha beta gamma\n";
    let hits = line_length::check(input, &resolved);
    assert_eq!(
        hits,
        vec![Violation {
            line: 1,
            column: 11,
            message: format!(
                "line too long ({} > {} characters)",
                input.trim_end_matches(['\n']).chars().count(),
                10
            ),
        }]
    );
}

#[test]
fn allows_long_single_word_by_default() {
    let resolved = build_config("rules:\n  line-length: {max: 20}\n");
    let input = format!("{}\n", "A".repeat(40));
    let hits = line_length::check(&input, &resolved);
    assert!(
        hits.is_empty(),
        "non-breakable words should be allowed by default: {hits:?}"
    );
}

#[test]
fn respects_allow_non_breakable_words_flag() {
    let resolved =
        build_config("rules:\n  line-length:\n    max: 20\n    allow-non-breakable-words: false\n");
    let input = format!("{}\n", "B".repeat(40));
    let hits = line_length::check(&input, &resolved);
    assert_eq!(hits.len(), 1, "long words should be flagged when disabled");
    assert_eq!(hits[0].line, 1);
    assert_eq!(hits[0].column, 21);
    assert!(
        hits[0].message.starts_with("line too long"),
        "unexpected message: {}",
        hits[0].message
    );
}

#[test]
fn allows_inline_mappings_when_requested() {
    let strict =
        build_config("rules:\n  line-length:\n    max: 20\n    allow-non-breakable-words: false\n");
    let inline_allowed = build_config(
        "rules:\n  line-length:\n    max: 20\n    allow-non-breakable-inline-mappings: true\n",
    );
    let mapping = "url: http://localhost/very/very/long/path\n";

    let strict_hits = line_length::check(mapping, &strict);
    assert_eq!(
        strict_hits.len(),
        1,
        "mapping should fail without inline support"
    );

    let inline_hits = line_length::check(mapping, &inline_allowed);
    assert!(
        inline_hits.is_empty(),
        "inline mapping should pass when allowed"
    );
}

#[test]
fn inline_mappings_still_fail_when_value_contains_spaces() {
    let resolved = build_config(
        "rules:\n  line-length:\n    max: 20\n    allow-non-breakable-inline-mappings: true\n",
    );
    let mapping = "url: http://localhost/short + extra words\n";
    let hits = line_length::check(mapping, &resolved);
    assert_eq!(
        hits.len(),
        1,
        "spaces should prevent inline mapping exemption"
    );
    assert_eq!(hits[0].line, 1);
    assert_eq!(hits[0].column, 21);
}

#[test]
fn handles_comment_prefixes_like_yamllint() {
    let resolved =
        build_config("rules:\n  line-length:\n    max: 20\n    allow-non-breakable-words: true\n");
    let allowed = "## http://example.com/super/long/url/with/no/spaces\n";
    let disallowed = "# # http://example.com/super/long/url/with/no/spaces\n";

    let allowed_hits = line_length::check(allowed, &resolved);
    assert!(allowed_hits.is_empty(), "double hash comments should pass");

    let disallowed_hits = line_length::check(disallowed, &resolved);
    assert_eq!(disallowed_hits.len(), 1, "spaced hash comments should fail");
}

#[test]
fn inline_option_implies_non_breakable_words() {
    let resolved = build_config(
        "rules:\n  line-length:\n    max: 20\n    allow-non-breakable-inline-mappings: true\n",
    );
    let word = format!("{}\n", "C".repeat(40));
    let hits = line_length::check(&word, &resolved);
    assert!(
        hits.is_empty(),
        "inline flag should imply allow words: {hits:?}"
    );
}

#[test]
fn diagnostic_column_handles_negative_max() {
    let resolved =
        build_config("rules:\n  line-length:\n    max: -1\n    allow-non-breakable-words: false\n");
    let hits = line_length::check("abc\n", &resolved);
    assert_eq!(
        hits.len(),
        1,
        "negative max should still produce diagnostics"
    );
    assert_eq!(hits[0].column, 0, "column should clamp to zero");
}

#[test]
fn crlf_lines_are_checked() {
    let resolved =
        build_config("rules:\n  line-length:\n    max: 3\n    allow-non-breakable-words: false\n");
    let hits = line_length::check("AAAA\r\n", &resolved);
    assert_eq!(
        hits.len(),
        1,
        "CRLF line should be counted with carriage return trimmed"
    );
    assert_eq!(hits[0].line, 1);
    assert_eq!(hits[0].column, 4);
}

#[test]
fn spaces_only_lines_flagged() {
    let resolved = build_config("rules:\n  line-length: {max: 2}\n");
    let hits = line_length::check("     \n", &resolved);
    assert_eq!(
        hits.len(),
        1,
        "spaces should not qualify as non-breakable words"
    );
    assert_eq!(hits[0].line, 1);
    assert_eq!(hits[0].column, 3);
}

#[test]
fn list_indicator_allows_long_word() {
    let resolved = build_config("rules:\n  line-length: {max: 5}\n");
    let input = format!("- {}\n", "a".repeat(30));
    let hits = line_length::check(&input, &resolved);
    assert!(
        hits.is_empty(),
        "list item with non-breakable word should be allowed by default"
    );
}

#[test]
fn dash_without_value_still_flags() {
    let resolved = build_config("rules:\n  line-length: {max: 1}\n");
    let hits = line_length::check("- \n", &resolved);
    assert_eq!(
        hits.len(),
        1,
        "dash without content should not bypass limit"
    );
    assert_eq!(hits[0].column, 2);
}

#[test]
fn inline_mapping_guard_handles_plain_text() {
    let resolved = build_config(
        "rules:\n  line-length:\n    max: 10\n    allow-non-breakable-inline-mappings: true\n    allow-non-breakable-words: false\n",
    );
    let hits = line_length::check("plain words here\n", &resolved);
    assert_eq!(
        hits.len(),
        1,
        "non-mapping lines should still report violations"
    );
}
