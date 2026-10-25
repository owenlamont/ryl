use ryl::rules::{document_end, empty_lines, new_line_at_end_of_file, trailing_spaces};

use crate::common;
use common::cli::ryl_on;

// The YAML 1.2 reference parser keeps the same value after a final LF for these bodies.
const UNSTRIPPED: [&str; 7] = [
    "a: |+\n  a",
    "a: |\n  a",
    "a: >\n  a\n  b",
    "a: >+\n  a\n  b",
    "|2\n  a",
    "k: | # | >\n  text",
    "k: | # c\n  text",
];

#[test]
fn the_final_newline_fixes_end_an_unstripped_scalar_at_the_end_of_file() {
    let marker = document_end::Config::new(true);
    for input in UNSTRIPPED {
        assert_eq!(
            new_line_at_end_of_file::fix(input, "\n"),
            Some(format!("{input}\n")),
            "{input:?}"
        );
        assert_eq!(
            document_end::fix(input, &marker),
            Some(format!("{input}\n...\n")),
            "{input:?}"
        );
        assert!(new_line_at_end_of_file::check(input).is_some(), "{input:?}");
    }
    assert_eq!(
        document_end::fix("- >+2\n    a\n", &marker).as_deref(),
        Some("- >+2\n    a\n...\n")
    );
}

#[test]
fn the_final_newline_fixes_still_end_other_files() {
    let marker = document_end::Config::new(true);
    for (input, newline, ended) in [
        ("a: |-\n  a", "a: |-\n  a\n", "a: |-\n  a\n...\n"),
        (
            "k: |\n  a\n# c",
            "k: |\n  a\n# c\n",
            "k: |\n  a\n# c\n...\n",
        ),
        (
            "a: |+\n  a\n# c",
            "a: |+\n  a\n# c\n",
            "a: |+\n  a\n# c\n...\n",
        ),
        (
            "a: |+\n  a\nb: 1",
            "a: |+\n  a\nb: 1\n",
            "a: |+\n  a\nb: 1\n...\n",
        ),
    ] {
        assert_eq!(
            new_line_at_end_of_file::fix(input, "\n").as_deref(),
            Some(newline)
        );
        assert_eq!(document_end::fix(input, &marker).as_deref(), Some(ended));
    }
}

#[test]
fn final_blank_scalar_indent_does_not_enter_the_value_before_a_marker() {
    let cfg = document_end::Config::new(true);
    assert_eq!(
        document_end::fix("a: |+\n  a\n ", &cfg).as_deref(),
        Some("a: |+\n  a\n\n...\n")
    );
    for prefix in ["a: ", "x:\n  a: ", "a: 1\n---\nb: ", "- "] {
        let indent = if prefix == "x:\n  a: " { "    " } else { "  " };
        let fixed_prefix = prefix.replace("a: 1\n---\n", "a: 1\n...\n---\n");
        for indicator in ["|", "|+", "|-", ">", ">+", ">-"] {
            let input = format!("{prefix}{indicator}\n{indent}");
            let expected = format!("{fixed_prefix}{indicator}\n\n...\n");
            assert_eq!(
                document_end::fix(&input, &cfg).as_deref(),
                Some(expected.as_str()),
                "{input:?}"
            );
            assert_eq!(
                new_line_at_end_of_file::fix(&input, "\n"),
                Some(format!("{input}\n")),
                "{input:?}"
            );
            assert_eq!(
                document_end::fix(&format!("{input}\n"), &cfg).as_deref(),
                Some(expected.as_str()),
                "{input:?}"
            );
        }
        for indicator in ["|2", "|+2", "|-2", ">2", ">+2", ">-2"] {
            let input = format!("{prefix}{indicator}\n{indent} ");
            assert_eq!(
                document_end::fix(&input, &cfg),
                Some(format!("{fixed_prefix}{indicator}\n{indent} \n...\n")),
                "{input:?}"
            );
        }
    }
}

