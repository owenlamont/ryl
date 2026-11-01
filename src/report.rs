//! Report formats, written one entry at a time: `JUnit` XML and `GitLab` code quality JSON.
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

/// One linted file's contribution to a report. `error` (a failed read/write) and
/// `problems` are mutually exclusive; a clean file has both empty, which `JUnit` renders
/// as a passing test and `GitLab` omits entirely.
#[derive(Debug)]
pub struct ReportEntry<'a> {
    /// Display path, already relativized to the project root and forward-slashed.
    pub path: String,
    pub problems: &'a [LintProblem],
    pub error: Option<&'a str>,
}

/// Render every entry as a `JUnit` `<testsuites>` document: one `<testsuite>` per file, one
/// `<testcase>` per diagnostic (a `<failure>`), a single passing case for a clean file,
/// and an `<error>` case for a file that could not be processed.
#[must_use]
pub fn render_junit(entries: &[ReportEntry]) -> Vec<u8> {
    let mut report = JunitReport::default();
    for entry in entries {
        report.append(entry);
    }
    let (mut header, body) = report.finish();
    header.extend_from_slice(&body);
    header
}

/// A `JUnit` document rendered one entry at a time. Its root counts precede the body, so
/// the body is held until [`JunitReport::finish`] writes the root.
pub struct JunitReport {
    body: Writer<Vec<u8>>,
    root_len: usize,
    totals: (usize, usize, usize),
}

impl Default for JunitReport {
    fn default() -> Self {
        // Opening the body with a placeholder root gives its suites the root's indentation.
        let body = write_root(Vec::new(), (0, 0, 0));
        Self {
            root_len: body.get_ref().len(),
            body,
            totals: (0, 0, 0),
        }
    }
}

impl JunitReport {
    pub fn append(&mut self, entry: &ReportEntry) {
        let (tests, failures, errors) = suite_counts(entry);
        self.totals.0 += tests;
        self.totals.1 += failures;
        self.totals.2 += errors;
        write_suite(&mut self.body, entry);
    }

    /// The document as its root and the rest, written in that order.
    ///
    /// # Panics
    ///
    /// Does not panic in practice: every write targets an in-memory buffer.
    #[must_use]
    pub fn finish(mut self) -> (Vec<u8>, Vec<u8>) {
        self.body
            .write_event(Event::End(BytesEnd::new("testsuites")))
            .expect(INFALLIBLE);
        let mut body = self.body.into_inner();
        body.drain(..self.root_len);
        body.push(b'\n');
        (write_root(Vec::new(), self.totals).into_inner(), body)
    }
}

fn write_root(
    buffer: Vec<u8>,
    (tests, failures, errors): (usize, usize, usize),
) -> Writer<Vec<u8>> {
    let mut writer = Writer::new_with_indent(buffer, b' ', 2);
    writer
        .write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))
        .expect(INFALLIBLE);
    let mut root = BytesStart::new("testsuites");
    root.push_attribute(("name", "ryl"));
    root.push_attribute(("tests", tests.to_string().as_str()));
    root.push_attribute(("failures", failures.to_string().as_str()));
    root.push_attribute(("errors", errors.to_string().as_str()));
    writer.write_event(Event::Start(root)).expect(INFALLIBLE);
    writer
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

    if let Some(error) = entry.error {
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
        for problem in entry.problems {
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
    crate::cli_support::escape_chars(text, |c| {
        c.is_control() || c == '\u{fffe}' || c == '\u{ffff}'
    })
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
#[must_use]
pub fn render_gitlab(entries: &[ReportEntry]) -> Vec<u8> {
    let mut buffer = b"[".to_vec();
    let mut state = GitlabState::new(entries.iter().map(|entry| entry.path.as_str()));
    for entry in entries {
        state.append(&mut buffer, entry);
    }
    buffer.extend_from_slice(b"]\n");
    buffer
}

/// Fingerprint salts carried across one streamed `GitLab` array. A salt key starts with its
/// path, so two entries can share a key only when their paths agree up to the first NUL;
/// only such a shared prefix keeps its salts beyond its own entry.
pub struct GitlabState {
    shared: HashSet<String>,
    seen: HashSet<String>,
    next_salts: HashMap<String, u64>,
    has_issue: bool,
}

impl GitlabState {
    /// State for a report over entries with these paths, in any order.
    pub fn new<'a>(paths: impl IntoIterator<Item = &'a str>) -> Self {
        let mut once = HashSet::new();
        let shared = paths
            .into_iter()
            .map(salt_group)
            .filter(|group| !once.insert(*group))
            .map(str::to_owned)
            .collect();
        Self {
            shared,
            seen: HashSet::new(),
            next_salts: HashMap::new(),
            has_issue: false,
        }
    }

    /// Append one entry's issues, each preceded by a comma unless it opens the array.
    ///
    /// # Panics
    ///
    /// Does not panic in practice: serialization targets an in-memory buffer.
    pub fn append(&mut self, out: &mut Vec<u8>, entry: &ReportEntry) {
        let mut own = (HashSet::new(), HashMap::new());
        let (seen, next_salts) = if self.shared.contains(salt_group(&entry.path)) {
            (&mut self.seen, &mut self.next_salts)
        } else {
            (&mut own.0, &mut own.1)
        };
        let has_issue = &mut self.has_issue;
        let mut issue = |check_name: &str, line: usize, message: &str, severity| {
            if std::mem::replace(has_issue, true) {
                out.push(b',');
            }
            let issue = make_issue(
                &entry.path,
                check_name,
                line,
                message,
                severity,
                seen,
                next_salts,
            );
            serde_json::to_writer(&mut *out, &issue)
                .expect("serializing a report issue to a Vec cannot fail");
        };
        if let Some(error) = entry.error {
            issue("error", 1, error, "blocker");
        } else {
            for problem in entry.problems {
                issue(
                    problem.rule.unwrap_or("syntax"),
                    problem.line,
                    &problem.message,
                    gitlab_severity(problem.level),
                );
            }
        }
    }
}

fn salt_group(path: &str) -> &str {
    path.split('\0').next().unwrap_or_default()
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
    next_salts: &mut HashMap<String, u64>,
) -> GitlabIssue {
    // Embedded NULs can make different field tuples share the same hash input.
    let next_salt = next_salts
        .entry(format!("{path}\0{check_name}\0{message}"))
        .or_default();
    let (salt, hash) = (*next_salt..)
        .find_map(|salt| {
            let hash = fingerprint(path, check_name, message, salt);
            seen.insert(hash.clone()).then_some((salt, hash))
        })
        .expect("a report cannot exhaust the u64 salt space");
    *next_salt = salt + 1;
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
    hex_digest(&hasher.finalize())
}

/// Lower-case hex of a digest, for the identifiers callers hand to other tools (GitLab's
/// fingerprint, an LSP `resultId`).
pub(crate) fn hex_digest(digest: &[u8]) -> String {
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(out, "{byte:02x}").expect("writing to a String is infallible");
    }
    out
}
