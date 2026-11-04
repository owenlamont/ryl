use std::path::Path;

use ryl::config::YamlLintConfig;
use ryl::format::format_str;

fn formats(input: &str, config: &str, expected: &str) {
    let cfg = YamlLintConfig::from_toml_str(config).unwrap();
    let output = format_str(input, &cfg, Path::new("case.yaml"), &[]);
    assert_eq!(output, expected, "input {input:?} under {config:?}");
    crate::property::reference::check_strict_events(
        input.trim_start_matches('\u{feff}'),
        output.trim_start_matches('\u{feff}'),
    )
    .unwrap();
    assert_eq!(
        format_str(&output, &cfg, Path::new("case.yaml"), &[]),
        output
    );
}

#[test]
fn node_ignore_keeps_mapping_entry_and_formats_neighbors() {
    formats(
        "before:    0\n# ryl format-ignore\nsection:\n  first:    1\n  second:    2\nafter:    3\n",
        "[format]\n",
        "before: 0\n# ryl format-ignore\nsection:\n  first:    1\n  second:    2\nafter: 3\n",
    );
}

#[test]
fn region_ignore_is_independent_from_generic_rule_disables() {
    formats(
        "# ryl disable rule:quoted-strings\nbefore:    'hello'\n# ryl format-disable\na:    [1,2]\n# ryl format-enable\nafter:    'world'\n",
        "[format]\n",
        "# ryl disable rule:quoted-strings\nbefore: 'hello'\n# ryl format-disable\na:    [1,2]\n# ryl format-enable\nafter: 'world'\n",
    );
}

#[test]
fn leading_node_ignore_protects_root_mapping() {
    let input = "# ryl format-ignore\na:    1\nb:    2\n";
    formats(input, "[format]\n", input);
}

#[test]
fn node_ignore_covers_flow_collection_and_properties() {
    formats(
        "first: 0\n# ryl format-ignore\na: &anchor !!seq [1,2,3]\nlast:    *anchor\n",
        "[format]\nsequence-style = 'block'\nproperty-order = 'tag-first'\n",
        "first: 0\n# ryl format-ignore\na: &anchor !!seq [1,2,3]\nlast: *anchor\n",
    );
}

#[test]
fn format_directives_inside_scalar_text_are_not_active() {
    formats(
        "a: '# ryl format-ignore'\nb:    1\ntext: |\n  # ryl format-disable\nc:    2\n",
        "[format]\n",
        "a: '# ryl format-ignore'\nb: 1\ntext: |\n  # ryl format-disable\nc: 2\n",
    );
}

#[test]
fn format_ignore_does_not_suppress_lint_or_lint_fixes() {
    let input = "# ryl format-ignore\nmode: yes\nother:    1\n";
    let cfg =
        YamlLintConfig::from_yaml_str("rules:\n  truthy: enable\n  colons: enable\n")
            .unwrap();
    assert!(
        ryl::lint_str(input, Path::new("case.yaml"), &cfg, Path::new("."))
            .iter()
            .any(|problem| problem.rule == Some("truthy"))
    );
    assert_eq!(
        ryl::fix::apply_safe_fixes(input, &cfg, Path::new("case.yaml"), Path::new(".")),
        "# ryl format-ignore\nmode: yes\nother: 1\n",
    );
}

#[test]
fn node_ignore_handles_sequence_items_and_explicit_mapping_keys() {
    formats(
        "items:\n  - first\n  # ryl format-ignore\n  - key:    'hello'\n    other:    [1,2]\nafter:    1\n",
        "[format]\n",
        "items:\n  - first\n  # ryl format-ignore\n  - key:    'hello'\n    other:    [1,2]\nafter: 1\n",
    );
    formats(
        "before: 0\n# ryl format-ignore\n? held\n:    [1,2]\nafter:    1\n",
        "[format]\n",
        "before: 0\n# ryl format-ignore\n? held\n:    [1,2]\nafter: 1\n",
    );
}

#[test]
fn region_disable_ends_at_document_boundary() {
    formats(
        "# ryl format-disable\na:    1\n---\nb:    2\n",
        "[format]\n",
        "# ryl format-disable\na:    1\n---\nb: 2\n",
    );
}

#[test]
fn ignored_end_of_file_keeps_missing_newline_and_crlf() {
    let input = "before:    0\r\n# ryl format-disable\r\na:    [1,2]";
    formats(
        input,
        "[format]\n",
        "before: 0\n# ryl format-disable\r\na:    [1,2]",
    );
}

#[test]
fn format_ignore_recognizes_only_own_line_comments() {
    formats(
        "a:    1 # ryl format-ignore\nb:    2\n",
        "[format]\n",
        "a: 1  # ryl format-ignore\nb: 2\n",
    );
}

