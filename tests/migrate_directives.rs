use ryl::directives::{
    Directives, FormatterComments, disables_file, legacy_directive_lines,
    migrate_comments, migrate_legacy_comments, migrate_markdown_comments,
};

#[test]
fn markdown_conversion_preserves_host_and_embedded_indentation() {
    for input in [
        "---\n# prettier-ignore\nheld:    [1,2]\n---\n\nprose\n",
        "prose\n\n```yaml\nbefore: 0\n# prettier-ignore\nheld:    [1,2]\n```\n",
        "  ```yaml\r\n  before: 0\r\n  # prettier-ignore\r\n  held:    [1,2]\r\n  ```\r\n",
    ] {
        let migration = migrate_markdown_comments(input, FormatterComments::Prettier);
        assert_eq!(
            migration.output,
            Some(input.replace("prettier-ignore", "ryl format-ignore")),
            "{input:?}"
        );
        assert!(migration.retained_lines.is_empty());
    }
}

#[test]
fn markdown_conversion_keeps_unsupported_formatter_contexts_but_converts_aliases() {
    for (opening, source, marker) in [
        ("YAML", FormatterComments::Prettier, "prettier-ignore"),
        ("{.yaml}", FormatterComments::Prettier, "prettier-ignore"),
        (
            "yaml",
            FormatterComments::PrettyYaml("pretty-yaml-ignore"),
            "pretty-yaml-ignore",
        ),
        ("yaml", FormatterComments::Yamlfmt, "!yamlfmt!:ignore"),
    ] {
        let input = format!(
            "prose yamllint disable\n\n```{opening}\n# {marker}\nheld:    [1,2]\n# yamllint disable rule:truthy\nmode: yes\n```\n"
        );
        let migration = migrate_markdown_comments(&input, source);
        assert_eq!(migration.output, Some(input.replace("# yamllint", "# ryl")));
        assert_eq!(migration.retained_lines, [4]);
    }
}

#[test]
fn markdown_html_ignore_retains_formatter_marker_and_reports_host_lines() {
    let input = "<!-- prettier-ignore -->\n```yaml\n# prettier-ignore\nheld:    [1,2]\n# yamllint disable rule:truthy\nmode: yes\n```\n";
    let migration = migrate_markdown_comments(input, FormatterComments::Prettier);
    assert_eq!(migration.output, Some(input.replace("# yamllint", "# ryl")));
    assert_eq!(migration.retained_lines, [1, 3]);
    assert!(
        migrate_markdown_comments(
            "```yaml\r# yamllint disable\rheld: 1\r```\r",
            FormatterComments::None
        )
        .output
        .is_none()
    );
}

#[test]
fn formatter_conversion_keeps_lint_active_and_reports_overlapping_custom_markers() {
    let input = "before: 0\n# prettier-ignore\nheld: yes  \nafter: 1\n";
    let output = migrate_comments(input, FormatterComments::Prettier)
        .output
        .unwrap();
    let config = ryl::config::YamlLintConfig::from_yaml_str(
        "rules:\n  trailing-spaces: enable\n  truthy: enable\n",
    )
    .unwrap();
    let problems = ryl::lint_str(
        &output,
        std::path::Path::new("case.yaml"),
        &config,
        std::path::Path::new("."),
    );
    assert!(
        problems
            .iter()
            .any(|problem| problem.rule == Some("trailing-spaces"))
    );
    assert!(
        problems
            .iter()
            .any(|problem| problem.rule == Some("truthy"))
    );
    let migration = migrate_comments(
        "# yamllint disable\nheld: yes\n",
        FormatterComments::PrettyYaml("yamllint disable"),
    );
    assert!(migration.output.is_none());
    assert_eq!(migration.retained_lines, [1]);
}

#[test]
fn custom_formatter_markers_keep_native_lint_directives_and_file_headers() {
    for (prefix, marker) in [
        ("before: 0\n", "custom # ryl disable rule:truthy"),
        (
            "# ryl disable rule:truthy\nbefore: 0\n",
            "custom # ryl enable rule:truthy",
        ),
        ("before: 0\n", "custom # ryl disable-line rule:truthy"),
        ("", "ryl disable-file"),
    ] {
        let input = format!("{prefix}# {marker}\nheld:    yes\nafter: no\n");
        let migration = migrate_comments(&input, FormatterComments::PrettyYaml(marker));
        assert!(
            migration.output.is_none(),
            "formatter conversion must retain lint behavior: {input:?}"
        );
        assert_eq!(migration.retained_lines.len(), 1);
    }
}

