//! Property tests for the report emitters (`ryl::report::render_junit`/`render_gitlab`).
//!
//! These fuzz the *output* path the deterministic `report_formats.rs` tests only sample:
//! arbitrary [`ReportEntry`] lists with hostile paths and messages (control characters, XML
//! and JSON metacharacters, line separators, multibyte) are rendered and checked against
//! oracle-free invariants. The headline ones are escaping/injection robustness: JUnit output
//! must *always* re-parse as well-formed XML, and GitLab output must *always* parse as JSON
//! and validate against the vendored schema (the same contract the deterministic test uses).

use std::collections::HashSet;
use std::sync::LazyLock;

use jsonschema::{Validator, validator_for};
use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use quick_xml::Reader;
use quick_xml::events::Event;
use ryl::cli_support::github_escape;
use ryl::report::{ReportEntry, render_gitlab, render_junit};
use ryl::{LintProblem, Severity};
use serde_json::Value;

/// The published GitLab contract, compiled once and reused across every generated case.
static GITLAB_SCHEMA: LazyLock<Validator> = LazyLock::new(|| {
    let schema: Value =
        serde_json::from_str(include_str!("fixtures/gitlab-code-quality.schema.json"))
            .expect("vendored schema is valid JSON");
    validator_for(&schema).expect("vendored schema compiles")
});

/// Characters spanning ryl's real escaping surface: ordinary text, XML metacharacters,
/// JSON-ish punctuation, C0/DEL control characters, the whitespace controls XML keeps,
/// NEL/LS/PS line separators, the U+FFFE/U+FFFF noncharacters XML 1.0 forbids, and
/// multibyte scalars.
fn arb_char() -> impl Strategy<Value = char> {
    prop_oneof![
        Just('a'),
        Just('Z'),
        Just('0'),
        Just(' '),
        Just('/'),
        Just('.'),
        Just('-'),
        Just('<'),
        Just('>'),
        Just('&'),
        Just('"'),
        Just('\''),
        Just('{'),
        Just('}'),
        Just('['),
        Just(']'),
        Just(':'),
        Just(','),
        Just('\u{0}'),
        Just('\u{1}'),
        Just('\u{1b}'),
        Just('\u{7f}'),
        Just('\t'),
        Just('\n'),
        Just('\r'),
        Just('\u{85}'),
        Just('\u{2028}'),
        Just('\u{2029}'),
        Just('\u{fffe}'),
        Just('\u{ffff}'),
        Just('é'),
        Just('日'),
        Just('🦀'),
    ]
}

fn arb_hostile_string() -> impl Strategy<Value = String> {
    proptest::collection::vec(arb_char(), 0..16)
        .prop_map(|chars| chars.into_iter().collect())
}

fn arb_rule() -> impl Strategy<Value = Option<&'static str>> {
    prop_oneof![
        Just(None),
        Just(Some("commas")),
        Just(Some("colons")),
        Just(Some("truthy")),
        Just(Some("key-duplicates")),
    ]
}

fn arb_problem() -> impl Strategy<Value = LintProblem> {
    (
        1usize..1000,
        1usize..200,
        prop_oneof![Just(Severity::Error), Just(Severity::Warning)],
        arb_rule(),
        arb_hostile_string(),
    )
        .prop_map(|(line, column, level, rule, message)| LintProblem {
            line,
            column,
            level,
            message,
            rule,
        })
}

/// `error` and `problems` are mutually exclusive in real entries (a file failed to process,
/// or it has diagnostics, or it is clean); the generator mirrors that.
fn arb_entry() -> impl Strategy<Value = ReportEntry> {
    (
        arb_hostile_string(),
        prop_oneof![
            arb_hostile_string().prop_map(|error| (Vec::new(), Some(error))),
            proptest::collection::vec(arb_problem(), 0..6)
                .prop_map(|problems| (problems, None)),
        ],
    )
        .prop_map(|(path, (problems, error))| ReportEntry {
            path,
            problems,
            error,
        })
}

fn arb_entries() -> impl Strategy<Value = Vec<ReportEntry>> {
    proptest::collection::vec(arb_entry(), 0..6)
}

