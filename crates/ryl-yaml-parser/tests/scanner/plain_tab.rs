use ryl_yaml_parser::{
    ErrorKind, Event, Options, Parser, ScalarStyle, ScanError, StructureStyle,
};

fn collect_events(input: &str) -> Result<Vec<Event<'_>>, ScanError> {
    collect_events_with_options(input, Options::default())
}

fn collect_events_with_options(
    input: &str,
    options: Options,
) -> Result<Vec<Event<'_>>, ScanError> {
    let str_events = Parser::new_from_str_with_options(input, options.clone())
        .collect::<Result<Vec<_>, _>>();
    let iter_events =
        Parser::new_from_iter_with_options(input.chars(), options.clone())
            .collect::<Result<Vec<_>, _>>();
    let fallible_events = Parser::new_from_fallible_iter_with_options(
        input.chars().map(Ok::<_, ErrorKind>),
        options,
    )
    .collect::<Result<Vec<_>, _>>();
    assert_eq!(str_events, iter_events, "input: {input:?}");
    assert_eq!(str_events, fallible_events, "input: {input:?}");
    Ok(str_events?.into_iter().map(|(event, _)| event).collect())
}

fn collect_scalars(input: &str) -> Result<Vec<String>, ScanError> {
    Ok(collect_events(input)?
        .into_iter()
        .filter_map(|event| match event {
            Event::Scalar(value, ..) => Some(value.into_owned()),
            _ => None,
        })
        .collect())
}

fn collect_structural_events_with_options(
    input: &str,
    options: Options,
) -> Result<Vec<Event<'_>>, ScanError> {
    Ok(collect_events_with_options(input, options)?
        .into_iter()
        .filter(|event| !matches!(event, Event::Comment(..)))
        .collect())
}