#[test]
fn cli_fixes_a_spaces_only_keep_scalar_without_changing_its_value() {
    let input = "a: |+\n  ";
    for (rules, expected) in [
        ("document-end = 'enable'", "a: |+\n\n...\n"),
        ("new-line-at-end-of-file = 'enable'", "a: |+\n  \n"),
        (
            "document-end = 'enable'\nnew-line-at-end-of-file = 'enable'",
            "a: |+\n\n...\n",
        ),
    ] {
        let config = format!("[lint.rules]\n{rules}\n");
        let (code, _, stderr, fixed) =
            ryl_on(Some(&config), input, &["check", "--fix"]);
        assert_eq!(code, 0, "{rules}: {stderr}");
        assert_eq!(fixed, expected, "{rules}");
    }
    let (code, _, stderr, formatted) =
        ryl_on(Some("[format]\ndocument-end = 'add'\n"), input, &["format"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(formatted, "a: |+\n\n...\n");
}

#[test]
fn root_explicit_indent_keeps_content_spaces_or_refuses_the_marker() {
    let cfg = document_end::Config::new(true);
    for newline in ["\n", "\r\n", "\r"] {
        let literal = format!("|2{newline}  ");
        let literal_ended = format!("{literal}{newline}...{newline}");
        assert_eq!(trailing_spaces::fix(&literal), None);
        assert_eq!(document_end::fix(&literal, &cfg), Some(literal_ended));
        let folded = format!(">+2{newline}  ");
        assert_eq!(trailing_spaces::fix(&folded), None);
        assert_eq!(document_end::fix(&folded, &cfg), None);
        assert_eq!(document_end::fix(&format!("{folded}{newline}"), &cfg), None);
        assert_eq!(
            new_line_at_end_of_file::fix(&folded, newline),
            Some(format!("{folded}{newline}"))
        );
    }
}

#[test]
fn cli_keeps_root_explicit_indent_scalar_values() {
    for (header, check_suffix, format_suffix) in
        [("|2", "\n...\n", "\n...\n"), (">+2", "", "\n")]
    {
        let input = format!("{header}\n  ");
        let (code, _, stderr, fixed) = ryl_on(
            Some("[lint.rules]\ndocument-end = 'enable'\n"),
            &input,
            &["check", "--fix"],
        );
        assert_eq!(code, usize::from(header == ">+2") as i32, "{stderr}");
        assert_eq!(fixed, format!("{input}{check_suffix}"));
        let (code, _, stderr, formatted) = ryl_on(
            Some("[format]\ndocument-end = 'add'\n"),
            &input,
            &["format"],
        );
        assert_eq!(code, 0, "{stderr}");
        assert_eq!(formatted, format!("{input}{format_suffix}"));
    }
}

#[test]
fn root_scalar_properties_keep_value_bearing_spaces() {
    let cfg = document_end::Config::new(true);
    for newline in ["\n", "\r\n", "\r"] {
        for prefix in ["&a ", "!foo ", "&a !foo ", "!foo &a "] {
            let literal = format!("{prefix}|2{newline}  ");
            assert_eq!(trailing_spaces::fix(&literal), None, "{literal:?}");
            assert_eq!(
                document_end::fix(&literal, &cfg),
                Some(format!("{literal}{newline}...{newline}")),
                "{literal:?}"
            );
            let folded = format!("{prefix}>+2{newline}  ");
            assert_eq!(trailing_spaces::fix(&folded), None, "{folded:?}");
            assert_eq!(document_end::fix(&folded, &cfg), None, "{folded:?}");
        }
    }
    for input in [
        "--- &a |2\n  ",
        "a: x\n--- &a |2\n  ",
        "&a |2\n  x\n  ",
        "# c\n&a |2\n  ",
        "%YAML 1.2\n--- &a |2\n  ",
    ] {
        assert_eq!(trailing_spaces::fix(input), None, "{input:?}");
        let fixed = document_end::fix(input, &cfg).unwrap();
        assert!(fixed.ends_with("  \n...\n"), "{fixed:?}");
    }
}

#[test]
fn cli_preserves_anchored_and_tagged_root_scalar_values() {
    for prefix in ["&a ", "!foo ", "&a !foo ", "!foo &a "] {
        for (header, marker) in [("|2", true), (">+2", false)] {
            let input = format!("{prefix}{header}\n  ");
            for (rules, expected) in [
                (
                    "document-end = 'enable'",
                    marker.then(|| format!("{input}\n...\n")),
                ),
                ("trailing-spaces = 'enable'", None),
                (
                    "document-end = 'enable'\ntrailing-spaces = 'enable'",
                    marker.then(|| format!("{input}\n...\n")),
                ),
                (
                    "new-line-at-end-of-file = 'enable'",
                    Some(format!("{input}\n")),
                ),
            ] {
                let config = format!("[lint.rules]\n{rules}\n");
                let (code, _, stderr, fixed) =
                    ryl_on(Some(&config), &input, &["check", "--fix"]);
                assert!(matches!(code, 0 | 1), "{rules}: {stderr}");
                assert_eq!(
                    fixed,
                    expected.as_deref().unwrap_or(&input),
                    "{rules}: {input:?}"
                );
            }
            let (code, _, stderr, formatted) = ryl_on(
                Some("[format]\ndocument-end = 'add'\n"),
                &input,
                &["format"],
            );
            assert_eq!(code, 0, "{stderr}");
            let expected = if marker {
                format!("{input}\n...\n")
            } else {
                format!("{input}\n")
            };
            assert_eq!(formatted, expected, "{input:?}");
        }
    }
}

#[test]
fn blank_lines_before_a_block_scalars_content_stay() {
    // PyYAML: `a: |+\n\n\n\nb: 1` is "\n\n\n" and `a: |\n\n\n\n  x` is "\n\n\nx\n".
    let collapse = empty_lines::Config::new(1, 0, 0);
    for input in ["a: |+\n\n\n\nb: 1\n", "a: |\n\n\n\n  x\nb: 1\n"] {
        assert_eq!(empty_lines::fix(input, &collapse), None, "{input:?}");
    }
    let input = "a:\n  - |\n    \n    a\n";
    let fixed = trailing_spaces::fix(input).unwrap();
    assert_eq!(fixed, "a:\n  - |\n\n    a\n");
    assert_eq!(
        ryl::yaml_dom::YamlOwned::load_from_str(input).unwrap(),
        ryl::yaml_dom::YamlOwned::load_from_str(&fixed).unwrap()
    );
}
