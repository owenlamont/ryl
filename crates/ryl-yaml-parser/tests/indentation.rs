use granit_parser::{Event, Parser, ScalarStyle, StructureStyle};

fn scalar_value<'a>(ev: &'a Event<'_>) -> Option<&'a str> {
    match ev {
        Event::Scalar(v, ..) => Some(v.as_ref()),
        _ => None,
    }
}

fn block_scalar_indents(yaml: &str) -> Vec<(String, Option<usize>)> {
    Parser::new_from_str(yaml)
        .map(|event| event.expect("valid yaml"))
        .filter_map(|(event, span)| match event {
            Event::Scalar(value, ScalarStyle::Literal | ScalarStyle::Folded, ..) => {
                Some((value.into_owned(), span.indent))
            }
            _ => None,
        })
        .collect()
}

fn first_error_info(yaml: &str) -> Option<String> {
    Parser::new_from_str(yaml).find_map(|event| event.err().map(|err| err.info()))
}

#[test]
fn indentation_is_reported_for_block_mapping_keys_only() {
    let yaml = "a: b\n";

    let mut scalars = Vec::new();
    for x in Parser::new_from_str(yaml) {
        let (ev, span) = x.expect("valid yaml");
        if let Some(v) = scalar_value(&ev) {
            scalars.push((v.to_string(), span.indent));
        }
    }

    // In a mapping, the first scalar is the key and must carry indentation (col=0).
    // The value must not carry indentation.
    assert!(scalars.contains(&("a".to_string(), Some(0))));
    assert!(scalars.contains(&("b".to_string(), None)));
}

#[test]
fn indentation_is_not_reported_in_flow_mappings() {
    let yaml = "{ a: b }\n";

    for x in Parser::new_from_str(yaml) {
        let (ev, span) = x.expect("valid yaml");
        if let Some(v) = scalar_value(&ev) {
            if v == "a" || v == "b" {
                assert_eq!(span.indent, None);
            }
        }
    }
}

fn assert_same_events(yaml: &str, reference: &str) {
    let expected: Vec<_> = Parser::new_from_str(reference)
        .map(|result| result.expect("reference should parse").0)
        .collect();
    for result in [
        Parser::new_from_str(yaml).collect::<Result<Vec<_>, _>>(),
        Parser::new_from_iter(yaml.chars()).collect::<Result<Vec<_>, _>>(),
    ] {
        let events: Vec<_> = result
            .unwrap_or_else(|error| panic!("input: {yaml:?}: {error}"))
            .into_iter()
            .map(|(event, _)| event)
            .collect();
        assert_eq!(events, expected, "input: {yaml:?}");
    }
}

#[test]
fn under_indented_flow_sequence_entries_are_accepted() {
    // Issue #33: entries may be at or below the enclosing block's indentation,
    // as accepted by PyYAML and ruamel.yaml. Following block nodes keep their structure.
    for (prefix, suffix) in [
        ("key: [", "]\nafter: done\n"),
        ("- targets: [", "]\n  after: done\n- next\n"),
        ("outer:\n  inner: [", "]\n  after: done\nlast: end\n"),
        ("key:\n  [", "]\nafter: done\n"),
    ] {
        for quote in ["", "'", "\""] {
            let first = format!("{quote}192.168.1.1:9100{quote}");
            let second = format!("{quote}192.168.1.2:9100{quote}");
            let reference = format!("{prefix}{first}, {second},{suffix}");
            for indent in 0..=3 {
                let spaces = " ".repeat(indent);
                let yaml = format!("{prefix}\n{spaces}{first},\n{spaces}{second},\n{suffix}");
                assert_same_events(&yaml, &reference);
            }
        }
    }
}

#[test]
fn under_indented_flow_mappings_and_nested_nodes_are_accepted() {
    for (yaml, reference) in [
        (
            "outer:\n  key: {\none:\n[first,\nsecond],\nthree: {four: five}\n}\n  after: done\n",
            "outer:\n  key: {one: [first, second], three: {four: five}}\n  after: done\n",
        ),
        (
            "key: {\n\"one\":\n\"value\",\n'other': 'value'\n}\nafter: done\n",
            "key: {\"one\": \"value\", 'other': 'value'}\nafter: done\n",
        ),
        (
            "key: [\n&ref !custom value,\n*ref,\n{? nested: [one, two]}\n]\nafter: done\n",
            "key: [&ref !custom value, *ref, {? nested: [one, two]}]\nafter: done\n",
        ),
        (
            "key: [\nfirst\n, second\n]\nafter: done\n",
            "key: [first, second]\nafter: done\n",
        ),
        (
            "key: [ # start\n# before entry\nvalue, # after entry\n]\nafter: done\n",
            "key: [ # start\n  # before entry\n  value, # after entry\n]\nafter: done\n",
        ),
        (
            "key: {\n?\nname\n:\nvalue\n}\nafter: done\n",
            "key: {? name: value}\nafter: done\n",
        ),
        (
            "key: [\nfirst\nsecond,\n\"third\nfourth\"\n]\n",
            "key: [first second, \"third fourth\"]\n",
        ),
    ] {
        assert_same_events(yaml, reference);
    }
}

#[test]
fn flow_key_properties_do_not_hide_under_indented_multiline_keys() {
    for properties in ["&a", "!!str", "&a !!str", "!!str &a"] {
        for key in ["name", "'name'", "\"name\""] {
            for body in [
                format!("{key}\n: value"),
                format!("{key}\n  : value"),
                format!("{key}: value"),
                format!("  {key}\n: value"),
                format!("# before key\n{key}\n  : value"),
            ] {
                let yaml = format!("k: {{ {properties}\n{body}\n}}\n");
                for result in [
                    Parser::new_from_str(&yaml).collect::<Result<Vec<_>, _>>(),
                    Parser::new_from_iter(yaml.chars()).collect::<Result<Vec<_>, _>>(),
                ] {
                    assert!(result.is_err(), "invalid input accepted: {yaml:?}");
                }
            }
        }
    }
}