#[test]
fn ignored_flow_key_repair_keeps_original_bytes() {
    let input = "# ryl format-disable\nbad: {?[]: 1}\n# ryl format-enable\nafter: 2\n";
    let cfg = YamlLintConfig::from_toml_str("[format]").unwrap();
    assert_eq!(format_str(input, &cfg, Path::new("case.yaml"), &[]), input);
}

#[test]
fn ignored_nested_node_does_not_absorb_dedented_sibling_comment() {
    formats(
        "before:\n# ryl format-ignore\nheld:\n  child:    'hello'\n# next sibling\nafter:    1\n",
        "[format]\n",
        "before:\n# ryl format-ignore\nheld:\n  child:    'hello'\n# next sibling\nafter: 1\n",
    );
}

#[test]
fn repeated_region_disables_do_not_nest_and_unmatched_enable_is_inert() {
    formats(
        "# ryl format-enable\nbefore:    0\n# ryl format-disable\na:    1\n# ryl format-disable\nb:    2\n# ryl format-enable\nafter:    3\n",
        "[format]\n",
        "# ryl format-enable\nbefore: 0\n# ryl format-disable\na:    1\n# ryl format-disable\nb:    2\n# ryl format-enable\nafter: 3\n",
    );
}

#[test]
fn ignore_at_eof_has_no_following_node_to_disable() {
    formats(
        "before:    0\n# ryl format-ignore\n",
        "[format]\n",
        "before: 0\n# ryl format-ignore\n",
    );
}

#[test]
fn ignored_folded_scalar_and_bom_are_preserved() {
    formats(
        "\u{feff}before:    0\n# ryl format-ignore\nheld: >-\n  café words\n  more words\nafter:    1\n",
        "line-length = 5\n[format]\nfold-long-lines = true\n",
        "\u{feff}before: 0\n# ryl format-ignore\nheld: >-\n  café words\n  more words\nafter: 1\n",
    );
}

#[test]
fn node_ignore_does_not_disable_independent_collection_conversions() {
    formats(
        "before: [1,2]\n# ryl format-ignore\nheld:    [3,4]\nafter: [5,6]\n",
        "[format]\nsequence-style = 'block'\n",
        "before:\n  - 1\n  - 2\n# ryl format-ignore\nheld:    [3,4]\nafter:\n  - 5\n  - 6\n",
    );
}

#[test]
fn formatter_suppression_is_local_to_markdown_regions() {
    let input = "Text\n\n```yaml\n# ryl format-disable\nheld:    [1,2]\n```\n\n```yaml\nafter:    1\n```\n";
    let expected = "Text\n\n```yaml\n# ryl format-disable\nheld:    [1,2]\n```\n\n```yaml\nafter: 1\n```\n";
    let cfg = YamlLintConfig::from_toml_str("[format]\n").unwrap();
    let format = |text: &str| {
        ryl::fix::fix_markdown_str(
            text,
            Path::new("case.md"),
            &cfg,
            Path::new("."),
            ryl::fix::Rewrite::Format,
        )
        .unwrap_or_else(|| text.to_owned())
    };
    assert_eq!(format(input), expected);
    assert_eq!(format(expected), expected);
}

#[test]
fn ignored_custom_indentation_keeps_independent_collection_conversions() {
    for (open, close) in [
        ("# ryl format-ignore\n", ""),
        ("# ryl format-disable\n", "# ryl format-enable\n"),
    ] {
        formats(
            &format!(
                "before: [1,2]\n{open}held:\n    a:    [3,4]\n{close}after: [5,6]\n"
            ),
            "[format]\nsequence-style = 'block'\n",
            &format!(
                "before:\n  - 1\n  - 2\n{open}held:\n    a:    [3,4]\n{close}after:\n  - 5\n  - 6\n"
            ),
        );
    }
}

#[test]
fn protected_indentation_keeps_mapping_and_sequence_relationships() {
    for (open, close) in [
        ("# ryl format-ignore\n", ""),
        ("# ryl format-disable\n", "# ryl format-enable\n"),
    ] {
        let close = if close.is_empty() {
            String::new()
        } else {
            format!("    {close}")
        };
        for before in ["before:", "before: 0", "before: |", "before: >"] {
            formats(
                &format!(
                    "outer:\n    {before}\n    {open}    held:    1\n{close}    after:    2\n"
                ),
                "[format]\n",
                &format!(
                    "outer:\n    {before}\n    {open}    held:    1\n{close}    after: 2\n"
                ),
            );
        }
    }
    formats(
        "outer:\n    -\n    # ryl format-ignore\n    -    held\n    -    after\n---\nnext:\n    value:    1\n",
        "indent-width = 2\n[format]\n",
        "outer:\n    -\n    # ryl format-ignore\n    -    held\n    - after\n---\nnext:\n  value: 1\n",
    );
}

