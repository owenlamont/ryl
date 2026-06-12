//! In-process validation of the JUnit XML and GitLab JSON report emitters
//! (`ryl::report`). GitLab output is checked against the vendored schema
//! (`tests/fixtures/gitlab-code-quality.schema.json`, the published GitLab contract);
//! JUnit output is re-parsed with quick-xml so structure and escaping are verified without
//! an external XSD validator. Driving the emitters directly (rather than the CLI) keeps the
//! assertions deterministic and exercises every report.rs branch.

use std::collections::HashMap;

use jsonschema::validator_for;
use quick_xml::Reader;
use quick_xml::escape::unescape;
use quick_xml::events::Event;
use ryl::report::{ReportEntry, render_gitlab, render_junit};
use ryl::{LintProblem, Severity};
use serde_json::Value;

fn problem(
    line: usize,
    column: usize,
    level: Severity,
    rule: Option<&'static str>,
    message: &str,
) -> LintProblem {
    LintProblem {
        line,
        column,
        level,
        message: message.to_string(),
        rule,
    }
}

fn gitlab_json(entries: &[ReportEntry]) -> Value {
    serde_json::from_slice(&render_gitlab(entries))
        .expect("gitlab output is valid JSON")
}

fn junit_xml(entries: &[ReportEntry]) -> String {
    String::from_utf8(render_junit(entries)).expect("junit output is UTF-8")
}

/// Element-name -> occurrence count over an XML document; panics on malformed XML, so a
/// successful parse is itself the well-formedness (and escaping) assertion.
fn element_counts(xml: &str) -> HashMap<String, usize> {
    let mut reader = Reader::from_str(xml);
    let mut counts = HashMap::new();
    loop {
        match reader
            .read_event()
            .expect("junit output is well-formed XML")
        {
            Event::Start(element) | Event::Empty(element) => {
                let name =
                    String::from_utf8_lossy(element.name().as_ref()).into_owned();
                *counts.entry(name).or_insert(0) += 1;
            }
            Event::Eof => break,
            _ => {}
        }
    }
    counts
}

#[test]
fn gitlab_output_matches_vendored_schema_and_maps_severity() {
    let schema: Value =
        serde_json::from_str(include_str!("fixtures/gitlab-code-quality.schema.json"))
            .expect("vendored schema is valid JSON");
    let validator = validator_for(&schema).expect("vendored schema compiles");

    let entries = vec![
        ReportEntry {
            path: "sub/dirty.yaml".to_string(),
            problems: vec![
                problem(3, 5, Severity::Error, Some("commas"), "too many spaces"),
                problem(4, 1, Severity::Warning, Some("truthy"), "truthy value"),
                problem(5, 2, Severity::Error, None, "syntax error: boom"),
            ],
            error: None,
        },
        ReportEntry {
            path: "broken.yaml".to_string(),
            problems: Vec::new(),
            error: Some("failed to read broken.yaml".to_string()),
        },
    ];

    let json = gitlab_json(&entries);
    assert!(
        validator.is_valid(&json),
        "gitlab output must satisfy the published schema: {json}"
    );

    let issues = json.as_array().expect("top level is an array");
    assert_eq!(
        issues.len(),
        4,
        "three diagnostics plus one processing error"
    );

    let severities: Vec<&str> = issues
        .iter()
        .map(|issue| issue["severity"].as_str().unwrap())
        .collect();
    assert_eq!(
        severities,
        ["major", "minor", "major", "blocker"],
        "error->major, warning->minor, processing error->blocker"
    );

    let syntax_issue = &issues[2];
    assert_eq!(
        syntax_issue["check_name"], "syntax",
        "a diagnostic without a rule id reports check_name `syntax`"
    );

    // GitLab requires relative paths without a `./` prefix.
    for issue in issues {
        let path = issue["location"]["path"].as_str().unwrap();
        assert!(
            !path.starts_with("./") && !path.starts_with('/'),
            "location.path must be relative: {path}"
        );
    }
}

