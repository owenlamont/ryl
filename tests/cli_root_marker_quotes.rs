mod common;

use common::cli::ryl_on;

const CONFIG: &str = "[format]\n[lint.rules.quoted-strings]\nrequired = 'only-when-needed'\ncheck-keys = true\nquote-type = 'single'\n";

fn assert_rewrite(input: &str, expected: &str, markdown: bool) {
    for command in [&["format"][..], &["check", "--fix"][..]] {
        let mut args = command.to_vec();
        if markdown {
            args.push("--markdown");
        }
        let (code, _, stderr, output) = ryl_on(Some(CONFIG), input, &args);
        assert_eq!(code, 0, "{input:?}: {stderr}");
        assert_eq!(output, expected, "{input:?}: {command:?}");
        let (code, _, stderr, again) = ryl_on(Some(CONFIG), &output, &args);
        assert_eq!(code, 0, "{stderr}");
        assert_eq!(again, output);
    }
}

#[test]
fn root_document_start_keeps_quotes() {
    assert_rewrite("'---'\n", "'---'\n", false);
}

#[test]
fn root_document_end_keeps_quotes() {
    assert_rewrite("'...'\n", "'...'\n", false);
}

#[test]
fn semantic_guard_rejects_unsafe_parseable_output() {
    for (input, output) in [
        ("'---'\n", "---\n"),
        ("'...'\n", "...\n"),
        ("'--- x'\n", "--- x\n"),
        ("'---\u{a0}x'\n", "---\u{a0}x\n"),
        ("'...\u{2003}x'\n", "...\u{2003}x\n"),
        ("'first ---\u{feff}x'\n", "first ---\u{feff}x\n"),
        ("!!str 'first ---\u{feff}x'\n", "!!str first ---\u{feff}x\n"),
        ("'first ---\u{feff}x'\n", "|-\n  first ---\u{feff}x\n"),
        ("\u{feff}'---\u{a0}x'\r", "\u{feff}---\u{a0}x\r"),
        ("'... # c'\n", "... # c\n"),
        ("'x ---'\n", "x\n---\n"),
        ("'x ...'\n", "x\n...\n"),
        ("'x'\n", "y\n"),
        ("'true'\n", "true\n"),
        ("[a, b]\n", "[b, a]\n"),
    ] {
        assert!(
            !ryl::fix::format_events_match(input, output),
            "{input:?} -> {output:?}"
        );
    }
}

#[test]
fn semantic_guard_accepts_presentation_changes() {
    for (input, output) in [
        ("'text'\n", "---\ntext\n...\n"),
        ("[a, b]\n", "- a\n- b\n"),
        ("%YAML 1.1\n---\n'1e3'\n", "%YAML 1.1\n---\n1e3\n"),
        ("a: |+\r  value\r  ", "a: |+\n  value\n\n"),
    ] {
        assert!(
            ryl::fix::format_events_match(input, output),
            "{input:?} -> {output:?}"
        );
    }
}

#[test]
fn root_markers_with_trailing_content_keep_quotes() {
    for value in ["--- x", "--- # c", "... # c", "... x", "---\tx", "...\tx"] {
        let input = format!("'{value}'\n");
        assert_rewrite(&input, &input, false);
    }
}

#[test]
fn marker_prefix_without_separation_can_be_plain() {
    for value in ["---x", "...x", "----", "....", "---#c", "...#c"] {
        assert_rewrite(&format!("'{value}'\n"), &format!("{value}\n"), false);
    }
}

#[test]
fn unicode_marker_separators_keep_quotes() {
    for separator in [
        '\u{a0}', '\u{1680}', '\u{2003}', '\u{2028}', '\u{2029}', '\u{202f}',
        '\u{205f}', '\u{3000}', '\u{feff}',
    ] {
        for marker in ["---", "..."] {
            let input = format!("'{marker}{separator}x'\n");
            assert_rewrite(&input, &input, false);
        }
    }
}

#[test]
fn bom_in_joined_continuations_keeps_quotes() {
    for prefix in ["", "key: ", "- "] {
        for value in ["---\u{feff}x", "...\u{feff}x"] {
            let input = format!("{prefix}'first\n  {value}'\n");
            assert_rewrite(&input, &input, false);
        }
    }
}

