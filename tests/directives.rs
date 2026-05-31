//! Inline rule-disable directive engine: check-path filtering plus direct coverage
//! of the public [`ryl::directives::Directives`] API (parse / is_disabled /
//! disables_any / reconcile). `--fix` reconciliation lives in `directives_fix.rs`,
//! yamllint parity in `yamllint_compat_directives.rs`, and end-to-end stdin/markdown
//! in `cli_directives.rs`.

use std::path::Path;

use ryl::config::YamlLintConfig;
use ryl::directives::Directives;
use ryl::lint::lint_str;

fn cfg(yaml: &str) -> YamlLintConfig {
    YamlLintConfig::from_yaml_str(yaml).expect("config parses")
}

/// `(line, rule)` for every diagnostic, in report order.
fn rule_lines(content: &str, config: &YamlLintConfig) -> Vec<(usize, String)> {
    lint_str(content, Path::new("in.yaml"), config, Path::new("."))
        .into_iter()
        .filter_map(|problem| problem.rule.map(|rule| (problem.line, rule.to_owned())))
        .collect()
}

const COLONS: &str = "rules:\n  colons: enable\n";

#[test]
fn inline_disable_line_suppresses_each_rule_for_both_spellings() {
    // (rule, line that triggers exactly that rule when the rule is enabled alone)
    let triggers = [
        ("colons", "a:  1"),
        ("commas", "[1,  2]"),
        ("braces", "{ a: 1 }"),
        ("brackets", "[ 1 ]"),
        ("truthy", "a: yes"),
        (
            "line-length",
            "key: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ),
    ];
    for (rule, trigger) in triggers {
        let config = cfg(&format!("rules:\n  {rule}: enable\n"));
        assert!(
            !rule_lines(&format!("{trigger}\n"), &config).is_empty(),
            "control: {rule} should fire on its trigger line"
        );
        for keyword in ["ryl", "yamllint"] {
            let line = format!("{trigger}  # {keyword} disable-line rule:{rule}\n");
            assert!(
                rule_lines(&line, &config).is_empty(),
                "inline `# {keyword} disable-line rule:{rule}` should suppress {rule}"
            );
        }
    }
}

#[test]
fn own_line_disable_line_targets_the_next_line_only() {
    let config = cfg(COLONS);
    let input = "# ryl disable-line rule:colons\na:  1\nb:  2\n";
    assert_eq!(
        rule_lines(input, &config),
        vec![(3, "colons".to_owned())],
        "own-line directive disables line 2 only; line 3 still fires"
    );
}

#[test]
fn bare_disable_line_suppresses_every_rule_on_the_target_line() {
    let config = cfg("rules:\n  colons: enable\n  truthy: enable\n");
    let inline = "a:  yes  # ryl disable-line\n";
    assert!(
        rule_lines(inline, &config).is_empty(),
        "bare inline disable-line drops all rules on its line"
    );
    let own_line = "# yamllint disable-line\na:  yes\n";
    assert!(
        rule_lines(own_line, &config).is_empty(),
        "bare own-line disable-line drops all rules on the next line"
    );
}

#[test]
fn block_disable_enable_brackets_a_region() {
    let config = cfg(COLONS);
    let input = "# ryl disable rule:colons\na:  1\nb:  2\n\
        # ryl enable rule:colons\nc:  3\n";
    assert_eq!(
        rule_lines(input, &config),
        vec![(5, "colons".to_owned())],
        "only the line after `enable` should fire"
    );
}

#[test]
fn bare_block_disable_suppresses_all_rules_until_enabled() {
    let config = cfg("rules:\n  colons: enable\n  truthy: enable\n");
    let input = "# ryl disable\na:  yes\n# ryl enable\nb:  no\n";
    assert_eq!(
        rule_lines(input, &config)
            .iter()
            .map(|(line, _)| *line)
            .collect::<Vec<_>>(),
        vec![4, 4],
        "bare disable mutes everything; bare enable restores it on line 4"
    );
}

#[test]
fn disable_then_enable_specific_rule_leaves_others_disabled() {
    let config = cfg("rules:\n  colons: enable\n  truthy: enable\n");
    // disable-all, then re-enable only colons: truthy stays muted, colons fires.
    let input = "# ryl disable\n# ryl enable rule:colons\na:  yes\n";
    assert_eq!(
        rule_lines(input, &config),
        vec![(3, "colons".to_owned())],
        "colons re-enabled, truthy still disabled"
    );
}

#[test]
fn multiple_rule_tokens_disable_each_listed_rule() {
    let config = cfg("rules:\n  colons: enable\n  truthy: enable\n");
    let input = "a:  yes  # ryl disable-line rule:colons rule:truthy\n";
    assert!(
        rule_lines(input, &config).is_empty(),
        "both listed rules should be suppressed"
    );
}

#[test]
fn directive_for_other_rule_does_not_suppress_this_rule() {
    let config = cfg(COLONS);
    let input = "a:  1  # ryl disable-line rule:truthy\n";
    assert_eq!(
        rule_lines(input, &config),
        vec![(1, "colons".to_owned())],
        "disabling an unrelated rule must not affect colons"
    );
}

#[test]
fn unknown_rule_token_is_inert() {
    let config = cfg(COLONS);
    let input = "a:  1  # ryl disable-line rule:does-not-exist\n";
    assert_eq!(
        rule_lines(input, &config),
        vec![(1, "colons".to_owned())],
        "an unknown rule id disables nothing"
    );
}

#[test]
fn strict_grammar_rejects_malformed_directives() {
    let config = cfg(COLONS);
    // Each of these is NOT a valid directive (mirrors yamllint's exact regexes), so
    // colons must still fire on the line.
    let rejected = [
        "a:  1  #   ryl disable-line rule:colons\n", // extra spaces after `#`
        "a:  1  # ryl disable-line colons\n",        // missing `rule:` prefix
        "a:  1  ## ryl disable-line rule:colons\n",  // double hash
        "a:  1  # rylx disable-line rule:colons\n",  // unknown keyword
        "a:  1  # ryl disable-line rule:colons extra\n", // trailing junk
    ];
    for input in rejected {
        assert_eq!(
            rule_lines(input, &config),
            vec![(1, "colons".to_owned())],
            "malformed directive must not suppress: {input:?}"
        );
    }
}

#[test]
fn hash_inside_quotes_is_not_a_directive() {
    let config = cfg(COLONS);
    // Both quote styles: the `#` is part of the scalar, not a comment, so colons
    // (the 2 spaces after the key colon) still fires and is not suppressed.
    for input in [
        "a:  \"# ryl disable-line rule:colons\"\n",
        "a:  '# ryl disable-line rule:colons'\n",
    ] {
        assert_eq!(
            rule_lines(input, &config),
            vec![(1, "colons".to_owned())],
            "a `#` inside a quoted scalar is not a comment: {input:?}"
        );
    }
}

#[test]
fn disable_file_suppresses_entire_file_including_syntax_errors() {
    let config = cfg(COLONS);
    for keyword in ["ryl", "yamllint"] {
        let input = format!("# {keyword} disable-file\na:  1\nb: [1\n");
        assert!(
            lint_str(&input, Path::new("in.yaml"), &config, Path::new(".")).is_empty(),
            "`# {keyword} disable-file` must suppress every diagnostic"
        );
    }
}

#[test]
fn disable_file_is_lenient_about_spacing_after_hash() {
    let config = cfg(COLONS);
    for first in [
        "#ryl disable-file",
        "#   yamllint disable-file",
        "# ryl disable-file  ",
    ] {
        let input = format!("{first}\na:  1\n");
        assert!(
            lint_str(&input, Path::new("in.yaml"), &config, Path::new(".")).is_empty(),
            "lenient disable-file should be honoured: {first:?}"
        );
    }
}

#[test]
fn disable_file_must_be_first_line_and_exact() {
    let config = cfg(COLONS);
    let rejected = [
        "a:  1\n# ryl disable-file\n", // not the first line
        "# ryl disable-file rule:colons\na:  1\n", // trailing junk
        "## ryl disable-file\na:  1\n", // double hash
    ];
    for input in rejected {
        assert!(
            !lint_str(input, Path::new("in.yaml"), &config, Path::new(".")).is_empty(),
            "must not suppress: {input:?}"
        );
    }
}

#[test]
fn disables_file_handles_empty_and_plain_buffers() {
    assert!(!ryl::directives::disables_file(""));
    assert!(!ryl::directives::disables_file("a: 1\n"));
    assert!(ryl::directives::disables_file("# ryl disable-file"));
}

#[test]
fn syntax_errors_are_never_suppressed() {
    let config = cfg(COLONS);
    let problems = lint_str(
        "# ryl disable\na: [1\n",
        Path::new("in.yaml"),
        &config,
        Path::new("."),
    );
    assert_eq!(problems.len(), 1, "syntax error survives a bare disable");
    assert_eq!(
        problems[0].rule, None,
        "the surviving problem is the syntax error"
    );
}

#[test]
fn directives_apply_inside_embedded_markdown_region() {
    let config = cfg("rules:\n  colons: enable\n");
    let markdown =
        "intro\n\n```yaml\na:  1  # ryl disable-line rule:colons\nb:  2\n```\n";
    let problems =
        ryl::lint_markdown_str(markdown, Path::new("doc.md"), &config, Path::new("."));
    let lines: Vec<usize> = problems
        .iter()
        .filter(|p| p.rule == Some("colons"))
        .map(|p| p.line)
        .collect();
    // The fenced block starts at markdown line 3, so its YAML line 1 (the disabled
    // one) maps to line 4 and line 2 (`b:  2`) to line 5.
    assert_eq!(
        lines,
        vec![5],
        "directive suppresses the region-local line only"
    );
}

#[test]
fn all_rule_ids_are_unique() {
    // Completeness (ALL_RULE_IDS == the real rule set) is guarded in property_check.rs
    // via the RULE_TRIGGERS cross-check; here we only assert there are no duplicates.
    let ids = ryl::rules::ALL_RULE_IDS;
    let mut sorted = ids.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), ids.len(), "rule ids must be unique");
}