#[test]
fn protected_explicit_key_keeps_indentless_conversion_idempotent() {
    formats(
        "before:    0\nconvert: [3,4]\n# ryl format-disable\n? held\n:    'crhqcwspzeoaavvhfofu'\n# ryl format-enable\nafter:    1\n",
        "[format]\nsequence-style = 'block'\nmapping-style = 'flow'\nindent-sequences = false\n",
        "before: 0\nconvert:\n- 3\n- 4\n# ryl format-disable\n? held\n:    'crhqcwspzeoaavvhfofu'\n# ryl format-enable\nafter: 1\n",
    );
}

#[test]
fn empty_and_comment_only_documents_keep_valid_layouts() {
    for (input, expected) in [
        ("\n", ""),
        ("# comment\n", "# comment\n"),
        ("# ryl format-disable\n", "# ryl format-disable\n"),
        ("# ryl format-ignore\n", "# ryl format-ignore\n"),
        (
            "# ryl format-disable \t\r\n#  tail \t\r\n",
            "# ryl format-disable \t\r\n#  tail \t\r\n",
        ),
    ] {
        formats(input, "[format]\n", expected);
    }
    let cfg = YamlLintConfig::from_toml_str("[lint.rules.key-ordering]\n").unwrap();
    assert_eq!(
        ryl::fix::apply_safe_fixes("\n", &cfg, Path::new("case.yaml"), Path::new(".")),
        "\n",
    );
}

#[test]
fn own_line_directives_can_be_separated_from_nodes_by_blank_lines() {
    formats(
        "before:    0\n# ryl format-ignore\n\nheld:    [1,2]\nafter:    1\n",
        "[format]\n",
        "before: 0\n# ryl format-ignore\n\nheld:    [1,2]\nafter: 1\n",
    );
    formats(
        "before:    0\n# ryl format-disable\n\nheld:    [1,2]\n# ryl format-enable\n\n\n\n",
        "[format]\n",
        "before: 0\n# ryl format-disable\n\nheld:    [1,2]\n# ryl format-enable\n",
    );
}

#[test]
fn frozen_source_indentation_keeps_generated_flow_rows_valid() {
    for (open, close) in [
        ("    # ryl format-ignore\n", ""),
        ("    # ryl format-disable\n", "    # ryl format-enable\n"),
    ] {
        for (source, expanded) in [
            ("[3,4]", "[\n      3,\n      4,\n     ]"),
            ("{a: 1,b: 2}", "{\n      a: 1,\n      b: 2,\n     }"),
        ] {
            formats(
                &format!(
                    "root:\n    - before\n{open}    - held\n{close}    - {source}\n"
                ),
                "[format]\nsequence-expand = 'always'\nmapping-expand = 'always'\n",
                &format!(
                    "root:\n    - before\n{open}    - held\n{close}    - {expanded}\n"
                ),
            );
        }
    }
}

#[test]
fn protected_mapping_descendants_prevent_dash_splitting() {
    let input = "root:\n    - before:\n      # ryl format-ignore\n      held:    2\n";
    for width in [1, 4] {
        formats(
            input,
            &format!("indent-width = {width}\n[format]\ndash-on-own-line = true\n"),
            input,
        );
    }
}

#[test]
fn protected_mapping_descendants_prevent_dash_joining() {
    for (open, close) in [
        ("# ryl format-ignore", ""),
        ("# ryl format-disable", "        # ryl format-enable\n"),
    ] {
        let input = format!(
            "root:\n    -\n        before:\n        {open}\n        held:    2\n{close}"
        );
        formats(
            &input,
            "indent-width = 2\n[format]\ndash-on-own-line = false\n",
            &input,
        );
    }
}

#[test]
fn protected_explicit_value_gap_keeps_expanded_sibling_valid() {
    formats(
        "before:    0\nconvert: [3,4]\nouter:\n    sibling:\n    # ryl format-ignore\n    ? held\n    :    'a'\n    sibling_after:    2\nextra:\n  - [5,6]\nafter:    1\n",
        "[format]\nsequence-expand = 'always'\nmapping-expand = 'never'\nbrace-spacing = true\n",
        "before: 0\nconvert:\n  [\n    3,\n    4,\n  ]\nouter:\n    sibling:\n    # ryl format-ignore\n    ? held\n    :    'a'\n    sibling_after: 2\nextra:\n  - [\n    5,\n    6,\n   ]\nafter: 1\n",
    );
}

