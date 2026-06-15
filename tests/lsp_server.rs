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
    CodeActionParams, CodeActionResponse, Diagnostic, DiagnosticClientCapabilities,
    DiagnosticWorkspaceClientCapabilities, DidChangeTextDocumentParams,
    DidChangeWatchedFilesClientCapabilities, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DocumentChanges, DocumentDiagnosticReport,
    DocumentFormattingParams, FormattingOptions, GeneralClientCapabilities, Hover,
    HoverContents, InitializeParams, InitializeResult, NumberOrString, OneOf,
    PartialResultParams, Position, PositionEncodingKind, PrepareRenameResponse,
    PublishDiagnosticsParams, Range, TextDocumentClientCapabilities,
    TextDocumentContentChangeEvent, TextDocumentIdentifier, TextDocumentItem, TextEdit,
    Uri, VersionedTextDocumentIdentifier, WorkDoneProgressParams,
    WorkspaceClientCapabilities, WorkspaceDiagnosticReport,
    WorkspaceDocumentDiagnosticReport, WorkspaceEdit, WorkspaceEditClientCapabilities,
    WorkspaceFolder,
};
use serde_json::{Value, json};
use tempfile::{TempDir, tempdir};

const TRAILING: &str = "[rules]\ntrailing-spaces = \"enable\"\n";

/// A method ryl does not (and will not) handle, so requesting it always yields a
/// `MethodNotFound` error — used to probe that the server is still responsive.
const UNHANDLED_METHOD: &str = "ryl/internalUnhandledProbe";

fn uri(text: &str) -> Uri {
    Uri::from_str(text).expect("valid URI")
}

fn file_uri(dir: &Path, name: &str) -> Uri {
    // Build a valid file URI cross-platform: forward slashes, and a leading slash
    // before a Windows drive (`C:/…` -> `/C:/…`) so it round-trips on every OS.
    let mut path = dir.join(name).display().to_string().replace('\\', "/");
    if !path.starts_with('/') {
        path.insert(0, '/');
    }
    uri(&format!("file://{path}"))
}

/// A temp project directory carrying a `.ryl.toml`; the adjacent config shields
/// discovery from any stray config higher up the tree.
fn project(config: &str) -> TempDir {
    let dir = tempdir().expect("tempdir");
    std::fs::write(dir.path().join(".ryl.toml"), config).expect("write config");
    dir
}

/// Assert none of the drained notifications is a diagnostics push — the invariant a
/// pull-capable client relies on: it gets diagnostics only via pull, never an extra
/// push that a client merging the two channels (VS Code) would double-count.
fn assert_no_diagnostics_push(notifications: &[Notification]) {
    assert!(
        notifications
            .iter()
            .all(|note| note.method != "textDocument/publishDiagnostics"),
        "a pull-capable client must not receive a publishDiagnostics push"
    );
}

/// The diagnostic items from a `textDocument/diagnostic` full-report response (ryl never
/// caches result ids, so the report is always full).
fn pull_items(response: Response) -> Vec<Diagnostic> {
    let report: DocumentDiagnosticReport =
        serde_json::from_value(response.result.expect("diagnostic result"))
            .expect("DocumentDiagnosticReport");
    let DocumentDiagnosticReport::Full(report) = report else {
        panic!("expected a full report");
    };
    report.full_document_diagnostic_report.items
}