#[test]
fn formatter_conversion_maps_verified_root_pair_and_sequence_attachments() {
    for source in [
        FormatterComments::Prettier,
        FormatterComments::PrettyYaml("pretty-yaml-ignore"),
    ] {
        let marker = if matches!(source, FormatterComments::Prettier) {
            "prettier-ignore"
        } else {
            "pretty-yaml-ignore"
        };
        for body in [
            "# MARKER\nroot:    [1,2]\n",
            "before: [0,0]\n# MARKER\nheld:    [1,2]\nafter: [3,4]\n",
            "- [0,0]\n# MARKER\n-    [1,2]\n- [3,4]\n",
            "root:\n  before: [0,0]\n  # MARKER\n  held:    [1,2]\n  after: [3,4]\n",
        ] {
            let input = body.replace("MARKER", marker);
            let migration = migrate_comments(&input, source);
            assert_eq!(
                migration.output.as_deref(),
                Some(input.replace(marker, "ryl format-ignore").as_str()),
                "{input:?}"
            );
            assert!(migration.retained_lines.is_empty(), "{input:?}");
            assert!(
                migrate_comments(migration.output.as_ref().unwrap(), source)
                    .output
                    .is_none()
            );
        }
    }
}

#[test]
fn prettier_first_nested_collection_and_standalone_value_convert() {
    for input in [
        "root:\n  # prettier-ignore\n  first:    [1,2]\n  second: [3,4]\n",
        "root:\n  # prettier-ignore\n  -    [1,2]\n  - [3,4]\n",
        "root:\n  # prettier-ignore\n  [1,2]\nafter: [3,4]\n",
        "---\n# prettier-ignore\nroot:    [1,2]\n...\n",
    ] {
        assert_eq!(
            migrate_comments(input, FormatterComments::Prettier).output,
            Some(input.replace("prettier-ignore", "ryl format-ignore")),
            "{input:?}"
        );
    }
}

#[test]
fn pretty_yaml_inert_first_nested_and_post_header_markers_remain() {
    for input in [
        "root:\n  # pretty-yaml-ignore\n  first:    [1,2]\n  second: [3,4]\n",
        "root:\n  # pretty-yaml-ignore\n  -    [1,2]\n",
        "root:\n  # pretty-yaml-ignore\n  [1,2]\n",
        "---\n# pretty-yaml-ignore\nroot:    [1,2]\n",
    ] {
        let migration = migrate_comments(
            input,
            FormatterComments::PrettyYaml("pretty-yaml-ignore"),
        );
        assert!(migration.output.is_none(), "{input:?}");
        assert_eq!(migration.retained_lines.len(), 1);
    }
}

#[test]
fn formatter_conversion_retains_unsafe_scope_and_unsupported_comments() {
    for body in [
        "root:\n  a: [0,0]\n  # MARKER\nafter:    [1,2]\n",
        "# MARKER\n...\n---\nafter: [1,2]\n",
        "...\n# MARKER\n---\nafter: [1,2]\n",
        "# MARKER\n%YAML 1.2\n---\nafter: [1,2]\n",
        "?\n  # MARKER\n  - [1,2]\n:    [3,4]\n",
        "# MARKER\n&held\nroot: [1,2]\n",
        "root: [\n  # MARKER\n  [1,2], [3,4]]\n",
        "# MARKER because\nroot: [1,2]\n",
        "root: [1,2] # MARKER\nafter: [3,4]\n",
        "# MARKER\n# ordinary\nroot: [1,2]\n",
        "# ryl format-enable\n# MARKER\nroot: [1,2]\n",
    ] {
        for (marker, source) in [
            ("prettier-ignore", FormatterComments::Prettier),
            (
                "pretty-yaml-ignore",
                FormatterComments::PrettyYaml("pretty-yaml-ignore"),
            ),
        ] {
            let input = body.replace("MARKER", marker);
            let migration = migrate_comments(&input, source);
            assert!(migration.output.is_none(), "{input:?}");
            assert_eq!(migration.retained_lines.len(), 1, "{input:?}");
        }
    }
}

