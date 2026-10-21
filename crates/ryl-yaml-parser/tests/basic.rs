#![allow(clippy::bool_assert_comparison)]
#![allow(clippy::float_cmp)]
mod support;
#[cfg(feature = "comments")]
use granit_parser::Placement;
use granit_parser::{Event, Parser, ScalarStyle, ScanError, StructureStyle, YamlVersion};

/// Run the parser through the string.
///
/// # Returns
/// This functions returns the events if parsing succeeds, the error the parser returned otherwise.
fn run_parser(input: &str) -> Result<Vec<Event<'_>>, ScanError> {
    let mut str_events = vec![];
    let mut str_error = None;
    let mut iter_events = vec![];
    let mut iter_error = None;

    for x in Parser::new_from_str(input) {
        match x {
            Ok(event) => str_events.push(event),
            Err(e) => {
                str_error = Some(e);
                break;
            }
        }
    }
    for x in Parser::new_from_iter(input.chars()) {
        match x {
            Ok(event) => iter_events.push(event),
            Err(e) => {
                iter_error = Some(e);
                break;
            }
        }
    }

    // eprintln!("str_events");
    // for x in &str_events {
    //     eprintln!("\t{x:?}");
    // }
    // eprintln!("iter_events");
    // for x in &iter_events {
    //     eprintln!("\t{x:?}");
    // }

    assert_eq!(str_events, iter_events);
    assert_eq!(str_error, iter_error);

    if let Some(err) = str_error {
        Err(err)
    } else {
        Ok(str_events.into_iter().map(|x| x.0).collect())
    }
}

fn collection_styles(input: &str) -> Vec<(&'static str, StructureStyle)> {
    run_parser(input)
        .unwrap()
        .into_iter()
        .filter_map(|event| match event {
            Event::SequenceStart(style, ..) => Some(("sequence", style)),
            Event::MappingStart(style, ..) => Some(("mapping", style)),
            _ => None,
        })
        .collect()
}

fn expected_events<'input>(events: impl IntoIterator<Item = Event<'input>>) -> Vec<Event<'input>> {
    events
        .into_iter()
        .filter(|event| cfg!(feature = "comments") || !support::is_comment_event(event))
        .collect()
}

#[test]
fn test_fail() {
    let s = "
# syntax error
scalar
key: [1, 2]]
key1:a2
";
    let Err(error) = run_parser(s) else { panic!() };
    assert_eq!(
        error.info(),
        "mapping values are not allowed in this context"
    );
    assert_eq!(
        error.to_string(),
        "mapping values are not allowed in this context at char 26 line 4 column 4"
    );
}

#[test]
fn test_sequence_structure_styles() {
    assert_eq!(
        collection_styles("- block\n- [flow]\n"),
        vec![
            ("sequence", StructureStyle::Block),
            ("sequence", StructureStyle::Flow),
        ]
    );

    assert_eq!(
        collection_styles("[flow, [nested]]\n"),
        vec![
            ("sequence", StructureStyle::Flow),
            ("sequence", StructureStyle::Flow),
        ]
    );
}

#[test]
fn test_mapping_structure_styles() {
    assert_eq!(
        collection_styles("block:\n  child: value\nflow: {child: value}\n"),
        vec![
            ("mapping", StructureStyle::Block),
            ("mapping", StructureStyle::Block),
            ("mapping", StructureStyle::Flow),
        ]
    );

    assert_eq!(
        collection_styles("[implicit: flow]\n"),
        vec![
            ("sequence", StructureStyle::Flow),
            ("mapping", StructureStyle::Flow),
        ]
    );
}

#[test]
fn test_empty_doc() {
    assert_eq!(
        run_parser("").unwrap(),
        [Event::StreamStart, Event::StreamEnd]
    );

    assert_eq!(
        run_parser("---").unwrap(),
        [
            Event::StreamStart,
            Event::DocumentStart(true, None),
            Event::Scalar("~".into(), ScalarStyle::Plain, 0, None),
            Event::DocumentEnd,
            Event::StreamEnd,
        ]
    );
}

#[test]
fn test_utf() {
    assert_eq!(
        run_parser("a: \u{4F60}\u{5273}").unwrap(),
        [
            Event::StreamStart,
            Event::DocumentStart(false, None),
            Event::MappingStart(StructureStyle::Block, 0, None),
            Event::Scalar("a".into(), ScalarStyle::Plain, 0, None),
            Event::Scalar("\u{4F60}\u{5273}".into(), ScalarStyle::Plain, 0, None),
            Event::MappingEnd,
            Event::DocumentEnd,
            Event::StreamEnd,
        ]
    );
}