// --- direct API coverage -------------------------------------------------------

#[test]
fn is_disabled_and_disables_any_without_directives() {
    let directives = Directives::parse("a: 1\nb: 2\n");
    assert!(!directives.is_disabled("colons", 1));
    assert!(!directives.disables_any("colons"));
}

#[test]
fn disables_any_reports_block_and_line_scopes() {
    let block = Directives::parse("# ryl disable rule:colons\na: 1\n");
    assert!(block.disables_any("colons"));
    assert!(!block.disables_any("commas"));

    let line = Directives::parse("a: 1  # ryl disable-line rule:commas\n");
    assert!(line.disables_any("commas"));
    assert!(!line.disables_any("colons"));
}

#[test]
fn reconcile_replace_keeps_disabled_lines() {
    // own-line directive on line 1 disables line 2 for colons.
    let directives = Directives::parse("# ryl disable-line rule:colons\n");
    let before = "keep\nDIS\nkeep\n";
    let after = "KEEP\nfix\nKEEP\n";
    assert_eq!(
        directives.reconcile("colons", before, after),
        "KEEP\nDIS\nKEEP\n",
        "line 2 (disabled) keeps its original text; others take the fix"
    );
}

#[test]
fn reconcile_reverts_deletions_and_drops_insertions_on_disabled_lines() {
    // Block-disable colons on lines 1..=2, enable from line 3 on.
    let directives = Directives::parse(
        "# ryl disable rule:colons\nx\n# ryl enable rule:colons\ny\n",
    );
    // Deletion case (before longer): before line 2 disabled -> kept; line 4 dropped.
    let before = "a\nb\nc\nd\n";
    let after = "a\nc\n";
    assert_eq!(
        directives.reconcile("colons", before, after),
        "a\nb\nc\n",
        "disabled deletion reverted, enabled deletion applied"
    );
}