#[test]
fn scalar_folding_uses_retained_mapping_owner() {
    formats(
        "root:\n    before:\n    # ryl format-ignore\n    held:    2\n    after: long words long words long words\n",
        "line-length = 14\n[format]\nfold-long-lines = true\n",
        "root:\n    before:\n    # ryl format-ignore\n    held:    2\n    after: long\n      words\n      long\n      words\n      long\n      words\n",
    );
    let cfg = YamlLintConfig::from_toml_str("line-length = 14\n[format]\nfold-long-lines = true\nquote-style = 'preserve'\n").unwrap();
    for (open, close) in [
        ("# ryl format-ignore", ""),
        ("# ryl format-disable", "    # ryl format-enable\n"),
    ] {
        for prose in [
            "long words long words long words",
            "'long words long words long words'",
            "\"long words long words long words\"",
            ">-\n      long words long words long words",
        ] {
            for newline in ["\n", "\r\n"] {
                let input = format!("root:\n    before:\n    {open}\n    held:    2\n{close}    after: {prose}\n").replace('\n', newline);
                let protected = format!("    {open}\n    held:    2\n{close}")
                    .replace('\n', newline);
                let output = format_str(&input, &cfg, Path::new("case.yaml"), &[]);
                assert!(output.contains(&protected));
                assert!(output.lines().count() > input.lines().count());
                assert!(
                    output
                        .lines()
                        .skip_while(|line| !line.starts_with("    after:"))
                        .skip(1)
                        .all(|line| line.starts_with("      "))
                );
                crate::property::reference::check_strict_events(&input, &output)
                    .unwrap();
                assert_eq!(
                    format_str(&output, &cfg, Path::new("case.yaml"), &[]),
                    output
                );
            }
        }
    }
}

#[test]
fn converted_collection_owners_keep_existing_folding_output() {
    formats(
        "root: {after: \"long words long words long words\"}\n",
        "line-length = 14\n[format]\nfold-long-lines = true\nmapping-style = 'block'\n",
        "root:\n  after: long\n    words long\n    words long\n    words\n",
    );
    formats(
        "items: [first, 'alpha  beta\tgamma delta epsilon']\n",
        r#"line-length=20
[format]
sequence-style='block'
fold-long-lines=true
quote-style='preserve'
"#,
        "items:\n  - first\n  - 'alpha  beta\tgamma\n    delta epsilon'\n",
    );
    formats(
        r#"map: {value: &text 'é alpha beta'}
*text : {value: 'more words'}
"#,
        r#"indent-width=4
line-length=3
[format]
mapping-style='block'
fold-long-lines=true
quote-style='preserve'
"#,
        r#"map:
    value: &text 'é
        alpha
        beta'
*text :
    value: 'more
        words'
"#,
    );
}

#[test]
fn independent_mapping_shapes_keep_sibling_ownership() {
    let prefix = "root:\n    before:\n    # ryl format-ignore\n    held:    2\n";
    for value in ["", " 0"] {
        for (own_line, source, expected) in [
            (
                true,
                format!("items:\n  - k:{value}\n    tail: 1\n"),
                format!("items:\n-\n k:{value}\n tail: 1\n"),
            ),
            (
                false,
                format!("items:\n  -    k:{value}\n       tail: 1\n"),
                format!("items:\n- k:{value}\n  tail: 1\n"),
            ),
            (
                false,
                format!("items:\n  -\n      k:{value}\n      tail: 1\n"),
                format!("items:\n- k:{value}\n  tail: 1\n"),
            ),
        ] {
            formats(
                &format!("{prefix}{source}"),
                &format!(
                    "indent-width=1\n[format]\ndash-on-own-line={own_line}\nindent-sequences=false\n"
                ),
                &format!("{prefix}{expected}"),
            );
        }
    }
}

#[test]
fn ignored_nodes_include_standalone_indicators_and_properties() {
    for (source, expected) in [
        (
            "before: 0\n# ryl format-ignore\n?    \n    held\n:    1\nafter:    2\n",
            "before: 0\n# ryl format-ignore\n?    \n    held\n:    1\nafter: 2\n",
        ),
        (
            "before: 0\nkey:\n  # ryl format-ignore\n  &a   \n    !!map   \n      child:    1\nafter:    2\n",
            "before: 0\nkey:\n  # ryl format-ignore\n  &a   \n    !!map   \n      child:    1\nafter: 2\n",
        ),
        (
            "before:    0\nkey:\n    # ryl format-ignore\n    |-\n      words\n      more words\nafter:    1\n",
            "before: 0\nkey:\n    # ryl format-ignore\n    |-\n      words\n      more words\nafter: 1\n",
        ),
    ] {
        formats(source, "[format]\n", expected);
    }
}

