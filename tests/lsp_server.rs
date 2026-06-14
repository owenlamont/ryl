#![cfg(feature = "lsp")]
//! End-to-end tests for `ryl server`. Most drive the protocol over an in-process
//! `Connection::memory()` pair (clean Message-level assertions); one drives the
//! real `ryl server` binary over stdio so the `run()` / subcommand wiring is
//! covered too.

use std::io::BufReader;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::str::FromStr;
use std::thread::{self, JoinHandle};

use lsp_server::{Connection, Message, Notification, Request, RequestId, Response};
use lsp_types::{
    ClientCapabilities, CodeActionContext, CodeActionKind, CodeActionOrCommand,
    CodeActionParams, CodeActionResponse, Diagnostic, DidChangeTextDocumentParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DocumentChanges,
    DocumentFormattingParams, FormattingOptions, GeneralClientCapabilities,
    InitializeParams, InitializeResult, OneOf, PartialResultParams,
    PositionEncodingKind, PublishDiagnosticsParams, Range,
    TextDocumentContentChangeEvent, TextDocumentIdentifier, TextDocumentItem, TextEdit,
    Uri, VersionedTextDocumentIdentifier, WorkDoneProgressParams,
    WorkspaceClientCapabilities, WorkspaceEditClientCapabilities, WorkspaceFolder,
};
use serde_json::Value;
use tempfile::{TempDir, tempdir};

const TRAILING: &str = "[rules]\ntrailing-spaces = \"enable\"\n";

fn uri(text: &str) -> Uri {
    Uri::from_str(text).expect("valid URI")
}

fn file_uri(dir: &Path, name: &str) -> Uri {
    uri(&format!("file://{}/{}", dir.display(), name))
}

/// A temp project directory carrying a `.ryl.toml`; the adjacent config shields
/// discovery from any stray config higher up the tree.
fn project(config: &str) -> TempDir {
    let dir = tempdir().expect("tempdir");
    std::fs::write(dir.path().join(".ryl.toml"), config).expect("write config");
    dir
}

/// In-process client driving a server thread over a memory connection.
struct Client {
    conn: Option<Connection>,
    thread: Option<JoinHandle<()>>,
    next_id: i32,
}

impl Client {
    fn launch(
        encodings: Option<Vec<PositionEncodingKind>>,
        root: Option<&Path>,
    ) -> (Self, InitializeResult) {
        // Default to advertising versioned-edit support, as real clients do.
        Self::launch_full(encodings, root, true, None)
    }

    fn launch_full(
        encodings: Option<Vec<PositionEncodingKind>>,
        root: Option<&Path>,
        document_changes: bool,
        root_uri: Option<&Path>,
    ) -> (Self, InitializeResult) {
        let (server, client) = Connection::memory();
        let thread = thread::spawn(move || {
            let _ = ryl::lsp::serve(&server);
        });
        let mut this = Client {
            conn: Some(client),
            thread: Some(thread),
            next_id: 0,
        };
        let init = this.initialize(encodings, root, document_changes, root_uri);
        (this, init)
    }

    fn conn(&self) -> &Connection {
        self.conn.as_ref().expect("connection live")
    }

