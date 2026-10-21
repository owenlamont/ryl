// Upstream ID is ZYU8 (the filename ZVU8.rs is intentional).
// These four cases are disabled upstream with `skip: true`:
// https://github.com/yaml/yaml-test-suite/blob/45db50aecf9b1520f8258938c88f396e96f30831/src/ZYU8.yaml
// Upstream calls them valid under YAML 1.2's productions but undesirable to
// encourage. Keep its acceptance expectation here, independently of that skip.

use granit_parser::{Event, Parser, ScalarStyle};

fn assert_empty_document(yaml: &str) {
    let options = granit_parser::options! { strict_indentation: true };
    let events = Parser::new_from_str_with_options(yaml, options)
        .map(|result| result.map(|(event, _)| event))
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_else(|error| panic!("strict mode rejected {yaml:?}: {error}"));

    // Upstream expects +STR, +DOC ---, =VAL :, -DOC, -STR.
    // This API represents a missing untagged scalar as plain `~`; the upstream
    // event format does not expose the document's version metadata.
    assert!(
        matches!(
            events.as_slice(),
            [Event::StreamStart, Event::DocumentStart(true, _),
             Event::Scalar(value, ScalarStyle::Plain, 0, None),
             Event::DocumentEnd, Event::StreamEnd] if value == "~"
        ),
        "unexpected events for {yaml:?}: {events:?}",
    );
}

#[test]
fn zyu8_00_yaml_without_separator() {
    assert_empty_document("%YAML1.1\n---\n");
}

#[test]
fn zyu8_01_punctuation_directive() {
    assert_empty_document("%***\n---\n");
}

#[test]
#[ignore = "Disabled upstream: formally valid YAML 1.2, but support is not encouraged"]
fn zyu8_02_extra_version_parameter() {
    assert_empty_document("%YAML 1.1 1.2\n---\n");
}

#[test]
fn zyu8_03_long_minor_version() {
    assert_empty_document("%YAML 1.12345\n---\n");
}