#[test]
fn indentless_sequence_header_does_not_pin_independent_child_mapping() {
    formats(
        "root:\n- a:\n    child: 1\n# ryl format-ignore\n- held\n",
        "indent-width=4\n[format]\nindent-sequences=false\ndash-on-own-line=true\n",
        "root:\n-\n    a:\n        child: 1\n# ryl format-ignore\n- held\n",
    );
    formats(
        "root: &a !!seq\n-    - one\n     # ryl format-ignore\n     - two\n- ?    key\n  : value\n",
        "[format]\n",
        "root: &a !!seq\n-    - one\n     # ryl format-ignore\n     - two\n- ? key\n  : value\n",
    );
}

#[test]
fn fixed_comments_keep_preceding_scalar_boundaries_and_independent_formatting() {
    for style in ["|", ">", "|4", ">4"] {
        let input = format!(
            "a: {style}\n    text\n  # ryl format-disable\nb:    1\n# ryl format-enable\nafter:    2\nextra:\n    child:    3\n"
        );
        let expected = format!(
            "a: {style}\n    text\n  # ryl format-disable\nb:    1\n# ryl format-enable\nafter: 2\nextra:\n  child: 3\n"
        );
        formats(&input, "[format]\n", &expected);
    }
}

#[test]
fn native_header_geometry_follows_empty_scalars_flow_pairs_and_aliases() {
    let input = "# ryl format-ignore\nempty: |\nsequence:\n- &anchor [a: b, [c, d]]\nalias: *anchor\nmap: {a: b}\n";
    formats(input, "[format]\n", input);
}

#[test]
fn empty_sequence_entries_do_not_steal_later_collection_headers() {
    for empty_count in 1..=3 {
        for (outer, inner) in [("  ", "      "), ("", "    ")] {
            let empties = format!("{outer}-\n").repeat(empty_count);
            let input = format!(
                "root:\n{empties}{outer}-\n{inner}- before\n{inner}# ryl format-ignore\n{inner}- held\nafter:    2\n"
            );
            let expected = input.replace("after:    2", "after: 2");
            formats(&input, "[format]\n", &expected);
        }
    }
}

#[test]
fn implicit_flow_pairs_do_not_require_a_mapping_opening_token() {
    let input = "# ryl format-ignore\n[\n? foo\n bar : baz\n]\n";
    formats(input, "[format]\n", input);
}

#[test]
fn later_compact_sequence_headers_follow_their_parent_entry() {
    formats(
        "root:\n- first\n-    - before\n     # ryl format-ignore\n     - held\nafter:    1\n",
        "[format]\n",
        "root:\n- first\n-    - before\n     # ryl format-ignore\n     - held\nafter: 1\n",
    );
}

#[test]
fn ignored_standalone_scalar_can_retain_a_different_indentation_width() {
    formats(
        "before:\n  child: 0\nheld:\n    # ryl format-ignore\n    'word'\nafter:    1\nextra:\n    child:    2\n",
        "[format]\n",
        "before:\n  child: 0\nheld:\n    # ryl format-ignore\n    'word'\nafter: 1\nextra:\n  child: 2\n",
    );
}

#[test]
fn ignored_node_keeps_properties_written_before_its_ignore_comment() {
    for (before, after) in [("&anchor", "!!map"), ("!!map", "&anchor")] {
        let input = format!(
            "before:    0\nheld: {before}   \n    # ryl format-ignore\n    {after}   \n        child:    1\nafter:    2\n"
        );
        let expected = input
            .replace("before:    0", "before: 0")
            .replace("after:    2", "after: 2");
        formats(&input, "[format]\nproperty-order='tag-first'\n", &expected);
    }
    formats(
        "before:    0\nseq: &outer   \n- &inner   \n  # ryl format-ignore\n  !!map   \n    child:    1\nafter:    2\n",
        "[format]\n",
        "before: 0\nseq: &outer\n- &inner   \n  # ryl format-ignore\n  !!map   \n    child:    1\nafter: 2\n",
    );
}

#[test]
fn ignored_nodes_follow_delayed_property_only_empty_values() {
    for properties in ["&previous", "!!str", "&previous !!str", "!!str &previous"] {
        for prefix in [
            format!("before: {properties}\n"),
            format!("? key\n: {properties}\n"),
            format!("before: {properties} |-\n"),
            format!("before: {properties} >-\n"),
        ] {
            formats(
                &format!("{prefix}# ryl format-ignore\nheld:    [1,2]\nafter:    2\n"),
                "[format]\n",
                &format!("{prefix}# ryl format-ignore\nheld:    [1,2]\nafter: 2\n"),
            );
        }
        formats(
            &format!(
                "items:\n  - {properties}\n  # ryl format-ignore\n  -    [1,2]\nafter:    2\n"
            ),
            "[format]\n",
            &format!(
                "items:\n  - {properties}\n  # ryl format-ignore\n  -    [1,2]\nafter: 2\n"
            ),
        );
    }
}

