use ryl::rules::trailing_spaces::{self, Violation};

#[test]
fn reports_trailing_space() {
    let input = "---\nsome: text \n";
    let hits = trailing_spaces::check(input);
    assert_eq!(
        hits,
        vec![Violation {
            line: 2,
            column: 11,
        }]
    );
}

#[test]
fn reports_trailing_tab() {
    let input = "key:\t\n";
    let hits = trailing_spaces::check(input);
    assert_eq!(hits, vec![Violation { line: 1, column: 5 }]);
}

#[test]
fn ignores_clean_lines() {
    let input = "foo: bar\n";
    let hits = trailing_spaces::check(input);
    assert!(hits.is_empty());
}

#[test]
fn handles_crlf_lines() {
    let input = "---\r\nsome: text \r\n";
    let hits = trailing_spaces::check(input);
    assert_eq!(
        hits,
        vec![Violation {
            line: 2,
            column: 11,
        }]
    );
}

#[test]
fn fix_trims_empty_block_scalar_headers_without_touching_bodies() {
    for (input, expected) in [
        ("a: | \t", "a: |"),
        ("a: >+2  # note \t", "a: >+2  # note"),
        ("a: |+ \t\n  ", "a: |+\n  "),
        ("a: | \t\n  x  ", "a: |\n  x  "),
    ] {
        assert_eq!(trailing_spaces::fix(input).as_deref(), Some(expected));
        assert_eq!(
            ryl::yaml_dom::YamlOwned::load_from_str(input).unwrap(),
            ryl::yaml_dom::YamlOwned::load_from_str(expected).unwrap()
        );
    }
}

#[test]
fn fix_trims_the_line_after_a_blank_only_block_scalar() {
    assert_eq!(
        trailing_spaces::fix("a: |+\n\nb: 1   \n"),
        Some("a: |+\n\nb: 1\n".to_string())
    );
    assert_eq!(
        trailing_spaces::fix("a: |+\nb: 1   \n"),
        Some("a: |+\nb: 1\n".to_string())
    );
}

#[test]
fn fix_strips_only_block_scalar_blank_indentation() {
    for header in ["|", ">", "|-", ">-", "|+", ">+", "|3", ">3-", "|+3"] {
        for newline in ["\n", "\r\n"] {
            for prefix in ["a: ", "root:\n  a: "] {
                let indent = if prefix.starts_with("root") { 5 } else { 3 };
                let spaces = " ".repeat(indent);
                let input = format!(
                    "{prefix}{header}\n{spaces}\n{spaces}x\n{spaces}\n{spaces} \n{spaces}y  \n"
                ).replace('\n', newline);
                let expected = format!(
                    "{prefix}{header}\n\n{spaces}x\n\n{spaces} \n{spaces}y  \n"
                )
                .replace('\n', newline);
                let fixed = trailing_spaces::fix(&input).unwrap();
                assert_eq!(fixed, expected, "{input:?}");
                assert_eq!(
                    ryl::yaml_dom::YamlOwned::load_from_str(&input).unwrap(),
                    ryl::yaml_dom::YamlOwned::load_from_str(&fixed).unwrap()
                );
                assert!(trailing_spaces::fix(&fixed).is_none());
            }
        }
    }
}

#[test]
fn fix_handles_blank_only_block_scalars_and_unterminated_lines() {
    for header in ["|3", ">3", "|3-", ">-3", "|3+", ">+3"] {
        for suffix in ["", "b: 1\n"] {
            for width in [1, 3, 4] {
                let input = format!("a: {header}\n{}\n{suffix}", " ".repeat(width));
                let expected = format!("a: {header}\n\n{suffix}");
                if width <= 3 {
                    assert_eq!(
                        trailing_spaces::fix(&input),
                        Some(expected),
                        "{input:?}"
                    );
                } else {
                    assert!(trailing_spaces::fix(&input).is_none(), "{input:?}");
                }
            }
        }
    }
    for (input, expected) in [
        ("a: |+\n   ", "a: |+\n   "),
        ("a: |3-\n   ", "a: |3-\n   "),
        ("a: |3+\n   x\n   ", "a: |3+\n   x\n   "),
        ("a: |3+\n    ", "a: |3+\n    "),
        ("a: |3+\n   \t\n   x\n", "a: |3+\n   \t\n   x\n"),
    ] {
        assert_eq!(
            trailing_spaces::fix(input).unwrap_or_else(|| input.to_owned()),
            expected
        );
    }
}