#[test]
fn formatter_conversion_preserves_blank_lines_endings_and_custom_marker() {
    let input = "# custom-skip\r\n\r\nroot:    [1,2]";
    assert_eq!(
        migrate_comments(input, FormatterComments::PrettyYaml("custom-skip"))
            .output
            .as_deref(),
        Some("# ryl format-ignore\r\n\r\nroot:    [1,2]")
    );
    assert_eq!(
        migrate_comments(
            "# pretty-yaml-ignore\n---\nroot: [1,2]\n",
            FormatterComments::PrettyYaml("pretty-yaml-ignore")
        )
        .output
        .as_deref(),
        Some("# ryl format-ignore\n---\nroot: [1,2]\n")
    );
}

#[test]
fn formatter_orphan_and_initial_header_with_leading_comment_keep_attachment() {
    let result = migrate_comments("# prettier-ignore", FormatterComments::Prettier);
    assert!(result.output.is_none());
    assert_eq!(result.retained_lines, [1]);
    let input = "# ordinary\n# pretty-yaml-ignore\n---\nheld:    [1,2]\n";
    assert_eq!(
        migrate_comments(input, FormatterComments::PrettyYaml("pretty-yaml-ignore"))
            .output,
        Some(input.replace("pretty-yaml-ignore", "ryl format-ignore"))
    );
}

#[test]
fn pretty_yaml_marker_after_empty_implicit_document_is_not_initial_header() {
    for first in ["[]", "{}", "[ {},[] ]"] {
        let input = format!(
            "{first}\n# pretty-yaml-ignore\n---\nheld:    [1,2]\nafter:    [3,4]\n"
        );
        let result = migrate_comments(
            &input,
            FormatterComments::PrettyYaml("pretty-yaml-ignore"),
        );
        assert!(
            result.output.is_none(),
            "later document marker must not become an inert native ignore: {input:?}"
        );
        assert_eq!(result.retained_lines, [2]);
    }
}

#[test]
fn markdown_conversion_handles_multiple_regions_and_inline_html_conservatively() {
    let input = "---\n# prettier-ignore\nfirst:    [1,2]\n---\n\n```yaml\n# prettier-ignore\nsecond:    [3,4]\n```\n";
    assert_eq!(
        migrate_markdown_comments(input, FormatterComments::Prettier).output,
        Some(input.replace("prettier-ignore", "ryl format-ignore"))
    );
    let input = "host <!-- prettier-ignore --> rest\n\n```yaml\n# prettier-ignore\nheld:    [1,2]\n```\n";
    let result = migrate_markdown_comments(input, FormatterComments::Prettier);
    assert!(result.output.is_none());
    assert_eq!(result.retained_lines, [1, 4]);
}

#[test]
fn markdown_region_roundtrip_refusal_preserves_ragged_and_mixed_endings() {
    for input in [
        "```yaml\r\n# yamllint disable\nheld: 1\r\n```\n",
        "  ```yaml\n  # yamllint disable\n \n  held: 1\n  ```\n",
    ] {
        let result = migrate_markdown_comments(input, FormatterComments::Prettier);
        assert!(result.output.is_none());
        assert_eq!(result.retained_lines, [2]);
    }
}

#[test]
fn yamlfmt_conversion_requires_actual_first_line_single_implicit_end_document() {
    for ending in ["\n", "\r\n", "\r"] {
        let input = format!("# !yamlfmt!:ignore{ending}root:    [1,2]");
        assert_eq!(
            migrate_comments(&input, FormatterComments::Yamlfmt).output,
            Some(input.replace("!yamlfmt!:ignore", "ryl format-disable"))
        );
    }
    for input in [
        "root: '# !yamlfmt!:ignore'\n",
        "root: 1\n# !yamlfmt!:ignore\n",
        "# !yamlfmt!:ignore\nroot: 1\n...\n",
        "# !yamlfmt!:ignore\nroot: 1\n---\nroot: 2\n",
        "# !yamlfmt!:ignore\n# ryl format-enable\nroot: 1\n",
    ] {
        assert!(
            migrate_comments(input, FormatterComments::Yamlfmt)
                .output
                .is_none(),
            "{input:?}"
        );
    }
}