#[test]
fn property_only_empty_values_end_before_following_mapping_keys() {
    use ryl::rules::indentation::{
        self, Config, IndentSequencesSetting, SpacesSetting,
    };
    let cfg = Config::new(SpacesSetting::Fixed(2), IndentSequencesSetting::True, false);
    for properties in ["&previous", "!!str", "&previous !!str", "!!str &previous"] {
        for prefix in [
            format!("before: {properties}\n"),
            format!("? key\n: {properties}\n"),
        ] {
            let input = format!("{prefix}held:    [1,2]\nafter:    2\n");
            assert!(indentation::check(&input, &cfg).is_empty(), "{input:?}");
            formats(
                &input,
                "[format]\n",
                &format!("{prefix}held: [1, 2]\nafter: 2\n"),
            );
        }
    }
}

#[test]
fn empty_property_values_leave_collection_closers_at_their_owner() {
    use ryl::rules::indentation::{
        self, Config, IndentSequencesSetting, SpacesSetting,
    };
    let cfg = Config::new(SpacesSetting::Fixed(2), IndentSequencesSetting::True, false);
    for input in [
        "before: &empty !!str\n",
        "map: {\n  before: &empty !!str\n }\nafter: 2\n",
        "list: [\n  before: &empty !!str\n ]\nafter: 2\n",
        "map: {\n  before: &empty !!str\n  ,\n  after: 2\n }\n",
        "list: [\n  before: &empty !!str\n  ,\n  next\n ]\n",
    ] {
        for properties in ["", "&empty", "!!str", "&empty !!str", "!!str &empty"] {
            let input = input.replace("&empty !!str", properties);
            let violations = indentation::check(&input, &cfg);
            assert!(violations.is_empty(), "{input:?}: {violations:?}");
            let output = indentation::reindent(&input, &cfg);
            assert!(output.refused.is_empty(), "{output:?}");
            assert_eq!(output.text, input);
        }
    }
}

fn preserves_formatted_values(input: &str, settings: &str) {
    let cfg = YamlLintConfig::from_toml_str(settings).unwrap();
    let output = format_str(input, &cfg, Path::new("case.yaml"), &[]);
    crate::property::reference::check_strict_events(input, &output).unwrap();
    assert_eq!(
        format_str(&output, &cfg, Path::new("case.yaml"), &[]),
        output
    );
}

#[test]
fn empty_property_keys_keep_colon_separation() {
    formats(
        r#"map: { &empty : '' }
"#,
        r#"[format]
mapping-expand='always'
"#,
        r#"map:
  {
    &empty : '',
  }
"#,
    );
    formats(
        r#"map: { !!str &empty : '' }
"#,
        r#"[format]
mapping-expand='always'
property-order='anchor-first'
"#,
        r#"map:
  {
    &empty !!str : '',
  }
"#,
    );
    for key in [
        "",
        "&empty",
        "!!str",
        "&empty !!str",
        "!!str &empty",
        "*anchor",
    ] {
        let input = format!("anchor: &anchor value\nmap: {{ {key} : '', other: 1 }}\n");
        for style in ["preserve", "flow", "block"] {
            preserves_formatted_values(
                &input,
                &format!(
                    "[format]\nmapping-style = '{style}'\nmapping-expand = 'always'\n"
                ),
            );
        }
    }
}

#[test]
fn large_document_empty_property_keys_preserve_offsets() {
    // More than 1 MiB enables separate parallel event and lexical scans.
    let input = format!(
        "#{}\r\nprefix: é\r\nmap: {{ !!str &empty : {{'': x}} }}\r\n",
        "padding ".repeat(131_073),
    );
    let settings =
        "[format]\nmapping-expand = 'always'\nproperty-order = 'anchor-first'\n";
    preserves_formatted_values(&input, settings);
    let cfg = YamlLintConfig::from_toml_str(settings).unwrap();
    let output = format_str(&input, &cfg, Path::new("case.yaml"), &[]);
    assert_ne!(output, input);
    assert!(output.ends_with("prefix: é\nmap:\n  {\n    &empty !!str :\n      {\n        '': x,\n      },\n  }\n"), "{output:?}");
}

