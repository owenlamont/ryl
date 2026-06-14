#![cfg(feature = "lsp")]
//! Tier-2 property test: a stateful model of the `ryl server` protocol. It replays
//! random sequences of didOpen / didChange / didClose / codeAction / formatting
//! against a live server (an in-process `Connection::memory()` pair) and checks the
//! invariants a real editor relies on:
//!
//! - **liveness** — every request gets a response and the session always shuts down
//!   cleanly (a hang or panic fails the case rather than wedging the editor);
//! - **version echoing** — `publishDiagnostics` carries the document's version;
//! - **clear-on-close** — closing a document publishes empty diagnostics;
//! - **state faithfulness** — published diagnostics always equal a fresh lint of the
//!   document's *current* text (no stale state across edits).
//!
//! It uses a small, focused driver rather than the example-test client in
//! `lsp_server.rs` because the replay needs only raw send/await primitives, and
//! keeping it in its own file lets the thorough run use a server-spawn-appropriate
//! case count.

#[path = "property_check/strategy.rs"]
mod strategy;

use std::path::Path;
use std::str::FromStr;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use lsp_server::{Connection, Message, Notification, Request, RequestId};
use lsp_types::{
    CodeActionContext, CodeActionParams, DidChangeTextDocumentParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DocumentFormattingParams,
    FormattingOptions, InitializeParams, PartialResultParams, Position,
    PublishDiagnosticsParams, Range, TextDocumentContentChangeEvent,
    TextDocumentIdentifier, TextDocumentItem, Uri, VersionedTextDocumentIdentifier,
    WorkDoneProgressParams,
};
use proptest::prelude::*;
use proptest::test_runner::FileFailurePersistence;
use serde_json::{Value, json, to_value};
use tempfile::tempdir;

use ryl::config::{Overrides, SourceKind, discover_config};
use ryl::lsp::analysis::diagnostics;
use ryl::lsp::encoding::PositionEncoding;

use strategy::arb_document;

const CONFIG: &str = "[rules]\ntrailing-spaces = \"enable\"\ndocument-start = \"enable\"\nkey-duplicates = \"enable\"\n";
const DOC_POOL: u8 = 3;

/// Focused in-process LSP client for the replay: spawn the server, exchange raw
/// messages, await responses/notifications with a timeout (a missing reply fails
/// the case instead of hanging).
struct Driver {
    conn: Option<Connection>,
    thread: Option<JoinHandle<()>>,
    next_id: i32,
}

impl Driver {
    fn start() -> Self {
        let (server, client) = Connection::memory();
        let thread = thread::spawn(move || {
            let _ = ryl::lsp::serve(&server);
        });
        let mut driver = Self {
            conn: Some(client),
            thread: Some(thread),
            next_id: 0,
        };
        // Default client capabilities -> the server negotiates UTF-16.
        let init = to_value(InitializeParams::default()).expect("serialize init");
        let id = driver.request("initialize", init);
        let _ = driver.await_response(&id);
        driver.notify("initialized", Value::Null);
        driver
    }

    fn conn(&self) -> &Connection {
        self.conn.as_ref().expect("connection live")
    }

    fn notify(&self, method: &str, params: Value) {
        self.conn()
            .sender
            .send(Message::Notification(Notification::new(
                method.to_string(),
                params,
            )))
            .expect("send notification");
    }

    fn request(&mut self, method: &str, params: Value) -> RequestId {
        self.next_id += 1;
        let id = RequestId::from(self.next_id);
        self.conn()
            .sender
            .send(Message::Request(Request::new(
                id.clone(),
                method.to_string(),
                params,
            )))
            .expect("send request");
        id
    }

    fn recv(&self) -> Message {
        self.conn()
            .receiver
            .recv_timeout(Duration::from_secs(10))
            .expect("server replied within the timeout (no hang)")
    }

    fn await_response(&self, id: &RequestId) -> Message {
        loop {
            let message = self.recv();
            if matches!(&message, Message::Response(response) if &response.id == id) {
                return message;
            }
        }
    }

    fn await_publish(&self) -> PublishDiagnosticsParams {
        loop {
            if let Message::Notification(note) = self.recv()
                && note.method == "textDocument/publishDiagnostics"
            {
                return serde_json::from_value(note.params)
                    .expect("diagnostics params");
            }
        }
    }

    fn shutdown(&mut self) {
        let id = self.request("shutdown", Value::Null);
        let _ = self.await_response(&id);
        self.notify("exit", Value::Null);
    }
}