#[test]
fn test_comments() {
    let s = "
# This is a comment
a: b # This is another comment
##
  #
";

    assert_eq!(
        run_parser(s).unwrap(),
        expected_events([
            Event::StreamStart,
            #[cfg(feature = "comments")]
            Event::Comment(" This is a comment".into(), Placement::Above),
            Event::DocumentStart(false, None),
            Event::MappingStart(StructureStyle::Block, 0, None),
            Event::Scalar("a".into(), ScalarStyle::Plain, 0, None),
            Event::Scalar("b".into(), ScalarStyle::Plain, 0, None),
            #[cfg(feature = "comments")]
            Event::Comment(" This is another comment".into(), Placement::Right),
            #[cfg(feature = "comments")]
            Event::Comment("#".into(), Placement::Above),
            #[cfg(feature = "comments")]
            Event::Comment("".into(), Placement::Above),
            Event::MappingEnd,
            Event::DocumentEnd,
            Event::StreamEnd,
        ])
    );
}

#[test]
fn test_quoting() {
    let s = "
- plain
- 'squote'
- \"dquote\"
";

    assert_eq!(
        run_parser(s).unwrap(),
        [
            Event::StreamStart,
            Event::DocumentStart(false, None),
            Event::SequenceStart(StructureStyle::Block, 0, None),
            Event::Scalar("plain".into(), ScalarStyle::Plain, 0, None),
            Event::Scalar("squote".into(), ScalarStyle::SingleQuoted, 0, None),
            Event::Scalar("dquote".into(), ScalarStyle::DoubleQuoted, 0, None),
            Event::SequenceEnd,
            Event::DocumentEnd,
            Event::StreamEnd,
        ]
    );
}

#[test]
fn test_multi_doc() {
    let s = "
a scalar
---
a scalar
---
a scalar
";
    assert_eq!(
        run_parser(s).unwrap(),
        [
            Event::StreamStart,
            Event::DocumentStart(false, None),
            Event::Scalar("a scalar".into(), ScalarStyle::Plain, 0, None),
            Event::DocumentEnd,
            Event::DocumentStart(true, None),
            Event::Scalar("a scalar".into(), ScalarStyle::Plain, 0, None),
            Event::DocumentEnd,
            Event::DocumentStart(true, None),
            Event::Scalar("a scalar".into(), ScalarStyle::Plain, 0, None),
            Event::DocumentEnd,
            Event::StreamEnd,
        ]
    );
}

#[test]
fn test_github_27() {
    // https://github.com/chyh1990/yaml-rust/issues/27
    assert_eq!(
        run_parser("&a").unwrap(),
        [
            Event::StreamStart,
            Event::DocumentStart(false, None),
            Event::Scalar("~".into(), ScalarStyle::Plain, 1, None),
            Event::DocumentEnd,
            Event::StreamEnd,
        ]
    );
}

#[test]
fn test_missing_node_with_anchor_is_null_scalar_but_tag_keeps_empty_content() {
    let scalar_events = |input: &str| -> Vec<(String, ScalarStyle, usize, Option<String>)> {
        run_parser(input)
            .unwrap()
            .into_iter()
            .filter_map(|event| match event {
                Event::Scalar(value, style, anchor, tag) => Some((
                    value.into_owned(),
                    style,
                    anchor,
                    tag.map(|tag| tag.original()),
                )),
                _ => None,
            })
            .collect()
    };

    assert_eq!(
        scalar_events("a: &x\n"),
        vec![
            ("a".to_string(), ScalarStyle::Plain, 0, None),
            ("~".to_string(), ScalarStyle::Plain, 1, None),
        ]
    );
    assert_eq!(
        scalar_events("a: !!str\n"),
        vec![
            ("a".to_string(), ScalarStyle::Plain, 0, None),
            (
                String::new(),
                ScalarStyle::Plain,
                0,
                Some("!!str".to_string()),
            ),
        ]
    );
}

#[test]
fn test_bad_hyphen() {
    // See: https://github.com/chyh1990/yaml-rust/issues/23
    assert!(run_parser("{-").is_err());
}

#[test]
fn test_issue_65() {
    // See: https://github.com/chyh1990/yaml-rust/issues/65
    let b = "\n\"ll\\\"ll\\\r\n\"ll\\\"ll\\\r\r\r\rU\r\r\rU";
    assert!(run_parser(b).is_err());
}

#[test]
fn test_issue_65_mwe() {
    // A MWE for `test_issue_65`. The error over there is that there is invalid trailing content
    // after a double quoted string.
    let b = r#""foo" l"#;
    assert!(run_parser(b).is_err());
}