fn converted(input: &str, expected: &str, lines: &[usize]) {
    assert_eq!(legacy_directive_lines(input), lines);
    assert_eq!(migrate_legacy_comments(input).as_deref(), Some(expected));
    assert_eq!(migrate_legacy_comments(expected), None);
    assert!(legacy_directive_lines(expected).is_empty());
    assert_eq!(disables_file(input), disables_file(expected));
    let before = Directives::parse(input);
    let after = Directives::parse(expected);
    for line in 1..=input.lines().count() + 2 {
        for rule in ryl::rules::ALL_RULE_IDS {
            assert_eq!(
                before.is_disabled(rule, line),
                after.is_disabled(rule, line),
                "suppression changed for {rule} on line {line}"
            );
        }
    }
}

#[test]
fn legacy_alias_conversion_keeps_blocks_and_line_targets() {
    let input = "# yamllint disable rule:colons\na:  1\n# yamllint enable rule:colons\n# yamllint disable-line rule:truthy\nb: yes\nc: no # yamllint disable-line\n";
    converted(input, &input.replace("yamllint", "ryl"), &[1, 3, 4, 6]);
}

#[test]
fn disable_file_conversion_keeps_first_line_grammar_and_invalid_body() {
    for prefix in ["#yamllint", "# yamllint", "#\t  yamllint"] {
        for ending in ["\n", "\r\n", "\r", ""] {
            let body = if ending.is_empty() { "" } else { "[invalid" };
            let input = format!("{prefix} disable-file{ending}{body}");
            converted(&input, &input.replace("yamllint", "ryl"), &[1]);
        }
    }
}

#[test]
fn legacy_alias_conversion_does_not_activate_shared_comment_segments() {
    for input in [
        "a:  1 # yamllint disable-line # explanation\n",
        "# yamllint disable rule:truthy # ryl enable rule:truthy\na: yes\n",
        "# yamllint disable-file # ryl disable-file\na: yes\n",
        "# explanation # yamllint disable-line\na: yes\n",
    ] {
        assert_eq!(migrate_legacy_comments(input), None, "{input:?}");
        assert!(legacy_directive_lines(input).is_empty());
    }
}

#[test]
fn legacy_alias_conversion_leaves_scalar_lookalikes_and_native_comments() {
    let input = "plain: yamllint disable\nquoted: '# yamllint disable-line'\nblock: |\n  # yamllint disable-file\n  # yamllint disable\n# ryl disable rule:truthy\na: yes\n";
    assert_eq!(migrate_legacy_comments(input), None);
    assert!(legacy_directive_lines(input).is_empty());
}

#[test]
fn legacy_alias_conversion_keeps_unknown_rule_lists_inert() {
    let input = "# yamllint disable rule:unknown\na: yes\n# yamllint disable-line rule:truthy rule:unknown\nb: yes\n";
    converted(input, &input.replace("yamllint", "ryl"), &[1, 3]);
}

#[test]
fn legacy_alias_conversion_uses_byte_spans_after_unicode_and_preserves_eof() {
    let input = "é🦀: yes # yamllint disable-line rule:truthy\r\n# yamllint disable rule:colons\r\na:  1";
    converted(input, &input.replace("yamllint", "ryl"), &[1, 2]);
}

#[test]
fn legacy_alias_conversion_retains_unrecognized_grammar() {
    for input in [
        "#yamllint disable\na: yes\n",
        "#  yamllint disable\na: yes\n",
        "# yamllint disable-line because\na: yes\n",
        "a: yes\n# yamllint disable-file\n",
        "# yamllint disables\na: yes\n",
    ] {
        assert_eq!(migrate_legacy_comments(input), None, "{input:?}");
    }
    let input = "# yamllint disable rule:truthy\na: yes\n# ordinary yamllint enable\n";
    converted(
        input,
        "# ryl disable rule:truthy\na: yes\n# ordinary yamllint enable\n",
        &[1],
    );
}

#[test]
fn pretty_yaml_initial_header_after_yaml_directive_keeps_attachment() {
    let input = "%YAML 1.2\n# pretty-yaml-ignore\n---\nheld:    [1,2]\n";
    assert_eq!(
        migrate_comments(input, FormatterComments::PrettyYaml("pretty-yaml-ignore"))
            .output,
        Some(input.replace("pretty-yaml-ignore", "ryl format-ignore"))
    );
}