/// A one-character ryl diagnostic for `rule` at a 0-based position, as a client echoes
/// back in a code-action request's context (carrying ryl's `source` as the real server
/// does, so the source filter accepts it).
fn diag(rule: &str, line: u32, character: u32) -> Diagnostic {
    Diagnostic {
        range: Range::new(
            Position::new(line, character),
            Position::new(line, character + 1),
        ),
        code: Some(NumberOrString::String(rule.to_string())),
        source: Some("ryl".to_string()),
        ..Default::default()
    }
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

    /// Launch advertising several workspace folders (a multi-root workspace).
    fn launch_roots(roots: &[&Path]) -> (Self, InitializeResult) {
        Self::launch_params(InitializeParams {
            workspace_folders: Some(
                roots
                    .iter()
                    .map(|path| WorkspaceFolder {
                        uri: file_uri(path, ""),
                        name: "root".to_string(),
                    })
                    .collect(),
            ),
            ..Default::default()
        })
    }

    /// Launch advertising the LSP 3.17 pull-diagnostics client capability (plus versioned
    /// edits, as a real pull client like VS Code does). The server must then rely on pull
    /// and never push `publishDiagnostics`. `refresh_support` advertises
    /// `workspace/diagnostic/refresh` so a config change can ask the client to re-pull.
    fn launch_pull(
        root: Option<&Path>,
        refresh_support: bool,
    ) -> (Self, InitializeResult) {
        Self::launch_params(InitializeParams {
            capabilities: ClientCapabilities {
                text_document: Some(TextDocumentClientCapabilities {
                    diagnostic: Some(DiagnosticClientCapabilities::default()),
                    ..Default::default()
                }),
                workspace: Some(WorkspaceClientCapabilities {
                    workspace_edit: Some(WorkspaceEditClientCapabilities {
                        document_changes: Some(true),
                        ..Default::default()
                    }),
                    diagnostic: refresh_support.then_some(
                        DiagnosticWorkspaceClientCapabilities {
                            refresh_support: Some(true),
                        },
                    ),
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
            ..Default::default()
        })
    }

    /// Spawn a server thread and complete the `initialize`/`initialized` handshake with
    /// the given client capabilities. The bespoke launches build their own params; the
    /// positional `launch_with` chain goes through `initialize` instead.
    fn launch_params(params: InitializeParams) -> (Self, InitializeResult) {
        let (server, client) = Connection::memory();
        let thread = thread::spawn(move || {
            let _ = ryl::lsp::serve(&server);
        });
        let mut this = Client {
            conn: Some(client),
            thread: Some(thread),
            next_id: 0,
        };
        let id = this.request("initialize", params);
        let response = this.response(&id);
        this.notify("initialized", serde_json::json!({}));
        let init = serde_json::from_value(response.result.expect("initialize result"))
            .expect("InitializeResult");
        (this, init)
    }

    fn launch_full(
        encodings: Option<Vec<PositionEncodingKind>>,
        root: Option<&Path>,
        document_changes: bool,
        root_uri: Option<&Path>,
    ) -> (Self, InitializeResult) {
        Self::launch_with(encodings, root, document_changes, root_uri, false, None)
    }

    fn launch_with(
        encodings: Option<Vec<PositionEncodingKind>>,
        root: Option<&Path>,
        document_changes: bool,
        root_uri: Option<&Path>,
        watch: bool,
        options: Option<Value>,
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
        let init = this.initialize(
            encodings,
            root,
            document_changes,
            root_uri,
            watch,
            options,
        );
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

    /// Drain messages up to and including the response to `id`, returning every
    /// notification seen on the way. Because the server's loop is single-threaded and
    /// in-order, any push triggered by an earlier notification arrives before this
    /// response — so a test can assert a pull-capable client got *no* `publishDiagnostics`.
    fn notifications_until_response(
        &self,
        id: &RequestId,
    ) -> (Vec<Notification>, Response) {
        let mut notifications = Vec::new();
        loop {
            match self.conn().receiver.recv().expect("recv") {
                Message::Response(response) if &response.id == id => {
                    return (notifications, response);
                }
                Message::Notification(note) => notifications.push(note),
                _ => {}
            }
        }
    }

    /// Drain messages up to and including the response to `id`, returning every message
    /// (server-to-client requests *and* notifications) seen on the way. Lets a test assert
    /// the server sent a particular request — e.g. `workspace/diagnostic/refresh` — while
    /// confirming no diagnostics push escaped.
    fn messages_until_response(&self, id: &RequestId) -> (Vec<Message>, Response) {
        let mut messages = Vec::new();
        loop {
            match self.conn().receiver.recv().expect("recv") {
                Message::Response(response) if &response.id == id => {
                    return (messages, response);
                }
                other => messages.push(other),
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
        watch: bool,
        options: Option<Value>,
    ) -> InitializeResult {
        let params = InitializeParams {
            capabilities: ClientCapabilities {
                general: encodings.map(|position_encodings| {
                    GeneralClientCapabilities {
                        position_encodings: Some(position_encodings),
                        ..Default::default()
                    }
                }),
                workspace: (document_changes || watch).then(|| {
                    WorkspaceClientCapabilities {
                        workspace_edit: document_changes.then(|| {
                            WorkspaceEditClientCapabilities {
                                document_changes: Some(true),
                                ..Default::default()
                            }
                        }),
                        did_change_watched_files: watch.then(|| {
                            DidChangeWatchedFilesClientCapabilities {
                                dynamic_registration: Some(true),
                                ..Default::default()
                            }
                        }),
                        ..Default::default()
                    }
                }),
                ..Default::default()
            },
            initialization_options: options,
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

    fn code_action_with_diagnostics(
        &mut self,
        uri: Uri,
        diagnostics: Vec<Diagnostic>,
    ) -> Option<CodeActionResponse> {
        let params = CodeActionParams {
            text_document: TextDocumentIdentifier { uri },
            range: Range::default(),
            context: CodeActionContext {
                diagnostics,
                only: None,
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

    fn hover(&mut self, uri: &Uri, line: u32, character: u32) -> Option<Hover> {
        let params = json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character },
        });
        let id = self.request("textDocument/hover", params);
        let response = self.response(&id);
        serde_json::from_value(response.result.expect("hover result"))
            .expect("Option<Hover>")
    }

    fn prepare_rename(
        &mut self,
        uri: &Uri,
        line: u32,
        character: u32,
    ) -> Option<PrepareRenameResponse> {
        let params = json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character },
        });
        let id = self.request("textDocument/prepareRename", params);
        let response = self.response(&id);
        serde_json::from_value(response.result.expect("prepareRename result"))
            .expect("Option<PrepareRenameResponse>")
    }

    /// Send a rename and return the raw response (so a test can check error vs result).
    fn rename(
        &mut self,
        uri: &Uri,
        line: u32,
        character: u32,
        new_name: &str,
    ) -> Response {
        let params = json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character },
            "newName": new_name,
        });
        let id = self.request("textDocument/rename", params);
        self.response(&id)
    }

    fn document_diagnostic(&mut self, uri: &Uri) -> DocumentDiagnosticReport {
        let params = json!({ "textDocument": { "uri": uri } });
        let id = self.request("textDocument/diagnostic", params);
        let response = self.response(&id);
        serde_json::from_value(response.result.expect("diagnostic result"))
            .expect("DocumentDiagnosticReport")
    }

    fn workspace_diagnostic(&mut self) -> WorkspaceDiagnosticReport {
        let params = json!({ "previousResultIds": [] });
        let id = self.request("workspace/diagnostic", params);
        let response = self.response(&id);
        serde_json::from_value(response.result.expect("workspace diagnostic result"))
            .expect("WorkspaceDiagnosticReport")
    }

    /// Send an incremental (ranged) change replacing `range` with `text`.
    fn did_change_range(&self, uri: Uri, version: i32, range: Range, text: &str) {
        self.notify(
            "textDocument/didChange",
            DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier { uri, version },
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: Some(range),
                    range_length: None,
                    text: text.to_string(),
                }],
            },
        );
    }

    /// Receive the next server-to-client request, skipping notifications/responses.
    fn recv_request(&self) -> Request {
        loop {
            if let Message::Request(request) =
                self.conn().receiver.recv().expect("recv")
            {
                return request;
            }
        }
    }

    /// Confirm the server still processes messages (an unknown request gets an
    /// error response). Used after sending something the server should ignore.
    fn assert_alive(&mut self) {
        let id = self.request(UNHANDLED_METHOD, Value::Null);
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

// Regression for #323: when the client advertises the pull model, the server must not
// *also* push `publishDiagnostics`. A client that keeps the push and pull channels in
// separate collections (VS Code via `vscode-languageclient`) would otherwise list every
// diagnostic twice. Every other test launches a push-only client, so they pin that the
// push path still works; these pin the pull path stays silent across open/change/close.
#[test]
fn pull_capable_client_receives_no_push_on_open() {
    let dir = project(TRAILING);
    let (mut client, _init) = Client::launch_pull(None, false);
    let doc = file_uri(dir.path(), "x.yaml");
    client.did_open(doc.clone(), "a: 1 \n");
    // A pull request flushes the loop: any stray push from didOpen would arrive ahead
    // of this response. The response must still carry the violation (pull works).
    let id = client.request(
        "textDocument/diagnostic",
        json!({ "textDocument": { "uri": doc } }),
    );
    let (notifications, response) = client.notifications_until_response(&id);
    assert_no_diagnostics_push(&notifications);
    assert_eq!(
        pull_items(response).len(),
        1,
        "pull still reports the trailing space"
    );
}

#[test]
fn pull_capable_client_receives_no_push_on_change() {
    let dir = project(TRAILING);
    let (mut client, _init) = Client::launch_pull(None, false);
    let doc = file_uri(dir.path(), "x.yaml");
    client.did_open(doc.clone(), "a: 1\n");
    client.did_change(doc.clone(), "a: 1 \n");
    let id = client.request(
        "textDocument/diagnostic",
        json!({ "textDocument": { "uri": doc } }),
    );
    let (notifications, response) = client.notifications_until_response(&id);
    assert_no_diagnostics_push(&notifications);
    // The change introduced the trailing space; pull must reflect the new state, not the
    // clean text opened a moment ago.
    assert_eq!(
        pull_items(response).len(),
        1,
        "pull reflects the post-change buffer"
    );
}

#[test]
fn pull_capable_client_receives_no_clear_on_close() {
    let dir = project(TRAILING);
    let (mut client, _init) = Client::launch_pull(None, false);
    let doc = file_uri(dir.path(), "x.yaml");
    client.did_open(doc.clone(), "a: 1 \n");
    client.did_close(doc);
    // The didClose clear is a push too; probe with an unhandled request so any escaped
    // clear arrives before the probe's error response.
    let id = client.request(UNHANDLED_METHOD, Value::Null);
    let (notifications, response) = client.notifications_until_response(&id);
    assert_no_diagnostics_push(&notifications);
    assert!(response.error.is_some(), "the probe is still answered");
}

// A pull client's diagnostics are gated off, so after a config change it must be told to
// re-pull (LSP `workspace/diagnostic/refresh`) or it would keep showing diagnostics from
// the old config. Push clients re-push instead (covered by the *_relints tests above).
#[test]
fn pull_client_is_asked_to_refresh_after_config_change() {
    let dir = project(TRAILING);
    let (mut client, _init) = Client::launch_pull(Some(dir.path()), true);
    client.did_open(file_uri(dir.path(), "x.yaml"), "a: 1 \n");
    // Simulate the watched config file changing on disk.
    client.notify("workspace/didChangeWatchedFiles", json!({ "changes": [] }));
    // Flush with a probe; any refresh request / stray push arrives before its response.
    let id = client.request(UNHANDLED_METHOD, Value::Null);
    let (messages, response) = client.messages_until_response(&id);
    assert!(response.error.is_some(), "the probe is still answered");
    let refreshes = messages
        .iter()
        .filter(|message| {
            matches!(message, Message::Request(request)
                if request.method == "workspace/diagnostic/refresh")
        })
        .count();
    assert_eq!(
        refreshes, 1,
        "a pull client is asked to re-pull after a config change"
    );
    assert!(
        messages.iter().all(|message| {
            !matches!(message, Message::Notification(note)
                if note.method == "textDocument/publishDiagnostics")
        }),
        "the refresh replaces a push for a pull client, it does not accompany one"
    );
}

#[test]
fn repeated_config_changes_send_distinct_refresh_request_ids() {
    let dir = project(TRAILING);
    let (mut client, _init) = Client::launch_pull(Some(dir.path()), true);
    client.did_open(file_uri(dir.path(), "x.yaml"), "a: 1 \n");
    // Two changes before any response: their refresh requests are concurrently outstanding,
    // so reusing one id would break the client's response correlation (JSON-RPC).
    client.notify("workspace/didChangeWatchedFiles", json!({ "changes": [] }));
    client.notify("workspace/didChangeWatchedFiles", json!({ "changes": [] }));
    let id = client.request(UNHANDLED_METHOD, Value::Null);
    let (messages, response) = client.messages_until_response(&id);
    assert!(response.error.is_some(), "the probe is still answered");
    let refresh_ids: Vec<_> = messages
        .iter()
        .filter_map(|message| match message {
            Message::Request(request)
                if request.method == "workspace/diagnostic/refresh" =>
            {
                Some(request.id.clone())
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        refresh_ids.len(),
        2,
        "each config change asks for a refresh"
    );
    assert_ne!(
        refresh_ids[0], refresh_ids[1],
        "concurrent refreshes use distinct ids so the client can correlate responses"
    );
}

#[test]
fn pull_client_without_refresh_support_gets_no_refresh() {
    let dir = project(TRAILING);
    let (mut client, _init) = Client::launch_pull(Some(dir.path()), false);
    client.did_open(file_uri(dir.path(), "x.yaml"), "a: 1 \n");
    client.notify("workspace/didChangeWatchedFiles", json!({ "changes": [] }));
    let id = client.request(UNHANDLED_METHOD, Value::Null);
    let (messages, response) = client.messages_until_response(&id);
    assert!(response.error.is_some(), "the probe is still answered");
    assert!(
        messages.iter().all(|message| {
            !matches!(message, Message::Request(request)
                if request.method == "workspace/diagnostic/refresh")
        }),
        "no refresh is sent to a client that did not advertise support for it"
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
fn untitled_buffer_is_not_suppressed_by_path_ignores() {
    // An `ignore` glob that would match the synthetic untitled.yaml must not
    // suppress an unsaved buffer, which has no real path to filter on.
    let dir = project("ignore = [\"*.yaml\"]\n[rules]\ntrailing-spaces = \"enable\"\n");
    let (client, _init) = Client::launch_full(None, None, true, Some(dir.path()));
    client.did_open(uri("untitled:Untitled-5"), "a: 1 \n");
    assert_eq!(
        client.diagnostics().len(),
        1,
        "path-based ignores do not apply to an untitled buffer"
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
    let id = client.request(UNHANDLED_METHOD, Value::Null);
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

// --- #317 nice-to-haves: capabilities, watching, config, actions, hover, sync, rename,
//     and pull diagnostics. ---

#[test]
fn initialize_advertises_extended_capabilities() {
    let (_client, init) = Client::launch(None, None);
    let caps = init.capabilities;
    assert!(caps.hover_provider.is_some(), "advertises hover");
    assert!(caps.rename_provider.is_some(), "advertises rename");
    assert!(
        caps.diagnostic_provider.is_some(),
        "advertises pull diagnostics"
    );
    assert!(
        matches!(
            caps.text_document_sync,
            Some(lsp_types::TextDocumentSyncCapability::Kind(
                lsp_types::TextDocumentSyncKind::INCREMENTAL
            ))
        ),
        "negotiates incremental sync"
    );
}

#[test]
fn registers_config_watcher_when_the_client_supports_it() {
    let dir = project(TRAILING);
    let (client, _init) =
        Client::launch_with(None, Some(dir.path()), true, None, true, None);
    let request = client.recv_request();
    assert_eq!(request.method, "client/registerCapability");
    let params = request.params.to_string();
    assert!(
        params.contains("didChangeWatchedFiles"),
        "registers the watched-files capability: {params}"
    );
    assert!(
        params.contains("ryl.toml"),
        "watches ryl config files: {params}"
    );
}

#[test]
fn config_watcher_includes_an_explicit_config_path() {
    let root = tempdir().expect("tempdir");
    let config = root.path().join("custom.toml");
    std::fs::write(&config, TRAILING).expect("config");
    let config_path = config.display().to_string().replace('\\', "/");
    let options = json!({ "configPath": config_path });
    let (client, _init) =
        Client::launch_with(None, Some(root.path()), true, None, true, Some(options));
    let request = client.recv_request();
    assert_eq!(request.method, "client/registerCapability");
    let params = request.params.to_string();
    assert!(
        params.contains("custom.toml"),
        "an explicit configPath (non-standard name) is also watched: {params}"
    );
}

#[test]
fn watched_file_change_relints_open_documents() {
    // Start under a config that does not flag the trailing space.
    let dir = project("[rules]\nkey-duplicates = \"enable\"\n");
    let (client, _init) =
        Client::launch_with(None, Some(dir.path()), true, None, true, None);
    assert_eq!(
        client.recv_request().method,
        "client/registerCapability",
        "the watcher is registered first"
    );
    let doc = file_uri(dir.path(), "x.yaml");
    client.did_open(doc, "a: 1 \n");
    assert!(
        client.diagnostics().is_empty(),
        "clean under the initial config"
    );
    // Swap in a config that enables trailing-spaces, then signal the watcher.
    std::fs::write(dir.path().join(".ryl.toml"), TRAILING).expect("rewrite config");
    client.notify("workspace/didChangeWatchedFiles", json!({ "changes": [] }));
    assert_eq!(
        client.diagnostics().len(),
        1,
        "the new on-disk config re-lints the open document"
    );
}

#[test]
fn config_data_init_option_enables_linting() {
    // An empty workspace root with no project config: only the inline configData makes
    // trailing-spaces apply to the untitled buffer.
    let root = tempdir().expect("tempdir");
    let options = json!({ "configData": "rules:\n  trailing-spaces: enable\n" });
    let (client, _init) =
        Client::launch_with(None, Some(root.path()), true, None, false, Some(options));
    client.did_open(uri("untitled:Untitled-cfg"), "a: 1 \n");
    assert_eq!(
        client.diagnostics().len(),
        1,
        "inline configData enables the rule for an otherwise config-less buffer"
    );
}

#[test]
fn enable_false_turns_linting_off() {
    let dir = project(TRAILING);
    let options = json!({ "enable": false });
    let (mut client, _init) =
        Client::launch_with(None, Some(dir.path()), true, None, false, Some(options));
    let doc = file_uri(dir.path(), "x.yaml");
    client.did_open(doc.clone(), "a: 1 \n");
    assert!(
        client.diagnostics().is_empty(),
        "ryl is turned off via enable=false"
    );
    assert!(client.code_action(doc).is_none(), "and offers no actions");
}

#[test]
fn did_change_configuration_updates_settings_and_relints() {
    let dir = project(TRAILING);
    let options = json!({ "enable": false });
    let (client, _init) =
        Client::launch_with(None, Some(dir.path()), true, None, false, Some(options));
    let doc = file_uri(dir.path(), "x.yaml");
    client.did_open(doc, "a: 1 \n");
    assert!(client.diagnostics().is_empty(), "disabled at start");
    client.notify(
        "workspace/didChangeConfiguration",
        json!({ "settings": { "ryl": { "enable": true } } }),
    );
    assert_eq!(
        client.diagnostics().len(),
        1,
        "re-enabling via didChangeConfiguration re-lints open docs"
    );
}

#[test]
fn hover_reports_the_rule_and_docs_link() {
    let dir = project(TRAILING);
    let (mut client, _init) = Client::launch(None, None);
    let doc = file_uri(dir.path(), "x.yaml");
    client.did_open(doc.clone(), "a: 1 \n");
    let _ = client.diagnostics();
    // The trailing space is column 5 (1-based) -> 0-based character 4.
    let hover = client
        .hover(&doc, 0, 4)
        .expect("hovering the flagged position");
    let HoverContents::Markup(markup) = hover.contents else {
        panic!("expected markup contents");
    };
    assert!(markup.value.contains("trailing-spaces"), "names the rule");
    assert!(
        markup.value.contains("ryl-docs.pages.dev/rules"),
        "links to the rules reference"
    );
}

#[test]
fn hover_off_a_diagnostic_is_null() {
    let dir = project(TRAILING);
    let (mut client, _init) = Client::launch(None, None);
    let doc = file_uri(dir.path(), "x.yaml");
    client.did_open(doc.clone(), "a: 1 \n");
    let _ = client.diagnostics();
    assert!(
        client.hover(&doc, 0, 0).is_none(),
        "no diagnostic covers the start of the line"
    );
}

#[test]
fn code_action_offers_disable_and_per_rule_fixes() {
    let dir = project(TRAILING);
    let (mut client, _init) = Client::launch(None, None);
    let doc = file_uri(dir.path(), "x.yaml");
    client.did_open(doc.clone(), "a: 1 \n");
    let _ = client.diagnostics();
    let actions = client
        .code_action_with_diagnostics(doc, vec![diag("trailing-spaces", 0, 4)])
        .expect("a flagged document offers actions");
    let titles: Vec<&str> = actions
        .iter()
        .filter_map(|action| match action {
            CodeActionOrCommand::CodeAction(action) => Some(action.title.as_str()),
            CodeActionOrCommand::Command(_) => None,
        })
        .collect();
    assert!(
        titles.contains(&"Fix all ryl problems"),
        "fix-all: {titles:?}"
    );
    assert!(
        titles.contains(&"Fix all trailing-spaces problems"),
        "per-rule fix-all: {titles:?}"
    );
    assert!(
        titles.contains(&"Disable trailing-spaces for this line"),
        "disable-line: {titles:?}"
    );
    assert!(
        titles.contains(&"Disable ryl for this file"),
        "disable-file: {titles:?}"
    );
}

#[test]
fn disable_line_action_inserts_a_directive_above_the_line() {
    let dir = project(TRAILING);
    let (mut client, _init) = Client::launch(None, None);
    let doc = file_uri(dir.path(), "x.yaml");
    client.did_open(doc.clone(), "a: 1 \n");
    let _ = client.diagnostics();
    let actions = client
        .code_action_with_diagnostics(doc, vec![diag("trailing-spaces", 0, 4)])
        .expect("actions offered");
    let action = actions
        .iter()
        .find_map(|action| match action {
            CodeActionOrCommand::CodeAction(action)
                if action.title == "Disable trailing-spaces for this line" =>
            {
                Some(action)
            }
            _ => None,
        })
        .expect("the disable-line action is present");
    let Some(DocumentChanges::Edits(edits)) = action
        .edit
        .as_ref()
        .and_then(|edit| edit.document_changes.as_ref())
    else {
        panic!("expected a versioned edit");
    };
    let OneOf::Left(text_edit) = &edits[0].edits[0] else {
        panic!("expected a plain TextEdit");
    };
    assert_eq!(
        text_edit.new_text, "# ryl disable-line rule:trailing-spaces\n",
        "inserts the directive on its own line"
    );
    assert_eq!(
        (text_edit.range.start.line, text_edit.range.start.character),
        (0, 0),
        "above the diagnostic's line"
    );
}

#[test]
fn incremental_change_patches_the_document() {
    let dir = project(TRAILING);
    let (client, _init) = Client::launch(None, None);
    let doc = file_uri(dir.path(), "x.yaml");
    client.did_open(doc.clone(), "a: 1\n");
    assert!(client.diagnostics().is_empty(), "clean on open");
    // Insert a trailing space at the end of the first line's content.
    let at_eol = Range::new(Position::new(0, 4), Position::new(0, 4));
    client.did_change_range(doc, 2, at_eol, " ");
    assert_eq!(
        client.diagnostics().len(),
        1,
        "the incrementally inserted space is flagged"
    );
}

#[test]
fn rename_rewrites_an_anchor_and_its_aliases() {
    let dir = project(TRAILING);
    let (mut client, _init) = Client::launch(None, None);
    let doc = file_uri(dir.path(), "x.yaml");
    client.did_open(doc.clone(), "a: &anchor 1\nb: *anchor\n");
    let _ = client.diagnostics();
    let prepared = client
        .prepare_rename(&doc, 0, 6)
        .expect("the anchor name is renameable");
    let PrepareRenameResponse::RangeWithPlaceholder { placeholder, .. } = prepared
    else {
        panic!("expected a range + placeholder");
    };
    assert_eq!(placeholder, "anchor");
    let response = client.rename(&doc, 0, 6, "renamed");
    let edit: WorkspaceEdit =
        serde_json::from_value(response.result.expect("rename produces an edit"))
            .expect("workspace edit");
    let Some(DocumentChanges::Edits(edits)) = edit.document_changes else {
        panic!("expected versioned document changes");
    };
    assert_eq!(
        edits[0].edits.len(),
        2,
        "the anchor and its alias are renamed"
    );
}

#[test]
fn rename_rejects_an_illegal_name() {
    let dir = project(TRAILING);
    let (mut client, _init) = Client::launch(None, None);
    let doc = file_uri(dir.path(), "x.yaml");
    client.did_open(doc.clone(), "a: &anchor 1\n");
    let _ = client.diagnostics();
    let response = client.rename(&doc, 0, 6, "bad name");
    assert!(response.error.is_some(), "a name with a space is rejected");
}

#[test]
fn rename_off_an_anchor_is_null() {
    let dir = project(TRAILING);
    let (mut client, _init) = Client::launch(None, None);
    let doc = file_uri(dir.path(), "x.yaml");
    client.did_open(doc.clone(), "a: 1\n");
    let _ = client.diagnostics();
    let response = client.rename(&doc, 0, 0, "x");
    assert_eq!(response.result, Some(Value::Null), "nothing to rename here");
    assert!(response.error.is_none());
}

#[test]
fn code_action_offers_no_disable_actions_for_markdown() {
    let dir = project(
        "[files]\nmarkdown = [\"*.md\"]\n[rules]\ntrailing-spaces = \"enable\"\n",
    );
    let (mut client, _init) = Client::launch(None, None);
    let doc = file_uri(dir.path(), "x.md");
    client.did_open(doc.clone(), "```yaml\na: 1 \n```\n");
    let _ = client.diagnostics();
    // The trailing space is on the embedded-YAML line; a disable comment can't be inserted
    // reliably into a Markdown host, so only the (region-aware) fix-all is offered.
    let actions = client
        .code_action_with_diagnostics(doc, vec![diag("trailing-spaces", 1, 4)])
        .expect("fix-all is offered for markdown");
    let titles: Vec<&str> = actions
        .iter()
        .filter_map(|action| match action {
            CodeActionOrCommand::CodeAction(action) => Some(action.title.as_str()),
            CodeActionOrCommand::Command(_) => None,
        })
        .collect();
    assert!(titles.contains(&"Fix all ryl problems"), "{titles:?}");
    assert!(
        !titles.iter().any(|title| title.starts_with("Disable")),
        "no disable actions are offered for a markdown document: {titles:?}"
    );
}

#[test]
fn rename_is_disabled_for_markdown_documents() {
    let dir = project(
        "[files]\nmarkdown = [\"*.md\"]\n[rules]\ntrailing-spaces = \"enable\"\n",
    );
    let (mut client, _init) = Client::launch(None, None);
    let doc = file_uri(dir.path(), "x.md");
    client.did_open(doc.clone(), "```yaml\na: &anchor 1\n```\n");
    let _ = client.diagnostics();
    assert!(
        client.prepare_rename(&doc, 1, 6).is_none(),
        "rename targets YAML documents, not markdown hosts"
    );
}

#[test]
fn document_diagnostic_pull_reports_open_and_disk_files() {
    let dir = project(TRAILING);
    let (mut client, _init) = Client::launch(None, None);
    let open = file_uri(dir.path(), "open.yaml");
    client.did_open(open.clone(), "a: 1 \n");
    let _ = client.diagnostics();
    let DocumentDiagnosticReport::Full(report) = client.document_diagnostic(&open)
    else {
        panic!("expected a full report");
    };
    assert_eq!(
        report.full_document_diagnostic_report.items.len(),
        1,
        "the open buffer is linted on pull"
    );
    // A file only on disk is read and linted too.
    std::fs::write(dir.path().join("disk.yaml"), "b: 2 \n").expect("write disk file");
    let on_disk = file_uri(dir.path(), "disk.yaml");
    let DocumentDiagnosticReport::Full(report) = client.document_diagnostic(&on_disk)
    else {
        panic!("expected a full report");
    };
    assert_eq!(
        report.full_document_diagnostic_report.items.len(),
        1,
        "a closed file is read from disk and linted"
    );
}

#[test]
fn document_diagnostic_pull_is_empty_for_unreadable_targets() {
    let dir = project(TRAILING);
    let (mut client, _init) = Client::launch(None, None);
    let missing = file_uri(dir.path(), "nope.yaml");
    let DocumentDiagnosticReport::Full(report) = client.document_diagnostic(&missing)
    else {
        panic!("expected a full report");
    };
    assert!(
        report.full_document_diagnostic_report.items.is_empty(),
        "a missing file yields no diagnostics"
    );
    let DocumentDiagnosticReport::Full(report) =
        client.document_diagnostic(&uri("untitled:none"))
    else {
        panic!("expected a full report");
    };
    assert!(
        report.full_document_diagnostic_report.items.is_empty(),
        "an unopened untitled buffer has no path to read"
    );
}

#[test]
fn workspace_diagnostic_pull_reports_repo_files() {
    let dir = project(TRAILING);
    std::fs::write(dir.path().join("bad.yaml"), "a: 1 \n").expect("write bad");
    std::fs::write(dir.path().join("good.yaml"), "a: 1\n").expect("write good");
    let (mut client, _init) =
        Client::launch_with(None, Some(dir.path()), true, None, false, None);
    let report = client.workspace_diagnostic();
    let bad = report
        .items
        .iter()
        .find_map(|item| match item {
            WorkspaceDocumentDiagnosticReport::Full(full)
                if full.uri.as_str().ends_with("bad.yaml") =>
            {
                Some(full)
            }
            _ => None,
        })
        .expect("bad.yaml is reported");
    assert_eq!(
        bad.full_document_diagnostic_report.items.len(),
        1,
        "the trailing space in bad.yaml is reported"
    );
}

#[test]
fn code_action_ignores_a_foreign_servers_diagnostics() {
    let dir = project(TRAILING);
    let (mut client, _init) = Client::launch(None, None);
    let doc = file_uri(dir.path(), "x.yaml");
    client.did_open(doc.clone(), "a: 1 \n"); // a real trailing space -> fix-all available
    let _ = client.diagnostics();
    // A diagnostic from a coexisting server (e.g. yaml-language-server) must not produce
    // ryl disable/per-rule actions or a "disable ryl" action.
    let foreign = Diagnostic {
        range: Range::new(Position::new(0, 0), Position::new(0, 1)),
        code: Some(NumberOrString::String("schema-error".to_string())),
        source: Some("yaml-language-server".to_string()),
        ..Default::default()
    };
    let actions = client
        .code_action_with_diagnostics(doc, vec![foreign])
        .expect("the whole-file fix-all is still offered from the text");
    let titles: Vec<&str> = actions
        .iter()
        .filter_map(|action| match action {
            CodeActionOrCommand::CodeAction(action) => Some(action.title.as_str()),
            CodeActionOrCommand::Command(_) => None,
        })
        .collect();
    assert!(titles.contains(&"Fix all ryl problems"), "{titles:?}");
    assert!(
        !titles.iter().any(|title| title.starts_with("Disable")),
        "no disable actions for a foreign diagnostic: {titles:?}"
    );
}

#[test]
fn document_diagnostic_pull_surfaces_a_config_error() {
    let dir = project("this is not valid toml = =\n");
    let (mut client, _init) = Client::launch(None, None);
    let doc = file_uri(dir.path(), "x.yaml");
    client.did_open(doc.clone(), "a: 1\n");
    let _ = client.diagnostics();
    let DocumentDiagnosticReport::Full(report) = client.document_diagnostic(&doc)
    else {
        panic!("expected a full report");
    };
    let items = report.full_document_diagnostic_report.items;
    assert_eq!(items.len(), 1, "the config error surfaces as a diagnostic");
    assert!(
        items[0].message.contains("configuration error"),
        "{:?}",
        items[0].message
    );
}

#[test]
fn workspace_diagnostic_pull_surfaces_a_config_error() {
    let dir = project("this is not valid toml = =\n");
    std::fs::write(dir.path().join("a.yaml"), "x: 1\n").expect("write");
    let (mut client, _init) =
        Client::launch_with(None, Some(dir.path()), true, None, false, None);
    let report = client.workspace_diagnostic();
    let surfaced = report.items.iter().any(|item| match item {
        WorkspaceDocumentDiagnosticReport::Full(full) => full
            .full_document_diagnostic_report
            .items
            .iter()
            .any(|d| d.message.contains("configuration error")),
        WorkspaceDocumentDiagnosticReport::Unchanged(_) => false,
    });
    assert!(
        surfaced,
        "a malformed config surfaces as an error diagnostic, not a silent clean report"
    );
}

#[test]
fn workspace_diagnostic_pull_spans_multiple_roots_and_dedups() {
    let dir = project(TRAILING);
    std::fs::write(dir.path().join("bad.yaml"), "a: 1 \n").expect("write");
    // The same directory advertised as two roots: every file is walked twice, so the
    // report must de-duplicate it to one entry.
    let (mut client, _init) = Client::launch_roots(&[dir.path(), dir.path()]);
    let report = client.workspace_diagnostic();
    let bad_reports = report
        .items
        .iter()
        .filter(|item| {
            matches!(
                item,
                WorkspaceDocumentDiagnosticReport::Full(full)
                    if full.uri.as_str().ends_with("bad.yaml")
            )
        })
        .count();
    assert_eq!(
        bad_reports, 1,
        "a file reachable from two roots is reported once"
    );
}

#[test]
fn workspace_diagnostic_can_be_cancelled() {
    let dir = project(TRAILING);
    std::fs::write(dir.path().join("a.yaml"), "a: 1 \n").expect("write");
    let (mut client, _init) =
        Client::launch_with(None, Some(dir.path()), true, None, false, None);
    let id = client.request("workspace/diagnostic", json!({ "previousResultIds": [] }));
    // Cancel by id. Depending on the race the worker answers with the report or a
    // RequestCancelled error, but the request is always answered (the loop never hangs).
    client.notify("$/cancelRequest", json!({ "id": client.next_id }));
    let response = client.response(&id);
    assert!(
        response.result.is_some() || response.error.is_some(),
        "a cancelled workspace pull is always answered"
    );
}

#[test]
fn cancel_request_for_unknown_or_malformed_id_is_ignored() {
    let dir = project(TRAILING);
    std::fs::write(dir.path().join("a.yaml"), "a: 1 \n").expect("write");
    let (mut client, _init) =
        Client::launch_with(None, Some(dir.path()), true, None, false, None);
    let id = client.request("workspace/diagnostic", json!({ "previousResultIds": [] }));
    // A string id matches no in-flight scan; a malformed params is ignored entirely.
    client.notify("$/cancelRequest", json!({ "id": "no-such-request" }));
    client.notify("$/cancelRequest", json!("not the params shape"));
    let _ = client.response(&id); // the pull still completes
    client.assert_alive();
}

#[test]
fn workspace_diagnostic_reaps_finished_workers() {
    let dir = project(TRAILING);
    std::fs::write(dir.path().join("a.yaml"), "a: 1\n").expect("write");
    let (mut client, _init) =
        Client::launch_with(None, Some(dir.path()), true, None, false, None);
    // Two sequential pulls: the second's spawn reaps the first's finished worker.
    let _ = client.workspace_diagnostic();
    let _ = client.workspace_diagnostic();
    client.assert_alive();
}

#[test]
fn workspace_diagnostic_without_a_root_is_empty() {
    let (mut client, _init) = Client::launch(None, None);
    assert!(
        client.workspace_diagnostic().items.is_empty(),
        "without a workspace root there is nothing to scan"
    );
}

#[test]
fn workspace_diagnostic_pull_handles_open_ignored_and_unreadable_files() {
    let dir =
        project("ignore = [\"skip.yaml\"]\n[rules]\ntrailing-spaces = \"enable\"\n");
    std::fs::write(dir.path().join("bad.yaml"), "a: 1 \n").expect("bad");
    std::fs::write(dir.path().join("skip.yaml"), "a: 1 \n").expect("ignored");
    // A UTF-16 LE BOM followed by an odd byte count cannot be decoded.
    std::fs::write(dir.path().join("binary.yaml"), [0xFF, 0xFE, 0x41])
        .expect("undecodable");
    let (mut client, _init) =
        Client::launch_with(None, Some(dir.path()), true, None, false, None);
    // Open one walked file (so the open-buffer text is used) and an untitled buffer (a
    // non-file URI the open map skips).
    client.did_open(file_uri(dir.path(), "bad.yaml"), "a: 1 \n");
    let _ = client.diagnostics();
    client.did_open(uri("untitled:ws"), "a: 1\n");
    let _ = client.diagnostics();
    let report = client.workspace_diagnostic();
    let paths: Vec<&str> = report
        .items
        .iter()
        .map(|item| match item {
            WorkspaceDocumentDiagnosticReport::Full(full) => full.uri.as_str(),
            WorkspaceDocumentDiagnosticReport::Unchanged(unchanged) => {
                unchanged.uri.as_str()
            }
        })
        .collect();
    assert!(
        paths.iter().any(|path| path.ends_with("bad.yaml")),
        "the open walked file is reported: {paths:?}"
    );
    assert!(
        !paths.iter().any(|path| path.ends_with("skip.yaml")),
        "the config-ignored file is skipped: {paths:?}"
    );
    assert!(
        !paths.iter().any(|path| path.ends_with("binary.yaml")),
        "the undecodable file is skipped: {paths:?}"
    );
}

#[test]
fn config_path_init_option_points_at_a_config_file() {
    let root = tempdir().expect("tempdir");
    let config = root.path().join("custom.toml");
    std::fs::write(&config, TRAILING).expect("write config");
    let config_path = config.display().to_string().replace('\\', "/");
    let options = json!({ "configPath": config_path });
    let (client, _init) =
        Client::launch_with(None, Some(root.path()), true, None, false, Some(options));
    client.did_open(uri("untitled:cfg-path"), "a: 1 \n");
    assert_eq!(
        client.diagnostics().len(),
        1,
        "configPath supplies the rules for an otherwise config-less buffer"
    );
}

#[test]
fn malformed_did_change_configuration_is_ignored() {
    let (mut client, _init) = Client::launch(None, None);
    client.notify(
        "workspace/didChangeConfiguration",
        Value::String("bad".to_string()),
    );
    client.assert_alive();
}

#[test]
fn incremental_change_ignores_a_reversed_range() {
    let dir = project(TRAILING);
    let (client, _init) = Client::launch(None, None);
    let doc = file_uri(dir.path(), "x.yaml");
    client.did_open(doc.clone(), "a: 1\n");
    assert!(client.diagnostics().is_empty(), "clean on open");
    // A reversed range (start after end) is skipped, leaving the document unchanged.
    let reversed = Range::new(Position::new(0, 4), Position::new(0, 0));
    client.did_change_range(doc, 2, reversed, " ");
    assert!(
        client.diagnostics().is_empty(),
        "a reversed-range edit is ignored, so no problem appears"
    );
}

#[test]
fn formatting_with_unresolvable_config_is_null() {
    let dir = project("this is not valid toml = =\n");
    let (mut client, _init) = Client::launch(None, None);
    let doc = file_uri(dir.path(), "x.yaml");
    client.did_open(doc.clone(), "a: 1 \n");
    let _ = client.diagnostics();
    assert!(
        client.formatting(doc).is_none(),
        "no resolvable config -> nothing to format"
    );
}

#[test]
fn prepare_rename_on_an_unopened_document_is_null() {
    let dir = project(TRAILING);
    let (mut client, _init) = Client::launch(None, None);
    assert!(
        client
            .prepare_rename(&file_uri(dir.path(), "never.yaml"), 0, 0)
            .is_none(),
        "an unopened document has no buffer to rename in"
    );
}

#[test]
fn malformed_rename_params_yield_a_null_result() {
    let (mut client, _init) = Client::launch(None, None);
    let id = client.request("textDocument/rename", Value::String("bad".to_string()));
    let response = client.response(&id);
    assert_eq!(
        response.result,
        Some(Value::Null),
        "bad rename params -> null"
    );
}

#[test]
fn rename_on_an_unopened_document_is_null() {
    let dir = project(TRAILING);
    let (mut client, _init) = Client::launch(None, None);
    let response = client.rename(&file_uri(dir.path(), "never.yaml"), 0, 0, "x");
    assert_eq!(
        response.result,
        Some(Value::Null),
        "an unopened document yields no rename edit"
    );
}

#[test]
fn rename_on_a_markdown_document_is_null() {
    let dir = project(
        "[files]\nmarkdown = [\"*.md\"]\n[rules]\ntrailing-spaces = \"enable\"\n",
    );
    let (mut client, _init) = Client::launch(None, None);
    let doc = file_uri(dir.path(), "x.md");
    client.did_open(doc.clone(), "```yaml\na: &anchor 1\n```\n");
    let _ = client.diagnostics();
    let response = client.rename(&doc, 1, 6, "renamed");
    assert_eq!(
        response.result,
        Some(Value::Null),
        "rename targets YAML documents, not markdown hosts"
    );
}

#[test]
fn code_action_handles_edge_case_diagnostics() {
    let dir = project(TRAILING);
    let (mut client, _init) = Client::launch(None, None);
    let doc = file_uri(dir.path(), "x.yaml");
    client.did_open(doc.clone(), "a: 1 \n");
    let _ = client.diagnostics();
    let mut rule_less = diag("trailing-spaces", 0, 4);
    rule_less.code = None; // a syntax-style diagnostic the client may echo back
    let diagnostics = vec![
        diag("trailing-spaces", 0, 4),
        diag("trailing-spaces", 0, 4), // duplicate (rule, line) -> deduped
        diag("commas", 0, 0),          // fixable, but no comma fix applies here
        diag("trailing-spaces", 99, 0), // line past the document -> no disable action
        diag("not-a-real-rule", 0, 0), // ryl-sourced but unknown rule id -> skipped
        rule_less,                     // no rule code -> skipped
    ];
    let actions = client
        .code_action_with_diagnostics(doc, diagnostics)
        .expect("at least the fix-all action is offered");
    let titles: Vec<&str> = actions
        .iter()
        .filter_map(|action| match action {
            CodeActionOrCommand::CodeAction(action) => Some(action.title.as_str()),
            CodeActionOrCommand::Command(_) => None,
        })
        .collect();
    assert!(
        !titles.contains(&"Fix all commas problems"),
        "no per-rule action when that rule changes nothing: {titles:?}"
    );
    assert!(
        titles.contains(&"Fix all trailing-spaces problems"),
        "the rule that does have a fix is offered: {titles:?}"
    );
    let trailing_disables = titles
        .iter()
        .filter(|title| title.starts_with("Disable trailing-spaces"))
        .count();
    assert_eq!(
        trailing_disables, 1,
        "the duplicate is deduped and the out-of-range line is dropped: {titles:?}"
    );
}

#[test]
fn disable_line_is_not_offered_inside_a_block_scalar() {
    let dir = project(TRAILING);
    let (mut client, _init) = Client::launch(None, None);
    let doc = file_uri(dir.path(), "x.yaml");
    // The trailing space on line 2 (0-based) is inside a literal block scalar, where a
    // `# ryl disable-line` insert would be scalar content rather than a directive.
    client.did_open(doc.clone(), "key: |\n  aaa\n  bbb \nmore: 1\n");
    let _ = client.diagnostics();
    let actions = client
        .code_action_with_diagnostics(doc, vec![diag("trailing-spaces", 2, 7)])
        .expect("disable-file is still offered");
    let titles: Vec<&str> = actions
        .iter()
        .filter_map(|action| match action {
            CodeActionOrCommand::CodeAction(action) => Some(action.title.as_str()),
            CodeActionOrCommand::Command(_) => None,
        })
        .collect();
    assert!(
        !titles
            .iter()
            .any(|title| title.starts_with("Disable trailing-spaces for this line")),
        "no disable-line action inside a block scalar: {titles:?}"
    );
    assert!(
        titles.contains(&"Disable ryl for this file"),
        "the whole-file disable is still offered: {titles:?}"
    );
}

#[test]
fn disable_line_is_not_offered_inside_a_multiline_quoted_scalar() {
    let dir = project(TRAILING);
    let (mut client, _init) = Client::launch(None, None);
    let doc = file_uri(dir.path(), "x.yaml");
    // A double-quoted scalar continued onto line 2 (0-based 1): a disable-line insert there
    // would land inside the still-open quote and corrupt the value, so it is not offered.
    client.did_open(doc.clone(), "key: \"first\n  second\"\n");
    let _ = client.diagnostics();
    let actions = client
        .code_action_with_diagnostics(doc, vec![diag("trailing-spaces", 1, 2)])
        .expect("disable-file is still offered");
    let titles: Vec<&str> = actions
        .iter()
        .filter_map(|action| match action {
            CodeActionOrCommand::CodeAction(action) => Some(action.title.as_str()),
            CodeActionOrCommand::Command(_) => None,
        })
        .collect();
    assert!(
        !titles
            .iter()
            .any(|title| title.starts_with("Disable trailing-spaces for this line")),
        "no disable-line inside a multiline quoted scalar: {titles:?}"
    );
}

#[test]
fn disable_line_is_suppressed_when_the_document_cannot_parse() {
    let dir = project(TRAILING);
    let (mut client, _init) = Client::launch(None, None);
    let doc = file_uri(dir.path(), "x.yaml");
    // The undefined alias makes the document unparsable, so block-scalar detection fails
    // and a disable-line could land in the literal block's content. Suppress it (but the
    // line-0 disable-file prepend is always safe).
    client.did_open(doc.clone(), "a: |\n  text \nb: *undefined\n");
    let _ = client.diagnostics();
    let actions = client
        .code_action_with_diagnostics(doc, vec![diag("trailing-spaces", 1, 6)])
        .expect("disable-file is still offered");
    let titles: Vec<&str> = actions
        .iter()
        .filter_map(|action| match action {
            CodeActionOrCommand::CodeAction(action) => Some(action.title.as_str()),
            CodeActionOrCommand::Command(_) => None,
        })
        .collect();
    assert!(
        !titles
            .iter()
            .any(|title| title.starts_with("Disable trailing-spaces for this line")),
        "no disable-line when block-scalar detection cannot run: {titles:?}"
    );
    assert!(
        titles.contains(&"Disable ryl for this file"),
        "the line-0 disable-file is still offered: {titles:?}"
    );
}

#[test]
fn rename_respects_enable_false() {
    let dir = project(TRAILING);
    let options = json!({ "enable": false });
    let (mut client, _init) =
        Client::launch_with(None, Some(dir.path()), true, None, false, Some(options));
    let doc = file_uri(dir.path(), "x.yaml");
    client.did_open(doc.clone(), "a: &anchor 1\n");
    let _ = client.diagnostics();
    assert!(
        client.prepare_rename(&doc, 0, 6).is_none(),
        "rename is off when ryl is disabled"
    );
}

#[test]
fn prepare_rename_with_unresolvable_config_is_null() {
    let dir = project("this is not valid toml = =\n");
    let (mut client, _init) = Client::launch(None, None);
    let doc = file_uri(dir.path(), "x.yaml");
    client.did_open(doc.clone(), "a: &anchor 1\n");
    let _ = client.diagnostics();
    assert!(
        client.prepare_rename(&doc, 0, 6).is_none(),
        "a broken config disables rename too"
    );
}

#[test]
fn rename_works_on_an_untitled_yaml_buffer() {
    let dir = project(TRAILING);
    let (mut client, _init) =
        Client::launch_with(None, Some(dir.path()), true, None, false, None);
    let doc = uri("untitled:anchors");
    client.did_open(doc.clone(), "a: &anchor 1\nb: *anchor\n");
    let _ = client.diagnostics();
    assert!(
        client.prepare_rename(&doc, 0, 6).is_some(),
        "an untitled YAML buffer's anchors are renameable"
    );
}

#[test]
fn prepare_rename_on_an_ignored_file_is_null() {
    let dir =
        project("ignore = [\"skip.yaml\"]\n[rules]\ntrailing-spaces = \"enable\"\n");
    let (mut client, _init) = Client::launch(None, None);
    let doc = file_uri(dir.path(), "skip.yaml");
    client.did_open(doc.clone(), "a: &anchor 1\n");
    let _ = client.diagnostics();
    assert!(
        client.prepare_rename(&doc, 0, 6).is_none(),
        "a config-ignored file is not renameable"
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