#[test]
fn converted_empty_property_values_preserve_following_comments() {
    let input = r#"# before
items: [{value: 'alpha beta gamma delta epsilon', empty: !!str}] # after
after: 1
"#;
    let config = r#"line-length=20
[format]
sequence-style='flow'
mapping-style='flow'
sequence-expand='auto'
quote-style='preserve'
"#;
    formats(
        input,
        config,
        r#"# before
items:  # after
  - value: 'alpha beta gamma delta epsilon'
    empty: !!str
after: 1
"#,
    );
    let cfg = YamlLintConfig::from_toml_str(config).unwrap();
    let output = format_str(input, &cfg, Path::new("case.yaml"), &[]);
    crate::property::representation::check_annotations(input, &output).unwrap();
}

#[test]
fn empty_property_sequence_entries_end_before_following_mapping_keys() {
    use ryl::rules::indentation::{
        self, Config, IndentSequencesSetting, SpacesSetting,
    };
    for (indent, setting) in [
        ("", IndentSequencesSetting::False),
        ("  ", IndentSequencesSetting::True),
    ] {
        let cfg = Config::new(SpacesSetting::Fixed(2), setting, false);
        for properties in ["&empty", "!!str", "&empty !!str", "!!str &empty"] {
            for items in [
                format!("{indent}- first\n{indent}- {properties}\n"),
                format!("{indent}- - first\n{indent}  - {properties}\n"),
            ] {
                let input = format!("items:\n{items}after:\n  next: 1\n");
                assert!(indentation::check(&input, &cfg).is_empty(), "{input:?}");
            }
        }
    }
    let input = r#"items: [value, &empty !!str]
after:
  child: 1
"#;
    let config = r#"[format]
sequence-style='block'
indent-sequences=false
[lint.rules.indentation]
indent-sequences=false
"#;
    formats(
        input,
        config,
        r#"items:
- value
- &empty !!str
after:
  child: 1
"#,
    );
    let cfg = YamlLintConfig::from_toml_str(config).unwrap();
    let output = format_str(input, &cfg, Path::new("case.yaml"), &[]);
    let violations =
        ryl::lint_str(&output, Path::new("case.yaml"), &cfg, Path::new("."));
    assert!(
        !violations
            .iter()
            .any(|problem| problem.rule == Some("indentation")),
        "{violations:?}"
    );
}

fn prose_suppression_case(held: &str, (open, close): (&str, &str), mode: &str) {
    for keep_quotes in [false, true] {
        let settings = format!(
            "line-length = 14\n[format]\nprose-wrap = '{mode}'\nquote-style = 'double'\nkeep-quotes = {keep_quotes}\nsequence-style = 'block'\n"
        );
        let cfg = YamlLintConfig::from_toml_str(&settings).unwrap();
        for ending in ["\n", "\r\n"] {
            let protected = format!("{open}{held}{close}").replace('\n', ending);
            let input = format!("before: \"alpha beta\n  gamma delta\"\n{open}{held}{close}after: \"alpha beta\n  gamma delta\"\nconvert: [1,2]\n").replace('\n', ending);
            let output = format_str(&input, &cfg, Path::new("case.yaml"), &[]);
            assert!(
                output.contains(&protected),
                "settings {settings:?}; input {input:?}; output {output:?}"
            );
            assert!(output.contains("convert:\n  - 1\n  - 2\n"), "{output:?}");
            if mode == "never" {
                assert!(
                    output.contains("before: alpha beta gamma delta")
                        || output.contains("before: \"alpha beta gamma delta\""),
                    "{output:?}"
                );
                assert!(
                    output.contains("after: alpha beta gamma delta")
                        || output.contains("after: \"alpha beta gamma delta\""),
                    "{output:?}"
                );
            }
            crate::property::reference::check_strict_events(&input, &output).unwrap();
            assert_eq!(
                format_str(&output, &cfg, Path::new("case.yaml"), &[]),
                output
            );
        }
    }
}

#[test]
fn prose_policies_and_quote_retention_leave_suppressed_scalars_exact() {
    for mode in ["preserve", "always", "never"] {
        for markers in [
            ("# ryl format-ignore\n", ""),
            ("# ryl format-disable\n", "# ryl format-enable\n"),
        ] {
            for held in [
                "held:   &kept !!str alpha beta\n    gamma delta\n",
                "held:   &kept !!str 'alpha beta\n    gamma delta'\n",
                "held:   &kept !!str \"alpha beta\n    gamma delta\"\n",
                "held:   &kept !!str >-\n    alpha beta\n    gamma delta\n",
            ] {
                prose_suppression_case(held, markers, mode);
            }
        }
    }
}

