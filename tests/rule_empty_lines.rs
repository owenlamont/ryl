use ryl::config::YamlLintConfig;
use ryl::rules::empty_lines::{self, Config};

fn resolve(contents: &str) -> Config {
    let cfg = YamlLintConfig::from_yaml_str(contents).expect("config parses");
    Config::resolve(&cfg)
}

#[test]
fn default_allows_two_blank_lines() {
    let cfg = resolve("rules:\n  empty-lines: enable\n");
    let ok = "key: value\n\n\nnext: item\n";
    let violations = empty_lines::check(ok, &cfg);
    assert!(
        violations.is_empty(),
        "unexpected diagnostics: {violations:?}"
    );

    let bad = "key: value\n\n\n\nnext: item\n";
    let hits = empty_lines::check(bad, &cfg);
    assert_eq!(hits.len(), 1);
    let hit = &hits[0];
    assert_eq!(hit.line, 4);
    assert_eq!(hit.column, 1);
    assert_eq!(hit.message, "too many blank lines (3 > 2)");
}

#[test]
fn exceeds_max_reports_violation() {
    let cfg = resolve(
        "rules:\n  empty-lines:\n    max: 0\n    max-start: 0\n    max-end: 0\n",
    );
    let input = "---\nvalue: 1\n\nother: 2\n";
    let hits = empty_lines::check(input, &cfg);
    assert_eq!(hits.len(), 1);
    let hit = &hits[0];
    assert_eq!(hit.line, 3);
    assert_eq!(hit.message, "too many blank lines (1 > 0)");
}

#[test]
fn start_limit_applied_before_general_max() {
    let cfg = resolve(
        "rules:\n  empty-lines:\n    max: 5\n    max-start: 1\n    max-end: 0\n",
    );
    let input = "\n\nkey: value\n";
    let hits = empty_lines::check(input, &cfg);
    assert_eq!(hits.len(), 1);
    let hit = &hits[0];
    assert_eq!(hit.line, 2);
    assert_eq!(hit.message, "too many blank lines (2 > 1)");
}

#[test]
fn end_limit_overrides_general_max() {
    let cfg = resolve(
        "rules:\n  empty-lines:\n    max: 5\n    max-start: 0\n    max-end: 1\n",
    );
    let input = "key: value\n\n\n";
    let hits = empty_lines::check(input, &cfg);
    assert_eq!(hits.len(), 1);
    let hit = &hits[0];
    assert_eq!(hit.line, 3);
    assert_eq!(hit.message, "too many blank lines (2 > 1)");
}

#[test]
fn single_newline_file_is_ignored() {
    let cfg = resolve(
        "rules:\n  empty-lines:\n    max: 0\n    max-start: 0\n    max-end: 0\n",
    );
    let hits = empty_lines::check("\n", &cfg);
    assert!(
        hits.is_empty(),
        "single newline should not produce violations"
    );

    let crlf_hits = empty_lines::check("\r\n", &cfg);
    assert!(
        crlf_hits.is_empty(),
        "single CRLF newline should not produce violations"
    );
}

#[test]
fn space_only_lines_are_not_blank() {
    let cfg = resolve(
        "rules:\n  empty-lines:\n    max: 0\n    max-start: 0\n    max-end: 0\n",
    );
    let input = "---\nintro\n \nend\n";
    let hits = empty_lines::check(input, &cfg);
    assert!(
        hits.is_empty(),
        "space-only lines should not be treated as blank"
    );
}