    fn request<P: serde::Serialize>(&mut self, method: &str, params: P) -> RequestId {
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

    fn notify<P: serde::Serialize>(&self, method: &str, params: P) {
        self.conn()
            .sender
            .send(Message::Notification(Notification::new(
                method.to_string(),
                params,
            )))
            .expect("send notification");
    }

    fn send_raw(&self, message: Message) {
        self.conn().sender.send(message).expect("send raw");
    }

    /// Receive the response to `id`, skipping any notifications queued ahead of it.
    fn response(&self, id: &RequestId) -> Response {
        loop {
            match self.conn().receiver.recv().expect("recv") {
                Message::Response(response) if &response.id == id => return response,
                _ => {}
            }
        }
    }

    fn diagnostics(&self) -> Vec<Diagnostic> {
        self.publish_params().diagnostics
    }

    /// Receive the next `publishDiagnostics` notification's full params (carrying
    /// the version the diagnostics were computed against).
    fn publish_params(&self) -> PublishDiagnosticsParams {
        loop {
            if let Message::Notification(note) =
                self.conn().receiver.recv().expect("recv")
                && note.method == "textDocument/publishDiagnostics"
            {
                return serde_json::from_value(note.params)
                    .expect("diagnostics params");
            }
        }
    }

    /// Drain messages up to and including the next `publishDiagnostics`, so a test
    /// can assert whether a `window/showMessage` preceded it.
    fn drain_to_publish(&self) -> Vec<Message> {
        let mut messages = Vec::new();
        loop {
            let message = self.conn().receiver.recv().expect("recv");
            let is_publish = matches!(
                &message,
                Message::Notification(note)
                    if note.method == "textDocument/publishDiagnostics"
            );
            messages.push(message);
            if is_publish {
                return messages;
            }
        }
    }

    #[allow(deprecated)] // root_uri is set deliberately to exercise the fallback
    fn initialize(
        &mut self,
        encodings: Option<Vec<PositionEncodingKind>>,
        root: Option<&Path>,
        document_changes: bool,
        root_uri: Option<&Path>,
    ) -> InitializeResult {
        let params = InitializeParams {
            capabilities: ClientCapabilities {
                general: encodings.map(|position_encodings| {
                    GeneralClientCapabilities {
                        position_encodings: Some(position_encodings),
                        ..Default::default()
                    }
                }),
                workspace: document_changes.then(|| WorkspaceClientCapabilities {
                    workspace_edit: Some(WorkspaceEditClientCapabilities {
                        document_changes: Some(true),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
            workspace_folders: root.map(|path| {
                vec![WorkspaceFolder {
                    uri: file_uri(path, ""),
                    name: "root".to_string(),
                }]
            }),
            root_uri: root_uri.map(|path| file_uri(path, "")),
            ..Default::default()
        };
        let id = self.request("initialize", params);
        let response = self.response(&id);
        // lsp-server's `initialize_finish` blocks until the client confirms.
        self.notify("initialized", serde_json::json!({}));
        serde_json::from_value(response.result.expect("initialize result"))
            .expect("InitializeResult")
    }

    fn did_open(&self, uri: Uri, text: &str) {
        self.notify(
            "textDocument/didOpen",
            DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri,
                    language_id: "yaml".to_string(),
                    version: 1,
                    text: text.to_string(),
                },
            },
        );
    }

    fn did_change(&self, uri: Uri, text: &str) {
        self.notify(
            "textDocument/didChange",
            DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier { uri, version: 2 },
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: text.to_string(),
                }],
            },
        );
    }

    fn did_close(&self, uri: Uri) {
        self.notify(
            "textDocument/didClose",
            DidCloseTextDocumentParams {
                text_document: TextDocumentIdentifier { uri },
            },
        );
    }

    fn code_action(&mut self, uri: Uri) -> Option<CodeActionResponse> {
        self.code_action_with(uri, None)
    }

    fn code_action_with(
        &mut self,
        uri: Uri,
        only: Option<Vec<CodeActionKind>>,
    ) -> Option<CodeActionResponse> {
        let params = CodeActionParams {
            text_document: TextDocumentIdentifier { uri },
            range: Range::default(),
            context: CodeActionContext {
                diagnostics: Vec::new(),
                only,
                trigger_kind: None,
            },
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        };
        let id = self.request("textDocument/codeAction", params);
        let response = self.response(&id);
        serde_json::from_value(response.result.expect("code action result"))
            .expect("response")
    }

    fn formatting(&mut self, uri: Uri) -> Option<Vec<TextEdit>> {
        let params = DocumentFormattingParams {
            text_document: TextDocumentIdentifier { uri },
            options: FormattingOptions::default(),
            work_done_progress_params: WorkDoneProgressParams::default(),
        };
        let id = self.request("textDocument/formatting", params);
        let response = self.response(&id);
        serde_json::from_value(response.result.expect("formatting result"))
            .expect("response")
    }

    /// Confirm the server still processes messages (an unknown request gets an
    /// error response). Used after sending something the server should ignore.
    fn assert_alive(&mut self) {
        let id = self.request("textDocument/hover", Value::Null);
        let response = self.response(&id);
        assert!(
            response.error.is_some(),
            "unknown request yields an error response"
        );
    }

    fn shutdown(&mut self) {
        let id = self.request("shutdown", Value::Null);
        let response = self.response(&id);
        assert!(response.error.is_none(), "shutdown succeeds");
        self.notify("exit", Value::Null);
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        // Dropping the client connection ends the server loop if it is still running.
        drop(self.conn.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[test]
fn initialize_advertises_linter_capabilities() {
    let (_client, init) = Client::launch(None, None);
    let caps = init.capabilities;
    assert_eq!(
        caps.position_encoding,
        Some(PositionEncodingKind::UTF16),
        "defaults to UTF-16 when the client states no preference"
    );
    assert!(
        caps.text_document_sync.is_some(),
        "advertises document sync"
    );
    assert!(
        caps.code_action_provider.is_some(),
        "advertises code actions"
    );
    assert!(
        caps.document_formatting_provider.is_some(),
        "advertises formatting"
    );
}

#[test]
fn initialize_negotiates_utf8() {
    let (_client, init) = Client::launch(Some(vec![PositionEncodingKind::UTF8]), None);
    assert_eq!(
        init.capabilities.position_encoding,
        Some(PositionEncodingKind::UTF8)
    );
}

#[test]
fn initialize_negotiates_utf32() {
    let (_client, init) = Client::launch(Some(vec![PositionEncodingKind::UTF32]), None);
    assert_eq!(
        init.capabilities.position_encoding,
        Some(PositionEncodingKind::UTF32)
    );
}

#[test]
fn did_open_publishes_diagnostics() {
    let dir = project(TRAILING);
    let (client, _init) = Client::launch(None, None);
    client.did_open(file_uri(dir.path(), "x.yaml"), "a: 1 \n");
    let diagnostics = client.diagnostics();
    assert_eq!(diagnostics.len(), 1, "the trailing space is reported");
    assert_eq!(diagnostics[0].source.as_deref(), Some("ryl"));
}

#[test]
fn published_diagnostics_carry_the_document_version() {
    let dir = project(TRAILING);
    let (client, _init) = Client::launch(None, None);
    client.did_open(file_uri(dir.path(), "x.yaml"), "a: 1 \n");
    assert_eq!(
        client.publish_params().version,
        Some(1),
        "diagnostics are tagged with the version they were computed against"
    );
}

#[test]
fn did_change_republishes_diagnostics() {
    let dir = project(TRAILING);
    let (client, _init) = Client::launch(None, None);
    let doc = file_uri(dir.path(), "x.yaml");
    client.did_open(doc.clone(), "a: 1\n");
    assert!(client.diagnostics().is_empty(), "clean on open");
    client.did_change(doc, "a: 1 \n");
    assert_eq!(
        client.diagnostics().len(),
        1,
        "the edit introduces a problem"
    );
}

#[test]
fn did_close_clears_diagnostics() {
    let dir = project(TRAILING);
    let (client, _init) = Client::launch(None, None);
    let doc = file_uri(dir.path(), "x.yaml");
    client.did_open(doc.clone(), "a: 1 \n");
    assert_eq!(client.diagnostics().len(), 1, "problem on open");
    client.did_close(doc);
    assert!(
        client.diagnostics().is_empty(),
        "closing clears diagnostics"
    );
}

#[test]
fn untitled_document_with_empty_root_has_no_diagnostics() {
    // An untitled (non-file) buffer anchored at a config-less workspace root: no
    // config is discovered, so nothing is linted.
    let root = tempdir().expect("tempdir");
    let (client, _init) = Client::launch(None, Some(root.path()));
    client.did_open(uri("untitled:Untitled-1"), "a: 1 \n");
    assert!(
        client.diagnostics().is_empty(),
        "no config -> no diagnostics"
    );
}

#[test]
fn untitled_document_without_root_is_handled() {
    // No workspace root: discovery falls back to the process cwd. Whatever it finds,
    // the server must stay responsive (exercises the root-absent fallback branch).
    let (mut client, _init) = Client::launch(None, None);
    client.did_open(uri("untitled:Untitled-2"), "a: 1\n");
    let _ = client.diagnostics();
    client.assert_alive();
}

#[test]
fn untitled_document_anchors_config_at_root_uri() {
    // An older client that sends only root_uri (no workspace folders): the untitled
    // buffer's config is discovered via root_uri, so it is still linted.
    let dir = project(TRAILING);
    let (client, _init) = Client::launch_full(None, None, true, Some(dir.path()));
    client.did_open(uri("untitled:Untitled-3"), "a: 1 \n");
    assert_eq!(
        client.diagnostics().len(),
        1,
        "root_uri anchors discovery so the untitled buffer is linted"
    );
}

#[test]
fn untitled_buffer_is_linted_as_yaml_despite_custom_file_globs() {
    // A project whose `[files].yaml` would not match the synthetic `untitled.yaml`
    // must still lint an unsaved YAML buffer as YAML (non-file URIs force YAML).
    let dir = project(
        "[files]\nyaml = [\"config/*.yml\"]\n[rules]\ntrailing-spaces = \"enable\"\n",
    );
    let (client, _init) = Client::launch_full(None, None, true, Some(dir.path()));
    client.did_open(uri("untitled:Untitled-4"), "a: 1 \n");
    assert_eq!(
        client.diagnostics().len(),
        1,
        "an untitled buffer is linted as YAML regardless of [files] globs"
    );
}

#[test]
fn code_action_offers_versioned_fix_all() {
    let dir = project(TRAILING);
    let (mut client, _init) = Client::launch(None, None); // advertises documentChanges
    let doc = file_uri(dir.path(), "x.yaml");
    client.did_open(doc.clone(), "a: 1 \n"); // version 1
    let _ = client.diagnostics();
    let actions = client
        .code_action(doc)
        .expect("a fixable document offers an action");
    assert_eq!(actions.len(), 1, "one fix-all action");
    let CodeActionOrCommand::CodeAction(action) = &actions[0] else {
        panic!("expected a CodeAction, not a Command");
    };
    assert_eq!(
        action.kind.as_ref().map(|k| k.as_str()),
        Some("source.fixAll.ryl")
    );
    let Some(DocumentChanges::Edits(edits)) = action
        .edit
        .as_ref()
        .and_then(|edit| edit.document_changes.as_ref())
    else {
        panic!("a documentChanges-capable client gets a versioned edit");
    };
    assert_eq!(
        edits[0].text_document.version,
        Some(1),
        "the edit is stamped with the document version so a stale apply is rejected"
    );
    let OneOf::Left(text_edit) = &edits[0].edits[0] else {
        panic!("expected a plain TextEdit");
    };
    assert_eq!(
        text_edit.new_text, "a: 1\n",
        "the fix removes the trailing space"
    );
}

#[test]
fn code_action_falls_back_to_changes_without_document_changes_support() {
    let dir = project(TRAILING);
    // A client that does not advertise documentChanges gets the unversioned map.
    let (mut client, _init) = Client::launch_full(None, None, false, None);
    let doc = file_uri(dir.path(), "x.yaml");
    client.did_open(doc.clone(), "a: 1 \n");
    let _ = client.diagnostics();
    let actions = client
        .code_action(doc)
        .expect("a fixable document offers an action");
    let CodeActionOrCommand::CodeAction(action) = &actions[0] else {
        panic!("expected a CodeAction, not a Command");
    };
    let edits = action
        .edit
        .as_ref()
        .and_then(|edit| edit.changes.as_ref())
        .map(|changes| changes.values().next().expect("one file's edits"))
        .expect("the unversioned changes map");
    assert_eq!(
        edits[0].new_text, "a: 1\n",
        "fix-all still works via changes"
    );
}

#[test]
fn code_action_honours_a_matching_only_filter() {
    let dir = project(TRAILING);
    let (mut client, _init) = Client::launch(None, None);
    let doc = file_uri(dir.path(), "x.yaml");
    client.did_open(doc.clone(), "a: 1 \n");
    let _ = client.diagnostics();
    // `source.fixAll` is an ancestor of `source.fixAll.ryl`, as codeActionsOnSave issues it.
    let only = Some(vec![CodeActionKind::new("source.fixAll")]);
    assert!(
        client.code_action_with(doc, only).is_some(),
        "a source.fixAll request still gets the fix-all action"
    );
}

#[test]
fn code_action_skips_an_unrelated_only_filter() {
    let dir = project(TRAILING);
    let (mut client, _init) = Client::launch(None, None);
    let doc = file_uri(dir.path(), "x.yaml");
    client.did_open(doc.clone(), "a: 1 \n");
    let _ = client.diagnostics();
    assert!(
        client
            .code_action_with(doc, Some(vec![CodeActionKind::QUICKFIX]))
            .is_none(),
        "a quickfix-only request does not get the source.fixAll action"
    );
}

#[test]
fn code_action_on_clean_document_is_null() {
    let dir = project(TRAILING);
    let (mut client, _init) = Client::launch(None, None);
    let doc = file_uri(dir.path(), "x.yaml");
    client.did_open(doc.clone(), "a: 1\n");
    let _ = client.diagnostics();
    assert!(
        client.code_action(doc).is_none(),
        "nothing to fix -> no action"
    );
}

#[test]
fn code_action_on_unopened_document_is_null() {
    let dir = project(TRAILING);
    let (mut client, _init) = Client::launch(None, None);
    assert!(
        client
            .code_action(file_uri(dir.path(), "never-opened.yaml"))
            .is_none(),
        "an unopened document has no edit"
    );
}

#[test]
fn formatting_on_unopened_document_is_null() {
    let dir = project(TRAILING);
    let (mut client, _init) = Client::launch(None, None);
    assert!(
        client
            .formatting(file_uri(dir.path(), "never-opened.yaml"))
            .is_none(),
        "formatting an unopened document yields no edits"
    );
}

#[test]
fn code_action_with_unresolvable_config_is_null() {
    // The document is open, but its config does not resolve to any enabled rule, so
    // there is no fix-all edit to offer.
    let dir = project("this is not valid toml = =\n");
    let (mut client, _init) = Client::launch(None, None);
    let doc = file_uri(dir.path(), "x.yaml");
    client.did_open(doc.clone(), "a: 1 \n");
    let _ = client.diagnostics();
    assert!(
        client.code_action(doc).is_none(),
        "no resolvable config -> no action"
    );
}

#[test]
fn formatting_returns_fix_edits() {
    let dir = project(TRAILING);
    let (mut client, _init) = Client::launch(None, None);
    let doc = file_uri(dir.path(), "x.yaml");
    client.did_open(doc.clone(), "a: 1 \n");
    let _ = client.diagnostics();
    let edits = client.formatting(doc).expect("a fixable document formats");
    assert_eq!(edits[0].new_text, "a: 1\n");
}

#[test]
fn formatting_a_clean_document_is_null() {
    let dir = project(TRAILING);
    let (mut client, _init) = Client::launch(None, None);
    let doc = file_uri(dir.path(), "x.yaml");
    client.did_open(doc.clone(), "a: 1\n");
    let _ = client.diagnostics();
    assert!(
        client.formatting(doc).is_none(),
        "a clean document needs no formatting"
    );
}

#[test]
fn unknown_request_is_method_not_found() {
    let (mut client, _init) = Client::launch(None, None);
    let id = client.request("textDocument/hover", Value::Null);
    let response = client.response(&id);
    let error = response.error.expect("unknown method errors");
    assert_eq!(error.code, lsp_server::ErrorCode::MethodNotFound as i32);
}

#[test]
fn malformed_request_params_yield_a_null_result() {
    let (mut client, _init) = Client::launch(None, None);
    // codeAction with the wrong params shape: parsing fails, so the server replies
    // with a null result rather than crashing.
    let id = client.request(
        "textDocument/codeAction",
        Value::String("not params".to_string()),
    );
    let response = client.response(&id);
    assert_eq!(
        response.result,
        Some(Value::Null),
        "bad params -> null result"
    );
    assert!(response.error.is_none());
}

#[test]
fn malformed_notification_is_ignored() {
    let (mut client, _init) = Client::launch(None, None);
    client.notify("textDocument/didOpen", Value::String("bad".to_string()));
    client.assert_alive();
}

#[test]
fn malformed_change_and_close_notifications_are_ignored() {
    let (mut client, _init) = Client::launch(None, None);
    // Wrong params shape for each: the if-let chain short-circuits and the server
    // simply stays responsive (no update, no removal).
    client.notify("textDocument/didChange", Value::String("bad".to_string()));
    client.notify("textDocument/didClose", Value::String("bad".to_string()));
    client.assert_alive();
}

#[test]
fn unrelated_notification_is_ignored() {
    let (mut client, _init) = Client::launch(None, None);
    client.notify("initialized", serde_json::json!({}));
    client.assert_alive();
}

#[test]
fn client_response_is_ignored() {
    let (mut client, _init) = Client::launch(None, None);
    client.send_raw(Message::Response(Response::new_ok(
        RequestId::from(999),
        Value::Null,
    )));
    client.assert_alive();
}

#[test]
fn shutdown_and_exit_end_the_session() {
    let (mut client, _init) = Client::launch(None, None);
    client.shutdown();
    // The server thread should now finish; Drop joins it without hanging.
}

#[test]
fn serve_returns_cleanly_without_an_initialize() {
    // No valid `initialize` arrives before the client drops: the server must end
    // the session gracefully, not panic.
    let (server, client) = Connection::memory();
    let handle = thread::spawn(move || ryl::lsp::serve(&server));
    client
        .sender
        .send(Message::Notification(Notification::new(
            "initialized".to_string(),
            Value::Null,
        )))
        .expect("send");
    drop(client);
    assert!(handle.join().is_ok(), "serve returns without panicking");
}

#[test]
fn serve_rejects_malformed_initialize_params() {
    // An `initialize` request whose params are the wrong shape is rejected with an
    // error response (so a real client stops waiting), not silently dropped.
    let (server, client) = Connection::memory();
    let id = RequestId::from(1);
    let handle = thread::spawn(move || ryl::lsp::serve(&server));
    client
        .sender
        .send(Message::Request(Request::new(
            id.clone(),
            "initialize".to_string(),
            Value::String("not the InitializeParams shape".to_string()),
        )))
        .expect("send");
    let Message::Response(response) = client.receiver.recv().expect("recv") else {
        panic!("expected an error response to the malformed initialize");
    };
    assert_eq!(response.id, id);
    assert!(
        response.error.is_some(),
        "malformed initialize is rejected with an error"
    );
    drop(client);
    assert!(handle.join().is_ok(), "serve returns without panicking");
}

#[test]
fn rejected_initialize_then_exit_is_abnormal() {
    // After a rejected handshake, the server keeps draining and treats a bare
    // `exit` as an abnormal session end (rather than hanging or panicking).
    let (server, client) = Connection::memory();
    let handle = thread::spawn(move || ryl::lsp::serve(&server));
    client
        .sender
        .send(Message::Request(Request::new(
            RequestId::from(1),
            "initialize".to_string(),
            Value::String("bad".to_string()),
        )))
        .expect("send initialize");
    let _ = client.receiver.recv().expect("error response");
    // A non-exit notification is ignored by the drain; only `exit` ends it.
    client
        .sender
        .send(Message::Notification(Notification::new(
            "initialized".to_string(),
            Value::Null,
        )))
        .expect("send initialized");
    client
        .sender
        .send(Message::Notification(Notification::new(
            "exit".to_string(),
            Value::Null,
        )))
        .expect("send exit");
    let outcome = handle.join().expect("serve returns");
    assert_eq!(outcome, ryl::lsp::SessionOutcome::Abnormal);
    drop(client);
}

#[test]
fn bare_exit_notification_ends_the_session() {
    // A bare `exit` (no prior `shutdown`) must terminate the server even with the
    // connection still open, otherwise the process would orphan.
    let (server, client) = Connection::memory();
    let handle = thread::spawn(move || ryl::lsp::serve(&server));
    client
        .sender
        .send(Message::Request(Request::new(
            RequestId::from(1),
            "initialize".to_string(),
            serde_json::to_value(InitializeParams::default()).expect("serialize"),
        )))
        .expect("send initialize");
    client
        .sender
        .send(Message::Notification(Notification::new(
            "initialized".to_string(),
            Value::Null,
        )))
        .expect("send initialized");
    client
        .sender
        .send(Message::Notification(Notification::new(
            "exit".to_string(),
            Value::Null,
        )))
        .expect("send exit");
    // Connection is still open; the server must terminate on `exit` regardless,
    // and report it as an abnormal exit (no prior shutdown) per the LSP spec.
    let outcome = handle.join().expect("server terminates on a bare exit");
    assert_eq!(outcome, ryl::lsp::SessionOutcome::Abnormal);
    drop(client);
}

fn is_show_message(message: &Message) -> bool {
    matches!(
        message,
        Message::Notification(note) if note.method == "window/showMessage"
    )
}

#[test]
fn malformed_config_is_reported_to_the_user_once() {
    let dir = project("this is not valid toml = =\n");
    let (client, _init) = Client::launch(None, None);
    let doc = file_uri(dir.path(), "a.yaml");
    client.did_open(doc.clone(), "x: 1\n");
    assert!(
        client.drain_to_publish().iter().any(is_show_message),
        "a broken config is surfaced via window/showMessage, not silently ignored"
    );
    // Re-linting the same broken config does not repeat the popup.
    client.did_change(doc, "x: 2\n");
    assert!(
        !client.drain_to_publish().iter().any(is_show_message),
        "the same config error is reported only once"
    );
}

#[test]
fn invalid_project_config_yields_no_diagnostics() {
    let dir = project("this is not valid toml = =\n");
    let (client, _init) = Client::launch(None, None);
    client.did_open(file_uri(dir.path(), "x.yaml"), "a: 1 \n");
    assert!(
        client.diagnostics().is_empty(),
        "a broken config is skipped, not crashed on"
    );
}

#[test]
fn file_matching_no_source_kind_yields_no_diagnostics() {
    let dir = project(TRAILING);
    let (client, _init) = Client::launch(None, None);
    // A `.txt` matches neither the default yaml globs nor any markdown glob.
    client.did_open(file_uri(dir.path(), "notes.txt"), "a: 1 \n");
    assert!(
        client.diagnostics().is_empty(),
        "non-yaml/markdown files are not linted"
    );
}

#[test]
fn config_ignored_file_is_not_linted_or_fixed() {
    // A file excluded by the config's `ignore` is skipped, exactly as the CLI does,
    // so the editor neither flags nor offers to fix it.
    let dir =
        project("ignore = ['skip.yaml']\n[rules]\ntrailing-spaces = \"enable\"\n");
    let (mut client, _init) = Client::launch(None, None);
    let doc = file_uri(dir.path(), "skip.yaml");
    client.did_open(doc.clone(), "a: 1 \n");
    assert!(
        client.diagnostics().is_empty(),
        "an ignored file gets no diagnostics"
    );
    assert!(
        client.code_action(doc).is_none(),
        "and no fix-all action for an ignored file"
    );
}

#[test]
fn file_matching_two_source_kinds_is_reported() {
    // The same glob under both kinds makes the source kind ambiguous (a hard error
    // in the CLI); the server surfaces it like any other config mistake.
    let dir = project(
        "[files]\nyaml = [\"*.data\"]\nmarkdown = [\"*.data\"]\n[rules]\ntrailing-spaces = \"enable\"\n",
    );
    let (client, _init) = Client::launch(None, None);
    client.did_open(file_uri(dir.path(), "x.data"), "a: 1 \n");
    let messages = client.drain_to_publish();
    assert!(
        messages.iter().any(is_show_message),
        "ambiguous source kind is surfaced via window/showMessage"
    );
}

// --- Real-binary stdio smoke test: covers run() and the subcommand dispatch. ---

fn read_message(reader: &mut impl std::io::BufRead) -> Message {
    Message::read(reader)
        .expect("read message")
        .expect("message, not EOF")
}

#[test]
fn server_binary_runs_over_stdio() {
    let dir = project(TRAILING);
    let mut child: Child = Command::new(env!("CARGO_BIN_EXE_ryl"))
        .arg("server")
        // Bound config discovery; the adjacent .ryl.toml is found first regardless.
        .env("HOME", dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn ryl server");
    let mut stdin = child.stdin.take().expect("child stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("child stdout"));

    let init = InitializeParams::default();
    Message::Request(Request::new(
        RequestId::from(1),
        "initialize".to_string(),
        init,
    ))
    .write(&mut stdin)
    .expect("write initialize");
    let Message::Response(_) = read_message(&mut stdout) else {
        panic!("expected initialize response");
    };
    Message::Notification(Notification::new(
        "initialized".to_string(),
        serde_json::json!({}),
    ))
    .write(&mut stdin)
    .expect("write initialized");

    let open = DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: file_uri(dir.path(), "x.yaml"),
            language_id: "yaml".to_string(),
            version: 1,
            text: "a: 1 \n".to_string(),
        },
    };
    Message::Notification(Notification::new("textDocument/didOpen".to_string(), open))
        .write(&mut stdin)
        .expect("write didOpen");
    let Message::Notification(note) = read_message(&mut stdout) else {
        panic!("expected publishDiagnostics");
    };
    assert_eq!(note.method, "textDocument/publishDiagnostics");

    Message::Request(Request::new(
        RequestId::from(2),
        "shutdown".to_string(),
        Value::Null,
    ))
    .write(&mut stdin)
    .expect("write shutdown");
    let _ = read_message(&mut stdout);
    Message::Notification(Notification::new("exit".to_string(), Value::Null))
        .write(&mut stdin)
        .expect("write exit");
    drop(stdin); // EOF lets the stdio reader thread (and the process) finish.

    let status = child.wait().expect("wait for child");
    assert!(status.success(), "clean shutdown exits 0");
}

#[test]
fn server_binary_exits_nonzero_on_bare_exit() {
    let dir = project(TRAILING);
    let mut child: Child = Command::new(env!("CARGO_BIN_EXE_ryl"))
        .arg("server")
        .env("HOME", dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn ryl server");
    let mut stdin = child.stdin.take().expect("child stdin");
    let mut stdout = BufReader::new(child.stdout.take().expect("child stdout"));

    Message::Request(Request::new(
        RequestId::from(1),
        "initialize".to_string(),
        InitializeParams::default(),
    ))
    .write(&mut stdin)
    .expect("write initialize");
    let Message::Response(_) = read_message(&mut stdout) else {
        panic!("expected initialize response");
    };
    Message::Notification(Notification::new(
        "initialized".to_string(),
        serde_json::json!({}),
    ))
    .write(&mut stdin)
    .expect("write initialized");
    // A bare `exit` with no prior `shutdown` is an abnormal termination (exit 1).
    Message::Notification(Notification::new("exit".to_string(), Value::Null))
        .write(&mut stdin)
        .expect("write exit");
    drop(stdin); // EOF lets the stdio reader thread finish.

    let status = child.wait().expect("wait for child");
    assert_eq!(status.code(), Some(1), "a bare exit (no shutdown) exits 1");
}