/// The value of `element`'s `key` attribute, decoded lossily, if present.
fn read_attr(element: &quick_xml::events::BytesStart, key: &[u8]) -> Option<String> {
    element.attributes().flatten().find_map(|attr| {
        (attr.key.as_ref() == key)
            .then(|| String::from_utf8_lossy(attr.value.as_ref()).into_owned())
    })
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 512,
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
            "tests/proptest-regressions/property_report.txt",
        ))),
        ..ProptestConfig::default()
    })]

    /// GitLab output is always valid JSON, validates against the vendored schema, uses only
    /// the allowed severities, has report-unique fingerprints, and one issue per diagnostic
    /// (plus one per processing error; clean files contribute none).
    #[test]
    fn gitlab_output_is_always_valid_and_schema_conformant(entries in arb_entries()) {
        let bytes = render_gitlab(&entries);
        let json: Value = serde_json::from_slice(&bytes)
            .map_err(|err| TestCaseError::fail(format!("invalid JSON: {err}")))?;
        prop_assert!(
            GITLAB_SCHEMA.is_valid(&json),
            "output does not satisfy the GitLab schema: {json}"
        );

        let issues = json.as_array().expect("schema guarantees a top-level array");
        let expected: usize = entries
            .iter()
            .map(|entry| if entry.error.is_some() { 1 } else { entry.problems.len() })
            .sum();
        prop_assert_eq!(issues.len(), expected, "one issue per diagnostic/error");

        let mut fingerprints = HashSet::new();
        for issue in issues {
            let severity = issue["severity"].as_str().unwrap();
            prop_assert!(
                matches!(severity, "major" | "minor" | "blocker"),
                "unexpected severity {severity}"
            );
            let fingerprint = issue["fingerprint"].as_str().unwrap();
            prop_assert!(
                fingerprints.insert(fingerprint.to_owned()),
                "duplicate fingerprint {fingerprint} within a report"
            );
        }
    }

    /// JUnit output is always well-formed XML (the re-parse fails otherwise), the root
    /// `<testsuites>` counts equal the actual element tallies, and no two testcases in a
    /// suite share a `name`.
    #[test]
    fn junit_output_is_always_well_formed_with_consistent_counts(
        entries in arb_entries(),
    ) {
        let bytes = render_junit(&entries);
        let xml = String::from_utf8(bytes)
            .map_err(|err| TestCaseError::fail(format!("non-UTF-8 output: {err}")))?;

        let mut reader = Reader::from_str(&xml);
        let (mut root_tests, mut root_failures, mut root_errors) = (None, None, None);
        let (mut testcases, mut failures, mut errors) = (0usize, 0usize, 0usize);
        let mut suite_names: HashSet<String> = HashSet::new();
        loop {
            match reader.read_event() {
                Err(err) => {
                    return Err(TestCaseError::fail(format!("malformed XML: {err}")));
                }
                Ok(Event::Eof) => break,
                Ok(Event::Start(element) | Event::Empty(element)) => {
                    match element.name().as_ref() {
                        b"testsuites" => {
                            root_tests = read_attr(&element, b"tests")
                                .and_then(|value| value.parse::<usize>().ok());
                            root_failures = read_attr(&element, b"failures")
                                .and_then(|value| value.parse::<usize>().ok());
                            root_errors = read_attr(&element, b"errors")
                                .and_then(|value| value.parse::<usize>().ok());
                        }
                        b"testsuite" => suite_names.clear(),
                        b"testcase" => {
                            testcases += 1;
                            let name = read_attr(&element, b"name").unwrap_or_default();
                            prop_assert!(
                                suite_names.insert(name.clone()),
                                "duplicate testcase name {name} within a suite"
                            );
                        }
                        b"failure" => failures += 1,
                        b"error" => errors += 1,
                        _ => {}
                    }
                }
                Ok(_) => {}
            }
        }

        prop_assert_eq!(root_tests, Some(testcases), "root tests count mismatch");
        prop_assert_eq!(root_failures, Some(failures), "root failures count mismatch");
        prop_assert_eq!(root_errors, Some(errors), "root errors count mismatch");
    }

    /// The GitHub workflow-command format is a line-oriented protocol, so its only injection
    /// defense is `github_escape`: no input may leave a control character (a newline would
    /// start a new `::command::`) in the encoded text, in either data or property mode.
    #[test]
    fn github_escape_never_emits_a_control_character(
        text in arb_hostile_string(),
        property in any::<bool>(),
    ) {
        let escaped = github_escape(&text, property);
        prop_assert!(
            !escaped.chars().any(|c| c.is_control()),
            "github_escape leaked a control character from {text:?}: {escaped:?}"
        );
    }
}