#[test]
fn flow_key_properties_preserve_supported_multiline_keys() {
    for properties in ["&a", "!!str", "&a !!str", "!!str &a"] {
        for key in ["name", "'name'", "\"name\""] {
            let reference = format!("k: {{{properties} {key}: value}}\n");
            for yaml in [
                format!("k: {{ {properties}\n  {key}\n  : value\n}}\n"),
                format!("k: {{\n{properties} {key}: value\n}}\n"),
                format!("k: {{ ? {properties}\n{key}\n: value\n}}\n"),
            ] {
                assert_same_events(&yaml, &reference);
            }
            assert_same_events(
                &format!("k: {{ {properties}\n# before key\n  {key}\n  : value\n}}\n"),
                &format!("k: {{ {properties}\n# before key\n  {key}: value\n}}\n"),
            );
        }
    }
}

#[test]
fn relaxed_flow_indentation_still_rejects_invalid_structure() {
    for yaml in [
        "outer:\n  key: value\n other: value\n",
        "key: [\nvalue\n[nested]\n]\n",
        "key: {\none: value\nother: value\n}\n",
        "key: [\nvalue\n}\n",
        "key: [\n|\nvalue\n]\n",
        // These implicit multiline keys are also rejected by PyYAML and ruamel.yaml.
        "k: {\nk\n:\nv\n}\n", // YAML test suite VJP3-00.
        "k: {\n\"k\"\n:\nv\n}\n",
        "k: {\n'k'\n:\nv\n}\n",
        "k: {\nmulti\nline: value\n}\n",
        "k: { &a\n!!str name\n  : value\n}\n",
        "k: { !!str\n&a name\n  : value\n}\n",
    ] {
        for result in [
            Parser::new_from_str(yaml).collect::<Result<Vec<_>, _>>(),
            Parser::new_from_iter(yaml.chars()).collect::<Result<Vec<_>, _>>(),
        ] {
            assert!(result.is_err(), "invalid input accepted: {yaml:?}");
        }
    }
}

#[test]
fn indentation_is_reported_for_nested_block_mapping_keys() {
    let yaml = "a:\n  b: c\n";

    let mut a_indent = None;
    let mut b_indent = None;
    let mut c_indent = None;

    for x in Parser::new_from_str(yaml) {
        let (ev, span) = x.expect("valid yaml");
        if let Some(v) = scalar_value(&ev) {
            match v {
                "a" => a_indent = span.indent,
                "b" => b_indent = span.indent,
                "c" => c_indent = span.indent,
                _ => {}
            }
        }
    }

    assert_eq!(a_indent, Some(0));
    assert_eq!(b_indent, Some(2));
    assert_eq!(c_indent, None);
}

#[test]
fn queued_key_node_after_comment_keeps_key_indent() {
    let yaml = "? - # key sequence comment\n    item\n: value\n";

    let mut key_sequence_indent = None;

    for next in Parser::new_from_str(yaml) {
        let (event, span) = next.expect("valid yaml");
        if matches!(event, Event::SequenceStart(..)) {
            key_sequence_indent = span.indent;
            break;
        }
    }

    assert_eq!(key_sequence_indent, Some(0));
}

#[test]
fn indentation_is_reported_for_block_scalar_content() {
    let yaml = "key: |\n  body\n";

    assert_eq!(
        block_scalar_indents(yaml),
        vec![("body\n".to_string(), Some(2))]
    );
}

#[test]
fn indentation_is_not_reported_for_whitespace_only_block_scalar_content() {
    let yaml = "key: |+\n  \n";

    assert_eq!(block_scalar_indents(yaml), vec![("\n".to_string(), None)]);
}

#[test]
fn root_block_sequence_can_have_anchor_on_previous_line() {
    let yaml = "&anchor\n- a\n- b\n";
    let events = Parser::new_from_str(yaml)
        .map(|event| event.expect("valid yaml").0)
        .collect::<Vec<_>>();

    assert!(events
        .iter()
        .any(|event| matches!(event, Event::SequenceStart(StructureStyle::Block, 1, None))));
}

#[test]
fn indented_mapping_value_sequence_can_have_anchor_and_comment_on_previous_lines() {
    let yaml = "seq:\n  &anchor\n  # c\n  - a\n  - b\n";
    let events = Parser::new_from_str(yaml)
        .map(|event| event.expect("valid yaml").0)
        .collect::<Vec<_>>();

    assert!(events
        .iter()
        .any(|event| matches!(event, Event::SequenceStart(StructureStyle::Block, 1, None))));
}

#[test]
fn unindented_mapping_value_sequence_after_anchor_is_rejected() {
    assert_eq!(
        first_error_info("seq:\n&anchor\n- a\n- b\n").as_deref(),
        Some("simple key expected ':'")
    );
}

#[test]
fn unindented_mapping_value_sequence_after_anchor_comment_is_rejected() {
    assert_eq!(
        first_error_info("seq:\n&anchor\n# c\n- a\n- b\n").as_deref(),
        Some("simple key expected ':'")
    );
}

#[test]
fn unindented_mapping_value_sequence_after_tag_comment_is_rejected() {
    assert_eq!(
        first_error_info("seq:\n!tag\n# c\n- a\n- b\n").as_deref(),
        Some("simple key expected ':'")
    );
}