#[test]
fn reconcile_drops_insertion_anchored_to_disabled_line() {
    let directives = Directives::parse("# ryl disable rule:colons\n");
    // Insertion before line 1 (disabled) is dropped; trailing insertion anchored to
    // the last line is governed by that line's state.
    let before = "a\n";
    let after = "INS\na\n";
    assert_eq!(
        directives.reconcile("colons", before, after),
        "a\n",
        "insertion anchored to a disabled line is dropped"
    );
}

#[test]
fn reconcile_keeps_insertion_anchored_to_enabled_line() {
    // own-line directive on line 1 disables line 2 only, so line 1 stays enabled.
    let directives = Directives::parse("# ryl disable-line rule:colons\n");
    assert_eq!(
        directives.reconcile("colons", "a\nb\n", "INS\na\nb\n"),
        "INS\na\nb\n",
        "an insertion anchored to an enabled line is emitted"
    );
}

#[test]
fn reconcile_applies_deletions_on_enabled_lines() {
    // own-line directive on line 2 disables line 3, leaving line 2 enabled.
    let directives = Directives::parse("x\n# ryl disable-line rule:colons\n");
    assert_eq!(
        directives.reconcile("colons", "a\nDEL\nb\n", "a\nb\n"),
        "a\nb\n",
        "deleting an enabled line is applied"
    );
}

#[test]
fn reconcile_with_empty_before_emits_all_insertions() {
    let directives = Directives::parse("# ryl disable rule:colons\n");
    assert_eq!(
        directives.reconcile("colons", "", "x\ny\n"),
        "x\ny\n",
        "nothing to anchor against, so every inserted line is emitted"
    );
}
