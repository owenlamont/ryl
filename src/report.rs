//! Whole-document report formats: `JUnit` XML and `GitLab` code quality JSON.
//!
//! All user text is sanitized before serialization so it cannot break out of the
//! structure: `JUnit` via `xml_sanitize` (control chars plus the U+FFFE/U+FFFF
//! noncharacters XML forbids), `GitLab` via [`sanitize_control`], then quick-xml /
//! `serde_json` apply structural escaping.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::io::Write;

use quick_xml::Writer;
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::cli_support::sanitize_control;
use crate::lint::{LintProblem, Severity};

// Writes target an owned `Vec<u8>` whose `io::Write` never fails, so each write is an
// `expect` rather than a `?` that would leave a dead, uncovered error arm.
const INFALLIBLE: &str = "writing report output to an in-memory buffer cannot fail";

/// One linted file's contribution to a report. `error` (a failed read/parse) and
/// `problems` are mutually exclusive; a clean file has both empty, which `JUnit` renders
/// as a passing test and `GitLab` omits entirely.
#[derive(Debug)]
pub struct ReportEntry {
    /// Display path, already relativized to the project root and forward-slashed.
    pub path: String,
    pub problems: Vec<LintProblem>,
    pub error: Option<String>,
}

/// Render every entry as a `JUnit` `<testsuites>` document: one `<testsuite>` per file, one
/// `<testcase>` per diagnostic (a `<failure>`), a single passing case for a clean file,
/// and an `<error>` case for a file that could not be processed.
///
/// # Panics
///
/// Does not panic in practice: every write targets an in-memory buffer.
#[must_use]
pub fn render_junit(entries: &[ReportEntry]) -> Vec<u8> {
    let (mut tests, mut failures, mut errors) = (0usize, 0usize, 0usize);
    for entry in entries {
        let (t, f, e) = suite_counts(entry);
        tests += t;
        failures += f;
        errors += e;
    }

    let mut buffer = Vec::new();
    let mut writer = Writer::new_with_indent(&mut buffer, b' ', 2);
    writer
        .write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))
        .expect(INFALLIBLE);
    let mut root = BytesStart::new("testsuites");
    root.push_attribute(("name", "ryl"));
    root.push_attribute(("tests", tests.to_string().as_str()));
    root.push_attribute(("failures", failures.to_string().as_str()));
    root.push_attribute(("errors", errors.to_string().as_str()));
    writer.write_event(Event::Start(root)).expect(INFALLIBLE);
    for entry in entries {
        write_suite(&mut writer, entry);
    }
    writer
        .write_event(Event::End(BytesEnd::new("testsuites")))
        .expect(INFALLIBLE);
    writer.into_inner().push(b'\n');
    buffer
}

/// `JUnit` `(tests, failures, errors)` for one file: a processing error is one errored case,
/// a clean file one passing case, otherwise one failing case per diagnostic.
fn suite_counts(entry: &ReportEntry) -> (usize, usize, usize) {
    if entry.error.is_some() {
        (1, 0, 1)
    } else if entry.problems.is_empty() {
        (1, 0, 0)
    } else {
        (entry.problems.len(), entry.problems.len(), 0)
    }
}

fn write_suite<W: Write>(writer: &mut Writer<W>, entry: &ReportEntry) {
    let (tests, failures, errors) = suite_counts(entry);
    let path = xml_sanitize(&entry.path);
    let mut suite = BytesStart::new("testsuite");
    suite.push_attribute(("name", path.as_ref()));
    suite.push_attribute(("tests", tests.to_string().as_str()));
    suite.push_attribute(("failures", failures.to_string().as_str()));
    suite.push_attribute(("errors", errors.to_string().as_str()));
    writer.write_event(Event::Start(suite)).expect(INFALLIBLE);

    if let Some(error) = &entry.error {
        let message = xml_sanitize(error);
        write_case_with_child(
            writer,
            "error",
            path.as_ref(),
            "error",
            message.as_ref(),
            "error",
            message.as_ref(),
        );
    } else if entry.problems.is_empty() {
        let mut case = BytesStart::new("testcase");
        case.push_attribute(("name", path.as_ref()));
        case.push_attribute(("classname", path.as_ref()));
        writer.write_event(Event::Empty(case)).expect(INFALLIBLE);
    } else {
        // A `#n` suffix disambiguates a repeated `rule:line:col`: some JUnit consumers
        // merge testcases that share a name.
        let mut seen: HashMap<String, u32> = HashMap::new();
        for problem in &entry.problems {
            let rule = problem.rule.unwrap_or("syntax");
            let base = format!("{rule}:{}:{}", problem.line, problem.column);
            let occurrence = seen.entry(base.clone()).or_insert(0);
            let name = if *occurrence == 0 {
                base.clone()
            } else {
                format!("{base}#{occurrence}")
            };
            *occurrence += 1;
            let message = xml_sanitize(&problem.message);
            let body = format!("{}:{} {message}", problem.line, problem.column);
            write_case_with_child(
                writer,
                &name,
                path.as_ref(),
                "failure",
                message.as_ref(),
                rule,
                &body,
            );
        }
    }

    writer
        .write_event(Event::End(BytesEnd::new("testsuite")))
        .expect(INFALLIBLE);
}