#[test]
fn gitlab_skips_clean_files() {
    let entries = vec![ReportEntry {
        path: "clean.yaml".to_string(),
        problems: Vec::new(),
        error: None,
    }];
    let json = gitlab_json(&entries);
    assert_eq!(
        json.as_array().expect("array").len(),
        0,
        "a clean file contributes no GitLab issues"
    );
}

#[test]
fn gitlab_duplicate_diagnostics_get_distinct_fingerprints() {
    // Two byte-identical diagnostics on the same line collide on the base hash; the salt
    // must disambiguate them so GitLab's within-report uniqueness requirement holds.
    let entries = vec![ReportEntry {
        path: "dup.yaml".to_string(),
        problems: vec![
            problem(2, 3, Severity::Error, Some("commas"), "too many spaces"),
            problem(2, 3, Severity::Error, Some("commas"), "too many spaces"),
        ],
        error: None,
    }];
    let json = gitlab_json(&entries);
    let issues = json.as_array().unwrap();
    assert_eq!(issues.len(), 2);
    assert_ne!(
        issues[0]["fingerprint"], issues[1]["fingerprint"],
        "duplicate diagnostics must still produce unique fingerprints"
    );
}

#[test]
fn gitlab_fingerprints_are_stable_across_runs() {
    let build = || {
        vec![ReportEntry {
            path: "a.yaml".to_string(),
            problems: vec![problem(
                1,
                1,
                Severity::Error,
                Some("commas"),
                "too many spaces",
            )],
            error: None,
        }]
    };
    let first = gitlab_json(&build());
    let second = gitlab_json(&build());
    assert_eq!(
        first[0]["fingerprint"], second[0]["fingerprint"],
        "a stable hash keeps the fingerprint constant across runs"
    );
}

#[test]
fn gitlab_fingerprint_ignores_line_shifts() {
    // An edit elsewhere that shifts a diagnostic's line must not change its fingerprint,
    // or GitLab would treat the same issue as newly introduced and lose cross-run tracking.
    let at = |line| {
        vec![ReportEntry {
            path: "a.yaml".to_string(),
            problems: vec![problem(
                line,
                1,
                Severity::Error,
                Some("commas"),
                "too many spaces",
            )],
            error: None,
        }]
    };
    let early = gitlab_json(&at(3));
    let later = gitlab_json(&at(40));
    assert_eq!(
        early[0]["fingerprint"], later[0]["fingerprint"],
        "fingerprint must be independent of the diagnostic's line"
    );
    assert_ne!(
        early[0]["location"]["lines"]["begin"], later[0]["location"]["lines"]["begin"],
        "but the reported line still reflects the actual position"
    );
}

#[test]
fn junit_output_is_well_formed_with_matching_counts() {
    let entries = vec![
        ReportEntry {
            path: "sub/dirty.yaml".to_string(),
            problems: vec![
                problem(3, 5, Severity::Error, Some("commas"), "too many spaces"),
                problem(4, 1, Severity::Warning, None, "syntax error"),
            ],
            error: None,
        },
        ReportEntry {
            path: "clean.yaml".to_string(),
            problems: Vec::new(),
            error: None,
        },
        ReportEntry {
            path: "broken.yaml".to_string(),
            problems: Vec::new(),
            error: Some("failed to read broken.yaml".to_string()),
        },
    ];

    let xml = junit_xml(&entries);
    let counts = element_counts(&xml);
    assert_eq!(counts.get("testsuites"), Some(&1));
    assert_eq!(counts.get("testsuite"), Some(&3), "one suite per file");
    assert_eq!(
        counts.get("testcase"),
        Some(&4),
        "two failures, one clean, one error"
    );
    assert_eq!(counts.get("failure"), Some(&2));
    assert_eq!(counts.get("error"), Some(&1));

    assert!(
        xml.contains(
            "<testsuites name=\"ryl\" tests=\"4\" failures=\"2\" errors=\"1\">"
        ),
        "root totals should aggregate every suite: {xml}"
    );
}