#[test]
fn prose_joining_respects_native_regions_in_markdown() {
    let input = "Text\n\n```yaml\n# ryl format-disable\nheld: 'alpha beta\n    gamma delta'\n```\n\n```yaml\nafter: \"alpha beta\n  gamma delta\"\n```\n";
    let expected = "Text\n\n```yaml\n# ryl format-disable\nheld: 'alpha beta\n    gamma delta'\n```\n\n```yaml\nafter: \"alpha beta gamma delta\"\n```\n";
    let cfg = YamlLintConfig::from_toml_str(
        "[format]\nprose-wrap = 'never'\nkeep-quotes = true\nquote-style = 'double'\n",
    )
    .unwrap();
    let format = |text: &str| {
        ryl::fix::fix_markdown_str(
            text,
            Path::new("case.md"),
            &cfg,
            Path::new("."),
            ryl::fix::Rewrite::Format,
        )
        .unwrap_or_else(|| text.to_owned())
    };
    assert_eq!(format(input), expected);
    assert_eq!(format(expected), expected);
}

#[test]
fn collapsed_scalars_keep_native_crlf_permissions_on_protected_lines() {
    for (before, protected, after) in [
        (
            "  - 'alpha beta\n    gamma delta'\n",
            "  # ryl format-disable\n  # ryl format-enable\n",
            "  - 'alpha beta\n    gamma delta'\n",
        ),
        (
            "  - 'alpha beta\n    gamma delta'\n",
            "  # ryl format-disable\n  -   held\n  # ryl format-enable\n",
            "  - 'alpha beta\n    gamma delta'\n",
        ),
        (
            "  - 'alpha beta\n    gamma delta'\n",
            "  # ryl format-ignore\n  -   held\n",
            "  - 'alpha beta\n    gamma delta'\n",
        ),
        (
            "  before: 'alpha beta\n    gamma delta'\n",
            "  # ryl format-ignore\n  held:    1\n",
            "  after: 'alpha beta\n    gamma delta'\n",
        ),
    ] {
        for policy in [
            "",
            "prose-wrap = 'preserve'\n",
            "prose-wrap = 'always'\n",
            "prose-wrap = 'never'\n",
        ] {
            for keep_quotes in [false, true] {
                let settings = format!(
                    "[format]\nsequence-style = 'block'\n{policy}keep-quotes = {keep_quotes}\n"
                );
                let cfg = YamlLintConfig::from_toml_str(&settings).unwrap();
                let input =
                    format!("root:\n{before}{protected}{after}tail:    [1,2]\n")
                        .replace('\n', "\r\n");
                let protected = protected.replace('\n', "\r\n");
                let output = format_str(&input, &cfg, Path::new("case.yaml"), &[]);
                assert!(
                    output.contains(&protected),
                    "settings {settings:?}; input {input:?}; output {output:?}"
                );
                assert!(
                    !output.replace(&protected, "").contains('\r'),
                    "settings {settings:?}; output {output:?}"
                );
                crate::property::reference::check_strict_events(&input, &output)
                    .unwrap();
                assert_eq!(
                    format_str(&output, &cfg, Path::new("case.yaml"), &[]),
                    output
                );
            }
        }
    }
}

#[test]
fn collapsed_sequence_keeps_only_suppressed_crlf() {
    let input = r#"root:
  - 'alpha beta
    gamma delta'
  # ryl format-ignore
  -    'held'
tail: [1,2]
"#
    .replace('\n', "\r\n");
    let settings = "[format]\nsequence-style = 'block'\n";
    let protected = "  # ryl format-ignore\r\n  -    'held'\r\n";
    let cfg = YamlLintConfig::from_toml_str(settings).unwrap();
    let output = format_str(&input, &cfg, Path::new("case.yaml"), &[]);
    assert_ne!(output, input);
    assert!(output.contains("tail:\n  - 1\n  - 2\n"), "{output:?}");
    assert!(output.contains(protected), "{output:?}");
    assert!(!output.replace(protected, "").contains('\r'), "{output:?}");
    crate::property::reference::check_strict_events(&input, &output).unwrap();
    assert_eq!(
        format_str(&output, &cfg, Path::new("case.yaml"), &[]),
        output
    );
}

#[test]
fn prose_reflow_preserves_explicit_indent_around_blank_block_content() {
    formats(
        "root:\n  - a: >2+\n       \n    b: x\nother: ' alpha beta gamma delta'\n",
        r#"indent-width=2
line-length=14
[format]
prose-wrap='always'
dash-on-own-line=true
"#,
        "root:\n  -\n    a: >2+\n       \n    b: x\nother: ' alpha\n  beta gamma\n  delta'\n",
    );
}

#[test]
fn prose_reflow_respects_existing_scalar_safety_refusals() {
    let input = "a:\n  b: |-\n    \n\nc: 'alpha beta\n  gamma delta'\n";
    for mode in ["always", "never"] {
        let settings = format!("[format]\nprose-wrap = '{mode}'\n");
        formats(input, &settings, input);
    }
}