#[test]
fn fix_keeps_blank_lines_that_belong_to_a_block_scalar() {
    let cfg = resolve("rules:\n  empty-lines: {max: 0, max-start: 0, max-end: 0}\n");
    for input in [
        "a: |+\n\n\n\nb: 1\n",
        "a: |\n\n\n\n  x\n",
        "a: >-\n\n\n  x\n",
        "a: >+\n\n\nb: 1\n",
        "a: !!str |+\n\n\nb: 1\n",
        "- |+\n\n\n- y\n",
        "|+\n\n\n",
        "a: |+\n\n\n",
        "a: |+\n\n# c\nb: 1\n",
        "|\n\n  x\n",
        ">-\n\n\n  x\n",
        "--- |\n\n  x\n",
        "--- !!str >+\n\n\n",
        "|\n\n  x\n...\n--- |\n\n  y\n",
    ] {
        for newline in ["\n", "\r\n"] {
            let input = input.replace('\n', newline);
            assert_eq!(empty_lines::fix(&input, &cfg), None, "{input:?}");
        }
    }
    assert_eq!(
        empty_lines::fix("a: |\n\n  x\nb: 1\n\nc: 2\n", &cfg),
        Some("a: |\n\n  x\nb: 1\nc: 2\n".to_string())
    );
    assert_eq!(
        empty_lines::fix("\n\n|+\n\n", &cfg),
        Some("|+\n\n".to_string())
    );
    assert_eq!(
        empty_lines::fix("a: |+\n\n# c\n\nb: 1\n", &cfg),
        Some("a: |+\n\n# c\nb: 1\n".to_string())
    );
    assert_eq!(
        empty_lines::fix("\n\n|\n\n  x\n", &cfg),
        Some("|\n\n  x\n".to_string())
    );
    assert_eq!(
        empty_lines::fix("a:\n\n  x\n", &cfg),
        Some("a:\n  x\n".to_string())
    );
}

#[test]
fn fix_trims_only_chomped_block_scalar_tails() {
    for header in ["|", ">", "|-", ">-", "|+", ">+", "|2-", ">-2", "|2+", ">+2"] {
        for body in ["", "  café\n", "\n  café\n\n  fin\n"] {
            for suffix in ["", "b: 1\n", "# after\nb: 1\n", "  b: 1\n"] {
                let prefix = if suffix == "  b: 1\n" {
                    "root:\n  a: "
                } else {
                    "a: "
                };
                let body = if suffix == "  b: 1\n" {
                    body.replace("  ", "    ")
                } else {
                    body.to_string()
                };
                let cfg = Config::new(1, 0, 0);
                let tail = if header.contains('+') {
                    "\n\n\n"
                } else if suffix.is_empty() {
                    ""
                } else {
                    "\n"
                };
                for newline in ["\n", "\r\n", "\r"] {
                    let input =
                        format!("{prefix}{header} # header\n{body}\n\n\n{suffix}")
                            .replace('\n', newline);
                    let expected =
                        format!("{prefix}{header} # header\n{body}{tail}{suffix}")
                            .replace('\n', newline);
                    let fixed =
                        empty_lines::fix(&input, &cfg).unwrap_or_else(|| input.clone());
                    assert_eq!(fixed, expected, "{input:?}");
                    assert_eq!(
                        ryl::yaml_dom::YamlOwned::load_from_str(&input).unwrap(),
                        ryl::yaml_dom::YamlOwned::load_from_str(&fixed).unwrap(),
                        "{input:?}"
                    );
                    assert_eq!(empty_lines::fix(&fixed, &cfg), None);
                    assert!(
                        header.contains('+')
                            || empty_lines::check(&fixed, &cfg).is_empty(),
                        "{fixed:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn chomped_tails_preserve_whitespace_content_and_document_boundaries() {
    let cfg = Config::new(0, 0, 0);
    for (input, expected) in [
        ("|\n  café\n\n\n", "|\n  café\n"),
        ("|-\n   \n\n\n", "|-\n   \n"),
        ("|\n  café\n    \n\n\n", "|\n  café\n    \n"),
        ("a: | # +\n  café\n\n", "a: | # +\n  café\n"),
        ("'key|+': |\n  café\n\n", "'key|+': |\n  café\n"),
        (
            "--- |\n  café\n\n...\n--- >-\n  fin\n\n",
            "--- |\n  café\n...\n--- >-\n  fin\n",
        ),
        ("a: |\n  café", "a: |\n  café"),
        ("a: |\n  ", "a: |\n  "),
    ] {
        let output = empty_lines::fix(input, &cfg).unwrap_or_else(|| input.to_string());
        assert_eq!(output, expected);
        assert_eq!(
            ryl::yaml_dom::YamlOwned::load_from_str(input).unwrap(),
            ryl::yaml_dom::YamlOwned::load_from_str(&output).unwrap()
        );
    }
}