#[test]
fn block_conversion_cannot_introduce_unicode_marker_lines() {
    for marker in ["---", "..."] {
        let input = format!("{{{marker}\u{a0}x: y}}\n");
        let (code, _, stderr, output) = ryl_on(
            Some("[format]\nmapping-style = 'block'\n"),
            &input,
            &["format"],
        );
        assert_eq!(code, 0, "{stderr}");
        assert_eq!(output, input);
    }
}

#[test]
fn nested_marker_values_can_be_plain() {
    for prefix in ["- ", "key: ", "? ", "? key\n: "] {
        for value in ["---", "...", "--- x", "---\u{a0}x"] {
            assert_rewrite(
                &format!("{prefix}'{value}'\n"),
                &format!("{prefix}{value}\n"),
                false,
            );
        }
    }
}

#[test]
fn adjacent_mapping_colon_prevents_a_document_marker() {
    for value in ["---", "..."] {
        assert_rewrite(&format!("'{value}': x\n"), &format!("{value}: x\n"), false);
    }
}

#[test]
fn separated_mapping_colon_keeps_load_bearing_quotes_during_lint_fix() {
    for value in ["---", "..."] {
        let input = format!("'{value}' : x\n");
        let (code, _, stderr, output) =
            ryl_on(Some(CONFIG), &input, &["check", "--fix"]);
        assert_eq!(code, 0, "{stderr}");
        assert_eq!(output, input);
        let (code, _, stderr, output) = ryl_on(Some(CONFIG), &input, &["format"]);
        assert_eq!(code, 0, "{stderr}");
        assert_eq!(output, format!("{value}: x\n"));
    }
}

#[test]
fn block_collection_conversion_keeps_marker_keys_and_values() {
    let config = "[format]\nmapping-style = 'block'\nsequence-style = 'block'\n";
    for (input, expected) in [
        ("[\"---\", \"...\", \"--- x\"]\n", "- ---\n- ...\n- --- x\n"),
        (
            "{\"--- x\": y, \"... # c\": z}\n",
            "'--- x': y\n'... # c': z\n",
        ),
    ] {
        let (code, _, stderr, output) = ryl_on(Some(config), input, &["format"]);
        assert_eq!(code, 0, "{stderr}");
        assert_eq!(output, expected);
    }
}

#[test]
fn marker_quote_decisions_are_not_cached_across_columns() {
    for value in ["---", "...", "---\u{a0}x"] {
        let input = format!("key: '{value}'\n---\n'{value}'\n");
        let expected = format!("key: {value}\n---\n'{value}'\n");
        assert_rewrite(&input, &expected, false);
    }
}

#[test]
fn multiline_marker_continuations_preserve_values() {
    for value in ["---", "...", "--- x", "... # c"] {
        for prefix in ["", "- ", "key: "] {
            let input = format!("{prefix}'first\n  {value}'\n");
            let expected = if value.contains('#') {
                input.clone()
            } else {
                format!("{prefix}first {value}\n")
            };
            assert_rewrite(&input, &expected, false);
        }
        let input = format!("first\n  {}\n", value.replace(" #", "  #"));
        assert_rewrite(&input, &input, false);
    }
}

#[test]
fn bom_root_markers_keep_quotes() {
    for quote in ['\'', '"'] {
        for value in ["---", "...", "--- x"] {
            let input = format!("\u{feff}{quote}{value}{quote}\n");
            let expected = format!("\u{feff}'{value}'\n");
            assert_rewrite(&input, &expected, false);
        }
    }
}

#[test]
fn in_memory_bom_root_markers_keep_quotes() {
    let cfg = ryl::config::YamlLintConfig::from_toml_str(CONFIG).unwrap();
    let path = std::path::Path::new("input.yaml");
    for value in ["---", "...", "--- x"] {
        let input = format!("\u{feff}'{value}'\n");
        assert_eq!(ryl::format::format_str(&input, &cfg, path, &[]), input);
        assert_eq!(
            ryl::fix::apply_safe_fixes(&input, &cfg, path, path.parent().unwrap()),
            input
        );
    }
}

#[test]
fn markdown_regions_keep_root_marker_quotes() {
    for value in ["---", "...", "--- x", "... # c", "---\u{a0}x"] {
        for (prefix, suffix) in [
            ("# Host\n\n```yaml\n", "```\n\nTail.\n"),
            ("---\n", "---\n\n# Host\n"),
        ] {
            let input = format!("{prefix}'{value}'\n{suffix}");
            assert_rewrite(&input, &input, true);
        }
    }
}