#[test]
fn junit_disambiguates_testcases_at_an_identical_position() {
    // Two diagnostics from the same rule at the same line:col must get distinct testcase
    // names, or a JUnit consumer that dedups by name would drop one.
    let entries = vec![ReportEntry {
        path: "dup.yaml".to_string(),
        problems: vec![
            problem(2, 3, Severity::Error, Some("commas"), "first"),
            problem(2, 3, Severity::Error, Some("commas"), "second"),
        ],
        error: None,
    }];
    let xml = junit_xml(&entries);

    let mut reader = Reader::from_str(&xml);
    let mut names = Vec::new();
    loop {
        match reader.read_event().expect("well-formed XML") {
            Event::Start(element) | Event::Empty(element)
                if element.name().as_ref() == b"testcase" =>
            {
                for attr in element.attributes() {
                    let attr = attr.expect("valid attribute");
                    if attr.key.as_ref() == b"name" {
                        names.push(
                            String::from_utf8_lossy(attr.value.as_ref()).into_owned(),
                        );
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    assert_eq!(names.len(), 2, "two testcases expected: {xml}");
    assert_ne!(
        names[0], names[1],
        "duplicate-position testcases need unique names"
    );
}

#[test]
fn junit_escapes_special_characters_in_messages() {
    // A crafted message with XML metacharacters must round-trip through the writer and a
    // re-parse, proving quick-xml escaped it rather than producing malformed XML.
    let raw = "a < b & \"c\" > d";
    let entries = vec![ReportEntry {
        path: "x.yaml".to_string(),
        problems: vec![problem(1, 1, Severity::Error, Some("commas"), raw)],
        error: None,
    }];
    let xml = junit_xml(&entries);

    let mut reader = Reader::from_str(&xml);
    let message = loop {
        match reader.read_event().expect("well-formed XML") {
            Event::Start(element) if element.name().as_ref() == b"failure" => {
                let attr = element
                    .attributes()
                    .find_map(|attr| {
                        let attr = attr.expect("valid attribute");
                        (attr.key.as_ref() == b"message").then_some(attr)
                    })
                    .expect("failure has a message attribute");
                let escaped =
                    std::str::from_utf8(attr.value.as_ref()).expect("UTF-8 value");
                break unescape(escaped).expect("attribute unescapes").into_owned();
            }
            Event::Eof => panic!("no failure element found"),
            _ => {}
        }
    };
    assert_eq!(message, raw, "the message must survive XML escaping intact");
}

#[test]
fn junit_message_cannot_inject_extra_elements() {
    // A message crafted to close the <failure> and open a new <testcase> must be escaped to
    // text: the document stays well-formed with exactly one testcase and one failure.
    let raw = "boom</failure><testcase name=\"x\"/><failure>";
    let entries = vec![ReportEntry {
        path: "p.yaml".to_string(),
        problems: vec![problem(1, 1, Severity::Error, Some("commas"), raw)],
        error: None,
    }];
    let counts = element_counts(&junit_xml(&entries));
    assert_eq!(
        counts.get("testcase"),
        Some(&1),
        "the payload must not inject a testcase"
    );
    assert_eq!(
        counts.get("failure"),
        Some(&1),
        "the payload must not inject a failure element"
    );
}

#[test]
fn gitlab_message_cannot_inject_json_structure() {
    // A message crafted to break out of its JSON string must be encoded as data, never
    // injecting a sibling field or extra issue.
    let raw = "evil\",\"check_name\":\"injected\",\"x\":\"\n}], [{";
    let entries = vec![ReportEntry {
        path: "p.yaml".to_string(),
        problems: vec![problem(1, 1, Severity::Error, Some("commas"), raw)],
        error: None,
    }];
    let json = gitlab_json(&entries);
    let issues = json.as_array().expect("array");
    assert_eq!(issues.len(), 1, "the payload must not create extra issues");
    assert_eq!(
        issues[0]["check_name"], "commas",
        "check_name comes from the rule, not an injected message field"
    );
    let description = issues[0]["description"].as_str().unwrap();
    assert!(
        !description.contains('\n'),
        "control characters are stripped from the description: {description:?}"
    );
}