#[test]
fn test_comment_after_tag() {
    // https://github.com/Ethiraric/yaml-rust2/issues/21#issuecomment-2053513507
    let s = "
%YAML 1.2
# This is a comment
--- #-------
foobar";

    assert_eq!(
        run_parser(s).unwrap(),
        expected_events([
            Event::StreamStart,
            #[cfg(feature = "comments")]
            Event::Comment(" This is a comment".into(), Placement::Above),
            Event::DocumentStart(true, Some(YamlVersion::new(1, 2))),
            #[cfg(feature = "comments")]
            Event::Comment("-------".into(), Placement::Right),
            Event::Scalar("foobar".into(), ScalarStyle::Plain, 0, None),
            Event::DocumentEnd,
            Event::StreamEnd,
        ])
    );
}

#[test]
fn test_directive_followed_by_comment_then_content_errors() {
    for yaml in ["%YAML 1.2\n# c\nfoo\n--- bar\n", "%YAML 1.2\n# c\n"] {
        let error = run_parser(yaml)
            .expect_err("directives must still be followed by an explicit document start");

        assert_eq!(error.info(), "did not find expected <document start>");
    }
}

#[test]
fn test_empty_block_scalar_value_does_not_depend_on_eof() {
    fn value_of(yaml: &str) -> String {
        run_parser(yaml)
            .unwrap()
            .into_iter()
            .find_map(|event| match event {
                Event::Scalar(value, ScalarStyle::Literal, _, _) => Some(value.into_owned()),
                _ => None,
            })
            .unwrap()
    }

    assert_eq!(value_of("a: |\nb: c\n"), value_of("a: |\n"));
    assert_eq!(value_of("a: |+\nb: c\n"), value_of("a: |+\n"));
    assert_eq!(value_of("a: |+\n\n"), "\n");
}

#[test]
fn test_large_block_scalar_indent() {
    // https://github.com/Ethiraric/yaml-rust2/issues/29
    // https://github.com/saphyr-rs/saphyr-parser/issues/2
    // Tests the `loop` fallback of `skip_block_scalar_indent`. The indent in the YAML string must
    // be greater than `BUFFER_LEN - 2`. The second line is further indented with spaces, and the
    // resulting string should be "a\n    b".
    let s = "
a: |-
                  a
                      b
";

    assert_eq!(
        run_parser(s).unwrap(),
        [
            Event::StreamStart,
            Event::DocumentStart(false, None),
            Event::MappingStart(StructureStyle::Block, 0, None),
            Event::Scalar("a".into(), ScalarStyle::Plain, 0, None),
            Event::Scalar("a\n    b".into(), ScalarStyle::Literal, 0, None),
            Event::MappingEnd,
            Event::DocumentEnd,
            Event::StreamEnd,
        ]
    );
}

#[test]
fn test_bad_docstart() {
    run_parser("---This used to cause an infinite loop").unwrap();
    assert_eq!(
        run_parser("----").unwrap(),
        [
            Event::StreamStart,
            Event::DocumentStart(false, None),
            Event::Scalar("----".into(), ScalarStyle::Plain, 0, None),
            Event::DocumentEnd,
            Event::StreamEnd,
        ]
    );

    assert_eq!(
        run_parser("--- #comment").unwrap(),
        expected_events([
            Event::StreamStart,
            Event::DocumentStart(true, None),
            #[cfg(feature = "comments")]
            Event::Comment("comment".into(), Placement::Right),
            Event::Scalar("~".into(), ScalarStyle::Plain, 0, None),
            Event::DocumentEnd,
            Event::StreamEnd,
        ])
    );

    assert_eq!(
        run_parser("---- #comment").unwrap(),
        expected_events([
            Event::StreamStart,
            Event::DocumentStart(false, None),
            Event::Scalar("----".into(), ScalarStyle::Plain, 0, None),
            #[cfg(feature = "comments")]
            Event::Comment("comment".into(), Placement::Right),
            Event::DocumentEnd,
            Event::StreamEnd,
        ])
    );
}

#[test]
fn test_indentation_equality() {
    let four_spaces = run_parser(
        r"
hash:
    with:
        indentations
",
    )
    .unwrap();

    let two_spaces = run_parser(
        r"
hash:
  with:
    indentations
",
    )
    .unwrap();

    let one_space = run_parser(
        r"
hash:
 with:
  indentations
",
    )
    .unwrap();

    let mixed_spaces = run_parser(
        r"
hash:
     with:
               indentations
",
    )
    .unwrap();

    for (((a, b), c), d) in four_spaces
        .iter()
        .zip(two_spaces.iter())
        .zip(one_space.iter())
        .zip(mixed_spaces.iter())
    {
        assert_eq!(a, b);
        assert_eq!(a, c);
        assert_eq!(a, d);
    }
}

#[test]
fn test_recursion_depth_check_objects() {
    let s = "{a:".repeat(10_000) + &"}".repeat(10_000);
    assert!(run_parser(&s).is_err());
}

#[test]
fn test_recursion_depth_check_arrays() {
    let s = "[".repeat(10_000) + &"]".repeat(10_000);
    assert!(run_parser(&s).is_err());
}
