use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use super::representation::representation;

struct ReferenceParser {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    events: HashMap<String, Option<Vec<String>>>,
}

impl ReferenceParser {
    fn new() -> Self {
        let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
        let parser = std::env::var_os("RYL_REFERENCE_PARSER").map_or_else(
            || tests.join("yaml-reference-parser/parser-1.2/javascript"),
            PathBuf::from,
        );
        assert!(
            parser.join("lib/parser.js").is_file(),
            "{} is not the YAML reference parser: run \
             `git submodule update --init tests/yaml-reference-parser`",
            parser.display()
        );
        let mut child = Command::new("node")
            .args(["-e", include_str!("reference.cjs")])
            .arg(parser)
            .env("NODE_PATH", tests.join("property_format/node_shims"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("start the YAML reference parser with Node");
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            input,
            output,
            events: HashMap::new(),
        }
    }

    fn events(&mut self, content: &str) -> Option<Vec<String>> {
        let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
        if let Some(events) = self.events.get(&normalized) {
            return events.clone();
        }
        writeln!(
            self.input,
            "{}",
            serde_json::to_string(&normalized).unwrap()
        )
        .unwrap();
        let mut line = String::new();
        assert_ne!(
            self.output.read_line(&mut line).unwrap(),
            0,
            "reference parser exited"
        );
        let events = serde_json::from_str::<Option<String>>(&line)
            .expect("reference parser events")
            .map(|events| without_presentation(&events));
        self.events.insert(normalized, events.clone());
        events
    }
}

impl Drop for ReferenceParser {
    fn drop(&mut self) {
        self.child.kill().expect("stop reference parser");
        self.child.wait().expect("reap reference parser");
    }
}

/// `events` without the document markers, flow indicators and scalar styles formatting
/// may change (ryl's own representation checks how each scalar resolves), and with each
/// anchor named by its definition order, leaving the names to `check_annotations`.
fn without_presentation(events: &str) -> Vec<String> {
    let mut anchors = HashMap::new();
    let mut defined = 0..;
    events
        .lines()
        .map(|event| {
            let (head, value) = match event.strip_prefix("=VAL ") {
                Some(rest) => {
                    let properties: usize = rest
                        .split(' ')
                        .take_while(|word| word.starts_with(['&', '<']))
                        .map(|word| word.len() + 1)
                        .sum();
                    (&event[..4 + properties], Some(&rest[properties + 1..]))
                }
                None => (event, None),
            };
            let mut words: Vec<String> = head
                .split_whitespace()
                .filter(|word| !matches!(*word, "---" | "..." | "{}" | "[]"))
                .map(|word| {
                    if let Some(name) = word.strip_prefix('&') {
                        let number = defined.next().unwrap();
                        anchors.insert(name, number);
                        format!("&{number}")
                    } else if let Some(name) = word.strip_prefix('*') {
                        format!(
                            "*{}",
                            anchors
                                .get(name)
                                .map_or_else(|| name.to_owned(), usize::to_string)
                        )
                    } else {
                        word.to_owned()
                    }
                })
                .collect();
            words.extend(value.map(|value| format!(":{value}")));
            words.join(" ")
        })
        .collect()
}

/// Compares ryl's representation of `input` and `output`, then the YAML 1.2 reference
/// parser's events.
pub fn check_values(input: &str, output: &str) -> Result<(), String> {
    let before = representation(input);
    let after = representation(output);
    if before.is_some() != after.is_some() {
        return Err(format!("parse-preservation: output {output:?}"));
    }
    if before != after {
        return Err(format!(
            "value-preservation: output {output:?}; before {before:?}; after {after:?}"
        ));
    }
    if input == output {
        return Ok(());
    }
    check_events(input, output)
}

/// Compares the YAML 1.2 reference parser's events for `input` and `output`; an input it
/// rejects is a lenient repair, so only ryl's own checks apply.
pub fn check_events(input: &str, output: &str) -> Result<(), String> {
    thread_local! {
        static PARSER: RefCell<ReferenceParser> = RefCell::new(ReferenceParser::new());
    }
    PARSER.with(|parser| {
        let mut parser = parser.borrow_mut();
        let Some(before) = parser.events(input) else {
            return Ok(());
        };
        match parser.events(output) {
            None => Err(format!(
                "parse-preservation: the reference parser rejects output {output:?}"
            )),
            Some(after) if after != before => Err(format!(
                "value-preservation: reference events differ for output {output:?}; \
                 before {before:?}; after {after:?}"
            )),
            Some(_) => Ok(()),
        }
    })
}