/// Like [`sanitize_control`], but also escapes the U+FFFE/U+FFFF noncharacters that XML
/// 1.0 forbids even as numeric references, so a crafted scalar cannot make the `JUnit`
/// document unparsable. Control chars are the only other XML-invalid scalars a Rust
/// `char` can hold (surrogates are unrepresentable), so this covers the whole gap.
fn xml_sanitize(text: &str) -> Cow<'_, str> {
    fn forbidden(c: char) -> bool {
        c.is_control() || c == '\u{fffe}' || c == '\u{ffff}'
    }
    if !text.contains(forbidden) {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if forbidden(c) {
            write!(out, "\\u{{{:x}}}", c as u32)
                .expect("writing to a String is infallible");
        } else {
            out.push(c);
        }
    }
    Cow::Owned(out)
}

fn write_case_with_child<W: Write>(
    writer: &mut Writer<W>,
    case_name: &str,
    classname: &str,
    child_tag: &str,
    message: &str,
    type_attr: &str,
    body: &str,
) {
    let mut case = BytesStart::new("testcase");
    case.push_attribute(("name", case_name));
    case.push_attribute(("classname", classname));
    writer.write_event(Event::Start(case)).expect(INFALLIBLE);

    let mut child = BytesStart::new(child_tag);
    child.push_attribute(("message", message));
    child.push_attribute(("type", type_attr));
    writer.write_event(Event::Start(child)).expect(INFALLIBLE);
    writer
        .write_event(Event::Text(BytesText::new(body)))
        .expect(INFALLIBLE);
    writer
        .write_event(Event::End(BytesEnd::new(child_tag)))
        .expect(INFALLIBLE);

    writer
        .write_event(Event::End(BytesEnd::new("testcase")))
        .expect(INFALLIBLE);
}

#[derive(Serialize)]
struct GitlabIssue {
    description: String,
    check_name: String,
    severity: &'static str,
    fingerprint: String,
    location: GitlabLocation,
}

#[derive(Serialize)]
struct GitlabLocation {
    path: String,
    lines: GitlabLines,
}

#[derive(Serialize)]
struct GitlabLines {
    begin: usize,
}

/// Render every diagnostic as a `GitLab` code quality JSON array. Clean files contribute
/// nothing; a processing error becomes a single `blocker` issue on line 1.
///
/// # Panics
///
/// Does not panic in practice: serialization targets an in-memory buffer.
#[must_use]
pub fn render_gitlab(entries: &[ReportEntry]) -> Vec<u8> {
    let mut issues: Vec<GitlabIssue> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for entry in entries {
        if let Some(error) = &entry.error {
            issues.push(make_issue(
                &entry.path,
                "error",
                1,
                error,
                "blocker",
                &mut seen,
            ));
        } else {
            for problem in &entry.problems {
                let check_name = problem.rule.unwrap_or("syntax");
                let severity = gitlab_severity(problem.level);
                issues.push(make_issue(
                    &entry.path,
                    check_name,
                    problem.line,
                    &problem.message,
                    severity,
                    &mut seen,
                ));
            }
        }
    }

    let mut buffer = serde_json::to_vec(&issues)
        .expect("serializing report issues to a Vec cannot fail");
    buffer.push(b'\n');
    buffer
}

fn gitlab_severity(level: Severity) -> &'static str {
    match level {
        Severity::Error => "major",
        Severity::Warning => "minor",
    }
}

fn make_issue(
    path: &str,
    check_name: &str,
    line: usize,
    message: &str,
    severity: &'static str,
    seen: &mut HashSet<String>,
) -> GitlabIssue {
    // GitLab requires fingerprints unique within a report, so two findings sharing the same
    // (path, rule, message) are disambiguated by an encounter-order salt: identity is then
    // stable under line shifts but a duplicate reordered ahead of another reassigns its salt
    // (no scheme is stable under both line shifts and duplicate reordering from these inputs).
    let mut salt = 0u64;
    let mut hash = fingerprint(path, check_name, message, salt);
    while !seen.insert(hash.clone()) {
        salt += 1;
        hash = fingerprint(path, check_name, message, salt);
    }
    GitlabIssue {
        description: sanitize_control(message).into_owned(),
        check_name: check_name.to_string(),
        severity,
        fingerprint: hash,
        location: GitlabLocation {
            path: path.to_string(),
            lines: GitlabLines { begin: line },
        },
    }
}

/// SHA-256 hex of the diagnostic's identity `(path, rule, message)`. Excludes line/column
/// so an edit that shifts the diagnostic does not reset GitLab's cross-version tracking. A
/// stable digest (not `DefaultHasher`, whose output varies across Rust versions) keeps it
/// constant across toolchains; `salt` disambiguates a shared identity.
fn fingerprint(path: &str, check_name: &str, message: &str, salt: u64) -> String {
    let mut hasher = Sha256::new();
    hasher.update(salt.to_le_bytes());
    hasher.update(path.as_bytes());
    hasher.update([0u8]);
    hasher.update(check_name.as_bytes());
    hasher.update([0u8]);
    hasher.update(message.as_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(out, "{byte:02x}").expect("writing to a String is infallible");
    }
    out
}