impl Drop for Driver {
    fn drop(&mut self) {
        drop(self.conn.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn doc_path(dir: &Path, index: u8) -> std::path::PathBuf {
    dir.join(format!("doc{index}.yaml"))
}

/// Byte offset of the start of `line` (0-based) in `text`, CR-aware. Independent of the
/// server's converter so an incremental edit at column 0 cross-checks `offset_at`'s
/// line counting; clamps a line past the end to the text end.
fn line_start_byte(text: &str, line: u32) -> usize {
    let bytes = text.as_bytes();
    let mut byte = 0;
    let mut current = 0;
    while current < line && byte < bytes.len() {
        match bytes[byte] {
            b'\r' => {
                byte += 1;
                if bytes.get(byte) == Some(&b'\n') {
                    byte += 1;
                }
                current += 1;
            }
            b'\n' => {
                byte += 1;
                current += 1;
            }
            _ => byte += 1,
        }
    }
    byte
}

fn doc_uri(dir: &Path, index: u8) -> Uri {
    // Valid file URI cross-platform (forward slashes; leading slash before a
    // Windows drive) so the suite runs on every OS.
    let mut path = doc_path(dir, index)
        .display()
        .to_string()
        .replace('\\', "/");
    if !path.starts_with('/') {
        path.insert(0, '/');
    }
    Uri::from_str(&format!("file://{path}")).expect("valid URI")
}

#[derive(Debug, Clone)]
enum Op {
    Open(u8, String),
    Change(u8, String),
    Close(u8),
    CodeAction(u8),
    Formatting(u8),
    Hover(u8),
    Diagnostic(u8),
}

fn arb_op() -> impl Strategy<Value = Op> {
    let index = 0u8..DOC_POOL;
    prop_oneof![
        // didOpen and didChange share server handling; one bool picks which.
        (
            index.clone(),
            any::<bool>(),
            arb_document().prop_map(|d| d.render())
        )
            .prop_map(|(i, is_open, text)| if is_open {
                Op::Open(i, text)
            } else {
                Op::Change(i, text)
            }),
        index.clone().prop_map(Op::Close),
        index.clone().prop_map(Op::CodeAction),
        index.clone().prop_map(Op::Hover),
        index.clone().prop_map(Op::Diagnostic),
        index.prop_map(Op::Formatting),
    ]
}

/// didOpen/didChange handling is identical server-side (store + publish); verify
/// the version echo and that the published diagnostics match a fresh lint.
fn check_update(
    dir: &Path,
    index: u8,
    version: i32,
    text: &str,
    diagnostics_params: &PublishDiagnosticsParams,
) -> Result<(), TestCaseError> {
    prop_assert_eq!(
        diagnostics_params.version,
        Some(version),
        "publishDiagnostics echoes the document version"
    );
    let path = doc_path(dir, index);
    let context = discover_config(std::slice::from_ref(&path), &Overrides::default())
        .expect("config discovers");
    let expected = diagnostics(
        text,
        &path,
        &context.config,
        &context.base_dir,
        SourceKind::Yaml,
        PositionEncoding::Utf16,
    );
    prop_assert_eq!(
        &diagnostics_params.diagnostics,
        &expected,
        "published diagnostics match a fresh lint of the current text"
    );
    Ok(())
}

fn code_action_params(uri: Uri) -> Value {
    to_value(CodeActionParams {
        text_document: TextDocumentIdentifier { uri },
        range: Range::default(),
        context: CodeActionContext {
            diagnostics: Vec::new(),
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: WorkDoneProgressParams::default(),
        partial_result_params: PartialResultParams::default(),
    })
    .expect("serialize code action params")
}

fn formatting_params(uri: Uri) -> Value {
    to_value(DocumentFormattingParams {
        text_document: TextDocumentIdentifier { uri },
        options: FormattingOptions::default(),
        work_done_progress_params: WorkDoneProgressParams::default(),
    })
    .expect("serialize formatting params")
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 192,
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
            "tests/proptest-regressions/property_lsp_protocol.txt",
        ))),
        ..ProptestConfig::default()
    })]

    #[test]
    fn server_upholds_protocol_invariants(ops in prop::collection::vec(arb_op(), 0..12)) {
        let dir = tempdir().expect("tempdir");
        std::fs::write(dir.path().join(".ryl.toml"), CONFIG).expect("write config");
        let mut driver = Driver::start();
        let mut version = 0;

        for op in ops {
            match op {
                Op::Open(index, text) => {
                    version += 1;
                    let document = TextDocumentItem {
                        uri: doc_uri(dir.path(), index),
                        language_id: "yaml".to_string(),
                        version,
                        text: text.clone(),
                    };
                    driver.notify(
                        "textDocument/didOpen",
                        to_value(DidOpenTextDocumentParams { text_document: document })
                            .expect("serialize"),
                    );
                    let published = driver.await_publish();
                    check_update(dir.path(), index, version, &text, &published)?;
                }
                Op::Change(index, text) => {
                    version += 1;
                    driver.notify(
                        "textDocument/didChange",
                        to_value(DidChangeTextDocumentParams {
                            text_document: VersionedTextDocumentIdentifier {
                                uri: doc_uri(dir.path(), index),
                                version,
                            },
                            content_changes: vec![TextDocumentContentChangeEvent {
                                range: None,
                                range_length: None,
                                text: text.clone(),
                            }],
                        })
                        .expect("serialize"),
                    );
                    let published = driver.await_publish();
                    check_update(dir.path(), index, version, &text, &published)?;
                }
                Op::Close(index) => {
                    driver.notify(
                        "textDocument/didClose",
                        to_value(DidCloseTextDocumentParams {
                            text_document: TextDocumentIdentifier {
                                uri: doc_uri(dir.path(), index),
                            },
                        })
                        .expect("serialize"),
                    );
                    let published = driver.await_publish();
                    prop_assert!(
                        published.diagnostics.is_empty(),
                        "closing a document clears its diagnostics"
                    );
                }
                Op::CodeAction(index) => {
                    let id = driver
                        .request("textDocument/codeAction", code_action_params(doc_uri(dir.path(), index)));
                    let Message::Response(response) = driver.await_response(&id) else {
                        unreachable!("await_response returns a response");
                    };
                    prop_assert!(response.error.is_none(), "code action never errors");
                }
                Op::Formatting(index) => {
                    let id = driver
                        .request("textDocument/formatting", formatting_params(doc_uri(dir.path(), index)));
                    let Message::Response(response) = driver.await_response(&id) else {
                        unreachable!("await_response returns a response");
                    };
                    prop_assert!(response.error.is_none(), "formatting never errors");
                }
                Op::Hover(index) => {
                    let params = json!({
                        "textDocument": { "uri": doc_uri(dir.path(), index) },
                        "position": { "line": 0, "character": 0 },
                    });
                    let id = driver.request("textDocument/hover", params);
                    let Message::Response(response) = driver.await_response(&id) else {
                        unreachable!("await_response returns a response");
                    };
                    prop_assert!(response.error.is_none(), "hover never errors");
                }
                Op::Diagnostic(index) => {
                    let params =
                        json!({ "textDocument": { "uri": doc_uri(dir.path(), index) } });
                    let id = driver.request("textDocument/diagnostic", params);
                    let Message::Response(response) = driver.await_response(&id) else {
                        unreachable!("await_response returns a response");
                    };
                    prop_assert!(response.error.is_none(), "pull diagnostic never errors");
                }
            }
        }

        // A clean shutdown/exit must terminate the server; Drop then joins the
        // thread, so a panic or hang in the loop surfaces here.
        driver.shutdown();
    }

    #[test]
    fn incremental_edits_keep_diagnostics_faithful(
        initial in arb_document().prop_map(|document| document.render()),
        inserts in prop::collection::vec(
            (0u32..6, prop_oneof![Just(" "), Just("x"), Just("k: 1\n")]),
            0..6,
        ),
    ) {
        let dir = tempdir().expect("tempdir");
        std::fs::write(dir.path().join(".ryl.toml"), CONFIG).expect("write config");
        let mut driver = Driver::start();
        let uri = doc_uri(dir.path(), 0);
        let mut version = 1;
        driver.notify(
            "textDocument/didOpen",
            to_value(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: uri.clone(),
                    language_id: "yaml".to_string(),
                    version,
                    text: initial.clone(),
                },
            })
            .expect("serialize"),
        );
        let published = driver.await_publish();
        let mut model = initial;
        check_update(dir.path(), 0, version, &model, &published)?;

        for (line, insert) in inserts {
            // The server applies the ranged edit via its own converter; the model
            // applies it at the independently-computed line-start byte (column 0
            // is unit-unambiguous), so the two texts must stay byte-identical.
            let at = line_start_byte(&model, line);
            model.insert_str(at, insert);
            version += 1;
            driver.notify(
                "textDocument/didChange",
                to_value(DidChangeTextDocumentParams {
                    text_document: VersionedTextDocumentIdentifier {
                        uri: uri.clone(),
                        version,
                    },
                    content_changes: vec![TextDocumentContentChangeEvent {
                        range: Some(Range::new(
                            Position::new(line, 0),
                            Position::new(line, 0),
                        )),
                        range_length: None,
                        text: insert.to_string(),
                    }],
                })
                .expect("serialize"),
            );
            let published = driver.await_publish();
            check_update(dir.path(), 0, version, &model, &published)?;
        }
        driver.shutdown();
    }
}
