use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use ryl::directives::{
    Directives, FormatterComments, disables_file, legacy_directive_lines,
    migrate_comments, migrate_legacy_comments,
};

fn changed_words(input: &str, output: &str) -> Result<usize, String> {
    let (input, output) = (input.as_bytes(), output.as_bytes());
    let (mut before, mut after, mut count) = (0, 0, 0);
    while before < input.len() || after < output.len() {
        if input.get(before) == output.get(after) {
            before += 1;
            after += 1;
        } else if input[before..].starts_with(b"yamllint")
            && output[after..].starts_with(b"ryl")
        {
            before += b"yamllint".len();
            after += b"ryl".len();
            count += 1;
        } else {
            return Err(format!("non-directive bytes changed at {before}/{after}"));
        }
    }
    Ok(count)
}

#[test]
fn converter_byte_oracle_rejects_unrelated_edits_and_handles_utf8() {
    assert_eq!(changed_words("", ""), Ok(0));
    assert_eq!(changed_words("é🦀 yamllint", "é🦀 ryl"), Ok(1));
    for (input, output) in [
        ("a", ""),
        ("", "a"),
        ("é", "è"),
        ("yamllint", "ry"),
        ("yaml", "ryl"),
    ] {
        assert!(changed_words(input, output).is_err());
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
            "tests/proptest-regressions/property_migrate_directives.txt",
        ))),
        ..ProptestConfig::default()
    })]

    #[test]
    fn legacy_conversion_preserves_bytes_scope_and_yaml(
        document in crate::property::check_strategy::arb_document_with_enables(),
        prefix in prop::sample::select(vec![
            "",
            "# yamllint disable rule:truthy\n",
            "#yamllint disable-file\r\n",
            "#yamllint disable-file\r",
            "# yamllint disable-line\n",
        ]),
    ) {
        let input = format!("{prefix}{}", document.render());
        let output = migrate_legacy_comments(&input).unwrap_or_else(|| input.clone());
        let count = changed_words(&input, &output).map_err(TestCaseError::fail)?;
        prop_assert_eq!(count, legacy_directive_lines(&input).len(), "input {:?}", input);
        prop_assert_eq!(migrate_legacy_comments(&output), None, "input {:?}", input);
        prop_assert_eq!(disables_file(&input), disables_file(&output), "input {:?}", input);
        prop_assert_eq!(
            crate::property::representation::representation(&input),
            crate::property::representation::representation(&output),
            "input {:?}; output {:?}", input, output
        );
        let before = Directives::parse(&input);
        let after = Directives::parse(&output);
        let lines = crate::property::representation::yaml_lines(&input).len();
        for line in 1..=lines + 2 {
            for rule in ryl::rules::ALL_RULE_IDS {
                prop_assert_eq!(before.is_disabled(rule, line), after.is_disabled(rule, line),
                    "input {:?}; {} on {}", input, rule, line);
            }
        }
    }

    #[test]
    fn formatter_conversion_preserves_node_scope_and_lint(
        pretty in any::<bool>(),
        custom in prop::sample::select(vec!["pretty-yaml-ignore", "custom-skip", "custom # ryl disable rule:truthy", "custom # ryl enable rule:truthy", "custom # ryl disable-line rule:truthy"]),
        atom in "[a-z]{1,12}",
        padding in 2_usize..7,
        ending in prop::sample::select(vec!["\n", "\r\n", "\r"]),
        eof in any::<bool>(),
        nested in any::<bool>(),
    ) {
        let (marker, source) = if pretty {
            (custom, FormatterComments::PrettyYaml(custom))
        } else {
            ("prettier-ignore", FormatterComments::Prettier)
        };
        let indent = if nested { "  " } else { "" };
        let gap = " ".repeat(padding);
        let body = format!("{indent}before:{gap}[0,0]\n{indent}# {marker}\n{indent}held:{gap}[{atom},{atom}]\n{indent}after:{gap}[3,4]");
        let input = format!("{}{}{}", if nested { "root:\n" } else { "" }, body, if eof { "" } else { "\n" }).replace('\n', ending);
        let migration = migrate_comments(&input, source);
        let collision = pretty && custom.contains("# ryl ");
        if collision {
            prop_assert!(migration.output.is_none());
            prop_assert_eq!(migration.retained_lines.len(), 1);
        } else {
            prop_assert!(migration.retained_lines.is_empty(), "input {:?}", input);
            prop_assert_eq!(migration.output.as_ref(), Some(&input.replace(marker, "ryl format-ignore")));
        }
        let output = migration.output.unwrap_or_else(|| input.clone());
        prop_assert!(migrate_comments(&output, source).output.is_none());
        prop_assert_eq!(
            crate::property::representation::representation(&input),
            crate::property::representation::representation(&output),
        );
        let before = Directives::parse(&input);
        let after = Directives::parse(&output);
        for line in 1..=crate::property::representation::yaml_lines(&input).len() + 2 {
            for rule in ryl::rules::ALL_RULE_IDS {
                prop_assert_eq!(before.is_disabled(rule, line), after.is_disabled(rule, line));
            }
        }
        if !collision {
            let config = ryl::config::YamlLintConfig::from_toml_str("[format]").unwrap();
            let path = std::path::Path::new("case.yaml");
            let formatted = ryl::format::format_str(&output, &config, path, &[]);
            prop_assert!(formatted.contains(&format!("{indent}held:{gap}[{atom},{atom}]")), "held node changed: {:?}", formatted);
            prop_assert!(formatted.contains(&format!("{indent}before: [0, 0]")), "before witness unchanged: {:?}", formatted);
            prop_assert!(formatted.contains(&format!("{indent}after: [3, 4]")), "after witness unchanged: {:?}", formatted);
            prop_assert_eq!(ryl::format::format_str(&formatted, &config, path, &[]), formatted);
        }
        if ending != "\r" {
            let markdown = format!("host before{ending}{ending}```yaml{ending}{input}{ending}```{ending}host after");
            let migrated = ryl::directives::migrate_markdown_comments(&markdown, source);
            if pretty {
                prop_assert!(migrated.output.is_none());
                prop_assert_eq!(migrated.retained_lines.len(), 1);
            } else {
                prop_assert_eq!(migrated.output, Some(markdown.replace(marker, "ryl format-ignore")));
                prop_assert!(migrated.retained_lines.is_empty());
            }
        }
    }
}