fn assert_structural_events_in_all_modes(input: &str, expected: &[Event<'_>]) {
    for emit_comments in [false, true] {
        for strict_indentation in [false, true] {
            let options = ryl_yaml_parser::options! {
                emit_comments: emit_comments,
                strict_indentation: strict_indentation,
            };
            let events = collect_structural_events_with_options(input, options)
                .unwrap_or_else(|error| panic!("{input:?}: {error}"));
            assert_eq!(
                events, expected,
                "{input:?}, emit_comments={emit_comments}, strict_indentation={strict_indentation}"
            );
        }
    }
}

#[test]
fn tabs_separate_mapping_values_in_block_and_flow_contexts() {
    for value in [
        "1", "-1", "true", "false", "null", "value", "-item", "_value", "äöü",
    ] {
        for separator in ["\t", "\t\t", "\t ", " \t"] {
            for (yaml, mapping_style, key_style) in [
                (
                    format!("{{\"key\":{separator}{value}}}"),
                    StructureStyle::Flow,
                    ScalarStyle::DoubleQuoted,
                ),
                (
                    format!("key:{separator}{value}\n"),
                    StructureStyle::Block,
                    ScalarStyle::Plain,
                ),
                (
                    format!("? key\n:{separator}{value}\n"),
                    StructureStyle::Block,
                    ScalarStyle::Plain,
                ),
            ] {
                assert_eq!(
                    collect_events(&yaml).unwrap(),
                    [
                        Event::StreamStart,
                        Event::DocumentStart(false, None),
                        Event::MappingStart(mapping_style, 0, None),
                        Event::Scalar("key".into(), key_style, 0, None),
                        Event::Scalar(value.into(), ScalarStyle::Plain, 0, None),
                        Event::MappingEnd,
                        Event::DocumentEnd,
                        Event::StreamEnd,
                    ],
                    "input: {yaml:?}",
                );
            }
        }
    }
}

#[test]
fn tabs_separate_explicit_keys_in_flow_collections() {
    // Tabs are ordinary s-white separation within flow collections.
    for separator in ["\t", " \t", "\t ", "\n\t"] {
        for (yaml, expected) in [
            (format!("{{?{separator}a: b}}"), vec!["a", "b"]),
            (format!("[?{separator}a]"), vec!["a", "~"]),
            (format!("{{?{separator}}}"), vec!["~", "~"]),
        ] {
            assert_eq!(
                collect_scalars(&yaml).unwrap_or_else(|e| panic!("{yaml:?}: {e}")),
                expected,
                "input: {yaml:?}",
            );
        }
    }
}

#[test]
fn tabs_separate_explicit_block_keys_without_becoming_indentation() {
    for separator in ["\t", " \t", "\t ", "\t\t"] {
        for key in [
            "key",
            "-item",
            "'quoted'",
            "\"quoted\"",
            "[a, b]",
            "{a: b}",
            "&a !local key",
            "|\n  key",
        ] {
            let yaml = format!("?{separator}{key}\n: value\n");
            let reference = format!("? {key}\n: value\n");
            for strict_indentation in [false, true] {
                let options = ryl_yaml_parser::options! { strict_indentation: strict_indentation };
                assert_eq!(
                    collect_events_with_options(&yaml, options.clone())
                        .unwrap_or_else(|error| panic!("{yaml:?}: {error}")),
                    collect_events_with_options(&reference, options).unwrap(),
                    "{yaml:?}"
                );
            }
        }
    }

    for (yaml, expected) in [
        ("?\t\n: value\n", ["~", "value"].as_slice()),
        ("?\tkey\n", ["key", "~"].as_slice()),
        ("?\t", ["~", "~"].as_slice()),
        (
            "outer:\n  ?\tkey\n  : value\n",
            ["outer", "key", "value"].as_slice(),
        ),
        ("- ?\tkey\n  : value\n", ["key", "value"].as_slice()),
        (
            "?\t&a key\n: value\n?\t*a\n: other\n",
            ["key", "value", "other"].as_slice(),
        ),
    ] {
        assert_eq!(collect_scalars(yaml).unwrap(), expected, "{yaml:?}");
    }
}

#[test]
fn explicit_key_tabs_after_comments_require_space_indentation() {
    let scalar_key_events = [
        Event::StreamStart,
        Event::DocumentStart(false, None),
        Event::MappingStart(StructureStyle::Block, 0, None),
        Event::Scalar("key".into(), ScalarStyle::Plain, 0, None),
        Event::Scalar("value".into(), ScalarStyle::Plain, 0, None),
        Event::MappingEnd,
        Event::DocumentEnd,
        Event::StreamEnd,
    ];
    let mapping_key_events = [
        Event::StreamStart,
        Event::DocumentStart(false, None),
        Event::MappingStart(StructureStyle::Block, 0, None),
        Event::MappingStart(StructureStyle::Block, 0, None),
        Event::Scalar("key".into(), ScalarStyle::Plain, 0, None),
        Event::Scalar("nested".into(), ScalarStyle::Plain, 0, None),
        Event::MappingEnd,
        Event::Scalar("value".into(), ScalarStyle::Plain, 0, None),
        Event::MappingEnd,
        Event::DocumentEnd,
        Event::StreamEnd,
    ];
    for (yaml, expected) in [
        ("?\t# key\n \tkey\n: value\n", scalar_key_events.as_slice()),
        (
            "? # key\n\t# another\n \tkey\n: value\n",
            scalar_key_events.as_slice(),
        ),
        (
            "?\r\n\t\r\n \tkey\r\n: value\r\n",
            scalar_key_events.as_slice(),
        ),
        (
            "?\n\t# comment\n \tkey\n: value\n",
            scalar_key_events.as_slice(),
        ),
        (
            "?\t\n  key: nested\n: value\n",
            mapping_key_events.as_slice(),
        ),
    ] {
        for emit_comments in [false, true] {
            for strict_indentation in [false, true] {
                let options = ryl_yaml_parser::options! { emit_comments: emit_comments, strict_indentation: strict_indentation };
                let events: Vec<_> = collect_events_with_options(yaml, options)
                    .unwrap_or_else(|error| panic!("{yaml:?}: {error}"))
                    .into_iter()
                    .filter(|event| !matches!(event, Event::Comment(..)))
                    .collect();
                assert_eq!(
                    events, expected,
                    "{yaml:?}, emit_comments={emit_comments}, strict_indentation={strict_indentation}",
                );
            }
        }
    }
    for yaml in [
        "?\t-\n",
        "? \t-\n",
        "?\tkey:\n",
        "? \tkey:\n",
        "?\t? key\n",
        "?\n \tkey: value\n",
    ] {
        assert!(
            collect_events(yaml).is_err(),
            "compact collection accepts tab indentation: {yaml:?}"
        );
    }
    for yaml in [
        "?\n\tkey\n: value\n",
        "?\t# key\n\tkey\n: value\n",
        "outer:\n  ?\n  \tkey\n  : value\n",
    ] {
        assert_eq!(
            collect_events(yaml).unwrap_err().kind(),
            &ErrorKind::TabNotAllowed,
            "{yaml:?}"
        );
    }
}

#[test]
fn explicit_key_properties_survive_tab_separation_and_comment_lines() {
    // The key node retains both properties across s-separate(1,FLOW-OUT); the next key
    // aliases that collection rather than defining a new anchor or losing its tag.
    let expected = [
        Event::StreamStart,
        Event::DocumentStart(false, None),
        Event::MappingStart(StructureStyle::Block, 0, None),
        Event::SequenceStart(
            StructureStyle::Flow,
            1,
            Some(std::borrow::Cow::Owned(ryl_yaml_parser::Tag::new(
                "!", "local",
            ))),
        ),
        Event::Scalar("é".into(), ScalarStyle::Plain, 0, None),
        Event::Scalar("item".into(), ScalarStyle::Plain, 0, None),
        Event::SequenceEnd,
        Event::Scalar("first".into(), ScalarStyle::Plain, 0, None),
        Event::Alias(1),
        Event::Scalar("second".into(), ScalarStyle::Plain, 0, None),
        Event::Scalar("next".into(), ScalarStyle::Plain, 0, None),
        Event::Scalar("done".into(), ScalarStyle::Plain, 0, None),
        Event::MappingEnd,
        Event::DocumentEnd,
        Event::StreamEnd,
    ];
    for properties in [
        "&a !local # properties\n\t# comment",
        "&a # anchor\n \t!local # tag",
    ] {
        let yaml = format!(
            "?\t{properties}\n \t[é, item]\n: first\n?\t*a\n: second\nnext: done\n"
        );
        for line_break in ["\n", "\r", "\r\n"] {
            assert_structural_events_in_all_modes(
                &yaml.replace('\n', line_break),
                &expected,
            );
        }
    }
}

#[test]
fn explicit_key_tab_state_ends_at_a_line_break_and_respects_parent_dedents() {
    // A tab cannot indent a compact mapping on its own line, but a subsequent line can
    // introduce a block mapping with space indentation. Dedents close the key and parent.
    let yaml = "- ?\t# key follows\n\t# comment\n    inner: value\n  : selected\n  next: done\n- tail\n";
    let expected = [
        Event::StreamStart,
        Event::DocumentStart(false, None),
        Event::SequenceStart(StructureStyle::Block, 0, None),
        Event::MappingStart(StructureStyle::Block, 0, None),
        Event::MappingStart(StructureStyle::Block, 0, None),
        Event::Scalar("inner".into(), ScalarStyle::Plain, 0, None),
        Event::Scalar("value".into(), ScalarStyle::Plain, 0, None),
        Event::MappingEnd,
        Event::Scalar("selected".into(), ScalarStyle::Plain, 0, None),
        Event::Scalar("next".into(), ScalarStyle::Plain, 0, None),
        Event::Scalar("done".into(), ScalarStyle::Plain, 0, None),
        Event::MappingEnd,
        Event::Scalar("tail".into(), ScalarStyle::Plain, 0, None),
        Event::SequenceEnd,
        Event::DocumentEnd,
        Event::StreamEnd,
    ];
    for line_break in ["\n", "\r", "\r\n"] {
        assert_structural_events_in_all_modes(
            &yaml.replace('\n', line_break),
            &expected,
        );
    }
}

#[test]
fn explicit_key_property_indentation_is_strict_only() {
    // YAML requires n+1 spaces before the key's next content line. Relaxed mode
    // accepts a tab indenting a scalar key after anchor/tag properties.
    for (yaml, anchor, tagged, in_sequence) in [
        ("?\t&a\n\tkey\n: value\n", 1, false, false),
        ("?\t!local # property\n\tkey\n: value\n", 0, true, false),
        (
            "?\t&a # property\n\t# comment\n\tkey\n: value\n",
            1,
            false,
            false,
        ),
        ("- ?\t&a # property\n  \tkey\n  : value\n", 1, false, true),
        (
            "?\t# before property\n &a\n\tkey\n: value\n",
            1,
            false,
            false,
        ),
        (
            "?\t# before property\n !local\n\tkey\n: value\n",
            0,
            true,
            false,
        ),
    ] {
        let mut expected = vec![Event::StreamStart, Event::DocumentStart(false, None)];
        if in_sequence {
            expected.push(Event::SequenceStart(StructureStyle::Block, 0, None));
        }
        expected.extend([
            Event::MappingStart(StructureStyle::Block, 0, None),
            Event::Scalar(
                "key".into(),
                ScalarStyle::Plain,
                anchor,
                tagged.then(|| {
                    std::borrow::Cow::Owned(ryl_yaml_parser::Tag::new("!", "local"))
                }),
            ),
            Event::Scalar("value".into(), ScalarStyle::Plain, 0, None),
            Event::MappingEnd,
        ]);
        if in_sequence {
            expected.push(Event::SequenceEnd);
        }
        expected.extend([Event::DocumentEnd, Event::StreamEnd]);

        for emit_comments in [false, true] {
            let strict = ryl_yaml_parser::options! {
                emit_comments: emit_comments,
                strict_indentation: true,
            };
            let error = collect_events_with_options(yaml, strict).unwrap_err();
            assert_eq!(error.kind(), &ErrorKind::TabNotAllowed, "{yaml:?}");

            let relaxed = ryl_yaml_parser::options! {
                emit_comments: emit_comments,
                strict_indentation: false,
            };
            assert_eq!(
                collect_structural_events_with_options(yaml, relaxed).unwrap(),
                expected,
                "{yaml:?}, emit_comments={emit_comments}"
            );
        }
    }
}

#[test]
fn tabs_cannot_indent_compact_explicit_key_collections_in_either_mode() {
    for yaml in [
        "- ?\tkey:\n  : value\n",
        "? &a\n  \tkey: value\n",
        "? !t\n  \t- item\n",
    ] {
        for emit_comments in [false, true] {
            for strict_indentation in [false, true] {
                let options = ryl_yaml_parser::options! {
                    emit_comments: emit_comments,
                    strict_indentation: strict_indentation,
                };
                assert!(
                    collect_events_with_options(yaml, options).is_err(),
                    "tab supplied compact collection indentation: {yaml:?}, emit_comments={emit_comments}, strict_indentation={strict_indentation}"
                );
            }
        }
    }
}

#[test]
fn tabs_after_flow_explicit_keys_preserve_collection_keys_and_alias_values() {
    for separator in ["\t", "\t# key\r\n "] {
        let yaml = format!(
            "{{?{separator}[é, b]: &v value, ?{separator}{{nested: key}}: *v}}"
        );
        let events: Vec<_> = collect_events(&yaml)
            .unwrap()
            .into_iter()
            .filter(|event| !matches!(event, Event::Comment(..)))
            .collect();
        assert_eq!(
            events,
            [
                Event::StreamStart,
                Event::DocumentStart(false, None),
                Event::MappingStart(StructureStyle::Flow, 0, None),
                Event::SequenceStart(StructureStyle::Flow, 0, None),
                Event::Scalar("é".into(), ScalarStyle::Plain, 0, None),
                Event::Scalar("b".into(), ScalarStyle::Plain, 0, None),
                Event::SequenceEnd,
                Event::Scalar("value".into(), ScalarStyle::Plain, 1, None),
                Event::MappingStart(StructureStyle::Flow, 0, None),
                Event::Scalar("nested".into(), ScalarStyle::Plain, 0, None),
                Event::Scalar("key".into(), ScalarStyle::Plain, 0, None),
                Event::MappingEnd,
                Event::Alias(1),
                Event::MappingEnd,
                Event::DocumentEnd,
                Event::StreamEnd,
            ],
            "{yaml:?}"
        );
    }

    // Leaving flow context must restore the prohibition on a tab indenting a block key.
    let error =
        collect_events("first: {?\tkey: value}\n?\n\tinvalid\n: value\n").unwrap_err();
    assert_eq!(error.kind(), &ErrorKind::TabNotAllowed);
    assert_eq!((error.marker().line(), error.marker().col()), (3, 0));
}

#[test]
fn tabs_after_colons_preserve_value_and_comment_parsing() {
    for (yaml, expected) in [
        ("key:\t\"value\"\n", "value"),
        ("key:\t|\n  value\n", "value\n"),
        ("key:\t[value]\n", "value"),
        ("[key:\tvalue]\n", "value"),
        ("? key\n:\tvalue\n", "value"),
        ("? key\n:\t[value]\n", "value"),
        ("  key:\tvalue\n", "value"),
        ("key:\tvalue # comment\n", "value"),
        ("key:\t# comment\n  value\n", "value"),
        ("key:\t\n  value\n", "value"),
        ("{key:\t# comment\nvalue}\n", "value"),
    ] {
        assert_eq!(
            collect_scalars(yaml).unwrap(),
            ["key", expected],
            "input: {yaml:?}",
        );
    }
}

#[test]
fn tabs_after_colons_preserve_empty_values_and_following_entries() {
    for yaml in [
        "key:\t",
        "key:\t\n",
        "{key:\t}",
        "[key:\t]",
        "key:\t# empty\n",
        "{key:\t# empty\n}",
        "[key:\t# empty\n]",
    ] {
        assert_eq!(
            collect_scalars(yaml).unwrap(),
            ["key", "~"],
            "input: {yaml:?}"
        );
    }

    for yaml in [
        "key:\t# empty\nnext:\tvalue\n",
        "{key:\t, next:\tvalue}",
        "{key:\t# empty\n, next:\tvalue}",
        "[key:\t, next:\tvalue]",
    ] {
        assert_eq!(
            collect_scalars(yaml).unwrap(),
            ["key", "~", "next", "value"],
            "input: {yaml:?}",
        );
    }
}

#[test]
fn tabs_after_colons_preserve_tagged_anchors_and_aliases() {
    for (yaml, style) in [
        ("key:\t&a !local value\ncopy:\t*a\n", StructureStyle::Block),
        ("{key:\t&a !local value, copy:\t*a}", StructureStyle::Flow),
    ] {
        assert_eq!(
            collect_events(yaml).unwrap(),
            [
                Event::StreamStart,
                Event::DocumentStart(false, None),
                Event::MappingStart(style, 0, None),
                Event::Scalar("key".into(), ScalarStyle::Plain, 0, None),
                Event::Scalar(
                    "value".into(),
                    ScalarStyle::Plain,
                    1,
                    Some(std::borrow::Cow::Owned(ryl_yaml_parser::Tag::new(
                        "!", "local"
                    ))),
                ),
                Event::Scalar("copy".into(), ScalarStyle::Plain, 0, None),
                Event::Alias(1),
                Event::MappingEnd,
                Event::DocumentEnd,
                Event::StreamEnd,
            ],
            "input: {yaml:?}",
        );
    }
}

#[test]
fn tabs_after_colons_do_not_allow_tab_indentation() {
    for yaml in [
        "key:\n\tvalue\n",
        "key:\t\n\tvalue\n",
        "key:\t# comment\n\tvalue\n",
    ] {
        assert_eq!(
            collect_scalars(yaml).unwrap_err().kind(),
            &ErrorKind::TabInBlockIndentation,
            "input: {yaml:?}",
        );
    }
}

#[test]
fn tabs_after_colons_cannot_indent_compact_block_collections() {
    for (value, expected) in [
        ("- value", vec!["key", "value"]),
        ("child: value", vec!["key", "child", "value"]),
        ("\"child\": value", vec!["key", "child", "value"]),
    ] {
        let yaml = format!("? key\n: {value}\n");
        assert_eq!(collect_scalars(&yaml).unwrap(), expected);

        for separator in ["\t", "\t\t", "\t ", " \t"] {
            let yaml = format!("? key\n:{separator}{value}\n");
            assert!(collect_scalars(&yaml).is_err(), "input: {yaml:?}");

            // A line break permits a block collection with its own space indentation.
            let yaml = format!("? key\n:{separator}# comment\n  {value}\n");
            assert_eq!(collect_scalars(&yaml).unwrap(), expected, "input: {yaml:?}");
        }
    }
}

#[test]
fn tab_in_block_literal_body_is_allowed() {
    // A tab in the body of a literal block scalar (|) should be accepted.
    let yaml = "key: |\n  a\tb"; // 'a\tb' inside the block content
    let scalars = collect_scalars(yaml)
        .expect("parser should accept tab inside block scalar body");
    // Literal style preserves newlines; a single content line ends with a trailing \n
    assert_eq!(scalars, vec!["key".to_string(), "a\tb\n".to_string()]);
}

#[test]
fn tab_in_block_folded_body_is_allowed() {
    // A tab in the body of a folded block scalar (>) should be accepted as content.
    let yaml = "key: >\n  a\tb";
    let scalars = collect_scalars(yaml)
        .expect("parser should accept tab inside folded block scalar body");
    // For a single content line, folded and literal both end with a trailing \n
    assert_eq!(scalars, vec!["key".to_string(), "a\tb\n".to_string()]);
}

#[test]
fn tab_at_start_of_block_scalar_is_rejected() {
    // If the first content character of the block scalar is a tab, it must be rejected.
    // This means the content line starts with a tab instead of spaces for indentation.
    let yaml = "key: |\n\tvalue";

    let mut got_err: Option<ScanError> = None;
    for item in Parser::new_from_str(yaml) {
        match item {
            Ok(_) => {}
            Err(e) => {
                got_err = Some(e);
                break;
            }
        }
    }

    let err = got_err.expect(
        "expected a ScanError due to leading tab at start of block scalar content",
    );
    // The scanner has a specific error for this case.
    assert_eq!(err.kind(), &ErrorKind::TabAtBlockScalarStart);
    assert!(
        err.info()
            .contains("a block scalar content cannot start with a tab"),
        "unexpected error message: {}",
        err.info()
    );
}

#[test]
fn explicit_indent_root_literal_can_end_before_document_start() {
    let yaml = "|2\n---\n";
    let scalars = collect_scalars(yaml)
        .expect("parser should accept empty explicit-indented block scalar");
    assert_eq!(scalars.first().map(String::as_str), Some(""));
}

#[test]
fn explicit_indent_root_literal_can_end_before_comment() {
    let yaml = "|2\n# comment\n";
    let scalars = collect_scalars(yaml).expect(
        "parser should accept empty explicit-indented block scalar before comment",
    );
    assert_eq!(scalars, vec![String::new()]);
}
