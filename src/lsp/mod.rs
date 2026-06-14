//! The `ryl server` language server: a thin synchronous protocol adapter over
//! ryl's existing lint and fix engine, built on `lsp-server` + `lsp-types` (the
//! rust-analyzer / Ruff stack). It provides diagnostics, a `source.fixAll.ryl`
//! code action, and `textDocument/formatting` (= apply safe fixes). Schema
//! validation, completion, and hover are intentionally left to Red Hat's
//! `yaml-language-server`, which ryl coexists with.
//!
//! Handling is graceful throughout: a malformed `initialize` (client-controlled)
//! ends the session cleanly, unknown requests get a `MethodNotFound` response, and
//! the loop ends when the client drops the connection or completes the
//! shutdown/exit handshake. The only `expect` is on serialising ryl's own
//! capabilities, which cannot fail.

pub mod analysis;
pub mod encoding;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::ExitCode;

use lsp_server::{
    Connection, ErrorCode, Message, Notification, Request, RequestId, Response,
};
use lsp_types::{
    CodeAction, CodeActionContext, CodeActionKind, CodeActionOrCommand,
    CodeActionParams, CodeActionProviderCapability, CodeActionResponse,
    DidChangeTextDocumentParams, DidCloseTextDocumentParams, DidOpenTextDocumentParams,
    DocumentChanges, DocumentFormattingParams, InitializeParams, InitializeResult,
    MessageType, OneOf, OptionalVersionedTextDocumentIdentifier,
    PublishDiagnosticsParams, ServerCapabilities, ServerInfo, ShowMessageParams,
    TextDocumentEdit, TextDocumentSyncCapability, TextDocumentSyncKind, TextEdit, Uri,
    WorkspaceEdit,
};

use crate::config::{ConfigContext, Overrides, SourceKind, discover_config};
use crate::lsp::encoding::{PositionEncoding, negotiate, uri_to_path};

/// The code-action kind ryl offers: a whole-file safe-fix, usable for
/// `editor.codeActionsOnSave` (a `source.fixAll` request subsumes it).
const FIX_ALL_KIND: &str = "source.fixAll.ryl";

/// How a session ended, mapped to a process exit code by [`run`]. Per the LSP
/// spec an `exit` notification *without* a prior `shutdown` is abnormal (exit 1);
/// every other ending (clean shutdown, dropped connection, rejected handshake) is
/// a normal exit 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionOutcome {
    Clean,
    Abnormal,
}

/// Run the language server over stdio until the client disconnects or completes
/// the shutdown/exit handshake, returning the process exit code.
///
/// # Panics
/// Panics only if the stdio reader/writer threads fail to join, which a working
/// transport never triggers. Malformed client input ends the session cleanly.
#[must_use]
pub fn run() -> ExitCode {
    let (connection, io_threads) = Connection::stdio();
    let outcome = serve(&connection);
    // Drop our connection handle so the outgoing channel closes; without this the
    // stdio writer thread never finishes and `io_threads.join()` would hang.
    drop(connection);
    io_threads
        .join()
        .expect("LSP stdio reader/writer threads should join cleanly");
    match outcome {
        SessionOutcome::Clean => ExitCode::SUCCESS,
        SessionOutcome::Abnormal => ExitCode::from(1),
    }
}

/// Drive the protocol over an established connection: the `initialize` handshake
/// then the message loop. Separated from [`run`] so it works over any
/// [`Connection`] — `run` wires stdio, tests use an in-process
/// `Connection::memory()` pair. The caller owns the connection and must drop it
/// after this returns so a stdio writer thread can finish (see [`run`]). A
/// malformed `initialize` is rejected with an error response and returns without
/// entering the loop.
///
/// # Panics
/// Panics only if serialising ryl's own server capabilities fails, which cannot
/// happen. Malformed client input ends the session cleanly instead.
#[must_use]
pub fn serve(connection: &Connection) -> SessionOutcome {
    // The initialize request and its params are client-controlled, so a malformed
    // one ends the session cleanly rather than panicking.
    let Ok((id, raw_params)) = connection.initialize_start() else {
        return SessionOutcome::Clean;
    };
    let params: InitializeParams = match serde_json::from_value(raw_params) {
        Ok(params) => params,
        Err(error) => {
            // Reject the handshake, then keep draining until the client ends the
            // session with `exit` or by closing the connection. Returning here
            // instead would leave a stdio client's reader thread blocked, hanging
            // run()'s io_threads.join().
            send(
                connection,
                Message::Response(Response::new_err(
                    id,
                    ErrorCode::InvalidParams as i32,
                    format!("invalid initialize params: {error}"),
                )),
            );
            return drain_until_session_end(connection);
        }
    };
    let encoding = negotiate(
        params
            .capabilities
            .general
            .as_ref()
            .and_then(|general| general.position_encodings.as_deref()),
    );
    let result = InitializeResult {
        capabilities: server_capabilities(encoding),
        server_info: Some(ServerInfo {
            name: "ryl".to_string(),
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
        }),
    };
    // `to_value` of our own capabilities can't fail; a transport error finishing
    // the handshake just means the client is gone, so the loop below ends at once.
    let result =
        serde_json::to_value(result).expect("server capabilities always serialize");
    let _ = connection.initialize_finish(id, result);

    let server = Server {
        encoding,
        // Anchors config discovery for untitled (non-file) buffers. Prefer the
        // first workspace folder; fall back to the LSP-3.17-deprecated `root_uri`
        // for older clients that send only it (hence the scoped allow).
        root: params
            .workspace_folders
            .as_ref()
            .and_then(|folders| folders.first())
            .and_then(|folder| uri_to_path(folder.uri.as_str()))
            .or_else(|| {
                #[allow(deprecated)]
                params
                    .root_uri
                    .as_ref()
                    .and_then(|uri| uri_to_path(uri.as_str()))
            }),
        supports_document_changes: params
            .capabilities
            .workspace
            .as_ref()
            .and_then(|workspace| workspace.workspace_edit.as_ref())
            .and_then(|workspace_edit| workspace_edit.document_changes)
            .unwrap_or(false),
        documents: HashMap::new(),
        reported_errors: HashSet::new(),
    };
    server.message_loop(connection)
}

fn server_capabilities(encoding: PositionEncoding) -> ServerCapabilities {
    ServerCapabilities {
        position_encoding: Some(encoding.kind()),
        text_document_sync: Some(TextDocumentSyncCapability::Kind(
            TextDocumentSyncKind::FULL,
        )),
        code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
        document_formatting_provider: Some(OneOf::Left(true)),
        ..Default::default()
    }
}

/// An open document: its latest full text (FULL sync sends the whole text on
/// every change) and version, the latter stamped into fix-all edits so a client
/// can drop a stale edit if the buffer moved on before it was applied.
struct Document {
    version: i32,
    text: String,
}

struct Server {
    encoding: PositionEncoding,
    /// Workspace root for anchoring config discovery of non-file (untitled) URIs.
    root: Option<PathBuf>,
    /// Whether the client supports `WorkspaceEdit.documentChanges` (versioned
    /// edits). When it does not, fix-all falls back to the unversioned `changes` map.
    supports_document_changes: bool,
    /// Open documents, keyed by their URI string.
    documents: HashMap<String, Document>,
    /// Config-discovery error messages already surfaced via `window/showMessage`,
    /// so a broken config is reported once rather than on every file/keystroke.
    reported_errors: HashSet<String>,
}

impl Server {
    fn message_loop(mut self, connection: &Connection) -> SessionOutcome {
        for message in &connection.receiver {
            match message {
                Message::Request(request) => {
                    // `Ok(true)` is a clean shutdown; an `Err` means the client
                    // vanished or misbehaved during the shutdown/exit handshake —
                    // either way the session is over, so end the loop gracefully
                    // rather than panicking.
                    if connection.handle_shutdown(&request).unwrap_or(true) {
                        return SessionOutcome::Clean;
                    }
                    self.handle_request(connection, request);
                }
                Message::Notification(notification) => {
                    // A bare `exit` (the spec allows it without a prior `shutdown`)
                    // must terminate the server and, per the spec, is an abnormal
                    // exit; the normal shutdown/exit sequence is consumed by
                    // `handle_shutdown` above and never reaches here.
                    if notification.method == "exit" {
                        return SessionOutcome::Abnormal;
                    }
                    self.handle_notification(connection, notification);
                }
                Message::Response(_) => {}
            }
        }
        // The client dropped the connection without a shutdown/exit: a normal end.
        SessionOutcome::Clean
    }

    fn handle_request(&self, connection: &Connection, request: Request) {
        let Request { id, method, params } = request;
        match method.as_str() {
            "textDocument/codeAction" => {
                let result = parse::<CodeActionParams>(&params)
                    .and_then(|params| self.code_action(&params));
                respond(connection, id, result);
            }
            "textDocument/formatting" => {
                let result = parse::<DocumentFormattingParams>(&params)
                    .and_then(|params| self.formatting(&params));
                respond(connection, id, result);
            }
            other => {
                send(
                    connection,
                    Message::Response(Response::new_err(
                        id,
                        ErrorCode::MethodNotFound as i32,
                        format!("unhandled request: {other}"),
                    )),
                );
            }
        }
    }

    fn handle_notification(
        &mut self,
        connection: &Connection,
        notification: Notification,
    ) {
        let Notification { method, params } = notification;
        match method.as_str() {
            "textDocument/didOpen" => {
                if let Some(params) = parse::<DidOpenTextDocumentParams>(&params) {
                    let document = params.text_document;
                    self.update(
                        connection,
                        document.uri,
                        document.version,
                        document.text,
                    );
                }
            }
            "textDocument/didChange" => {
                if let Some(params) = parse::<DidChangeTextDocumentParams>(&params)
                    && let Some(change) = params.content_changes.into_iter().next_back()
                {
                    let document = params.text_document;
                    self.update(
                        connection,
                        document.uri,
                        document.version,
                        change.text,
                    );
                }
            }
            "textDocument/didClose" => {
                if let Some(params) = parse::<DidCloseTextDocumentParams>(&params) {
                    let uri = params.text_document.uri;
                    self.documents.remove(uri.as_str());
                    publish(connection, uri, None, Vec::new());
                }
            }
            _ => {}
        }
    }

    /// Store the latest text/version for `uri` and publish fresh diagnostics.
    fn update(
        &mut self,
        connection: &Connection,
        uri: Uri,
        version: i32,
        text: String,
    ) {
        let diagnostics = match self.resolve(uri.as_str()) {
            Ok(Some(target)) => analysis::diagnostics(
                &text,
                &target.path,
                &target.context.config,
                &target.context.base_dir,
                target.kind,
                self.encoding,
            ),
            // No config / no rules / ignored / not a linted kind: nothing to report.
            Ok(None) => Vec::new(),
            // A broken config disables linting silently, which is confusing — tell
            // the user once, then publish empty diagnostics like the other cases.
            Err(error) => {
                self.report_config_error(connection, &error);
                Vec::new()
            }
        };
        self.documents
            .insert(uri.as_str().to_string(), Document { version, text });
        publish(connection, uri, Some(version), diagnostics);
    }

    /// Surface a config-discovery error to the user once (deduped by message).
    fn report_config_error(&mut self, connection: &Connection, error: &str) {
        if self.reported_errors.insert(error.to_string()) {
            let params = ShowMessageParams {
                typ: MessageType::ERROR,
                message: format!("ryl: configuration error, linting is off: {error}"),
            };
            send(
                connection,
                Message::Notification(Notification::new(
                    "window/showMessage".to_string(),
                    params,
                )),
            );
        }
    }

    fn code_action(&self, params: &CodeActionParams) -> Option<CodeActionResponse> {
        if !fix_all_requested(&params.context) {
            return None;
        }
        let uri = &params.text_document.uri;
        let version = self.documents.get(uri.as_str())?.version;
        let edit = self.fix_edit(uri.as_str())?;
        let action = CodeAction {
            title: "Fix all ryl problems".to_string(),
            kind: Some(CodeActionKind::new(FIX_ALL_KIND)),
            edit: Some(workspace_edit(
                uri.clone(),
                version,
                edit,
                self.supports_document_changes,
            )),
            ..Default::default()
        };
        Some(vec![CodeActionOrCommand::CodeAction(action)])
    }

    fn formatting(&self, params: &DocumentFormattingParams) -> Option<Vec<TextEdit>> {
        Some(vec![self.fix_edit(params.text_document.uri.as_str())?])
    }

    /// The whole-document safe-fix edit for an open document, shared by the
    /// fix-all code action and formatting.
    fn fix_edit(&self, uri: &str) -> Option<TextEdit> {
        let document = self.documents.get(uri)?;
        // A config error or non-linted file means there is nothing to fix; the
        // error itself was already surfaced when the document was opened/changed.
        let target = self.resolve(uri).ok().flatten()?;
        analysis::fix_all_edit(
            &document.text,
            &target.path,
            &target.context.config,
            &target.context.base_dir,
            target.kind,
            self.encoding,
        )
    }

    /// Resolve the path, config, and source kind for a URI. `Ok(None)` means
    /// nothing to lint (no config, no rules, ignored, or not a linted kind); `Err`
    /// is a config-discovery/parse failure the caller surfaces to the user.
    fn resolve(&self, uri: &str) -> Result<Option<Target>, String> {
        // A non-file URI is an untitled/unsaved buffer with no real path: anchor
        // discovery at the workspace fallback and (below) lint it as YAML regardless
        // of `[files]`, since it is unsaved YAML the user is editing.
        let (path, is_file) = match uri_to_path(uri) {
            Some(path) => (path, true),
            None => (self.fallback_base().join("untitled.yaml"), false),
        };
        // Full CLI precedence for this one input: project config (walking up from
        // the path), then `YAMLLINT_CONFIG_FILE`, then user-global, then empty.
        let context =
            discover_config(std::slice::from_ref(&path), &Overrides::default())?;
        if !context.config.enables_any_rule()
            || context.config.is_file_ignored(&path, &context.base_dir)
        {
            return Ok(None);
        }
        let kind = if is_file {
            // A real file: its kind comes from `[files]`. An overlapping glob makes
            // `source_kind` error (surfaced like any config mistake); no match skips it.
            match context.config.source_kind(&path, &context.base_dir)? {
                Some(kind) => kind,
                None => return Ok(None),
            }
        } else {
            SourceKind::Yaml
        };
        Ok(Some(Target {
            path,
            context,
            kind,
        }))
    }

    fn fallback_base(&self) -> PathBuf {
        self.root
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
    }
}

struct Target {
    path: PathBuf,
    context: ConfigContext,
    kind: SourceKind,
}

/// Build the fix-all workspace edit. A client advertising `documentChanges`
/// support gets a versioned `TextDocumentEdit` (so it can discard the edit if the
/// buffer moved past `version` before the edit is applied); otherwise it gets the
/// unversioned `changes` map. `Uri` has benign interior mutability (a fluent-uri
/// parse cache) that never affects its hash/equality, hence the lint allow.
#[allow(clippy::mutable_key_type)]
fn workspace_edit(
    uri: Uri,
    version: i32,
    edit: TextEdit,
    supports_document_changes: bool,
) -> WorkspaceEdit {
    if supports_document_changes {
        WorkspaceEdit {
            document_changes: Some(DocumentChanges::Edits(vec![TextDocumentEdit {
                text_document: OptionalVersionedTextDocumentIdentifier {
                    uri,
                    version: Some(version),
                },
                edits: vec![OneOf::Left(edit)],
            }])),
            ..Default::default()
        }
    } else {
        WorkspaceEdit {
            changes: Some(HashMap::from([(uri, vec![edit])])),
            ..Default::default()
        }
    }
}

/// Whether the client's `context.only` filter admits the fix-all action: no
/// filter means yes, otherwise a requested kind must equal or be an ancestor of
/// `source.fixAll.ryl` (so a `source` or `source.fixAll` request still matches,
/// the way `editor.codeActionsOnSave` issues them).
fn fix_all_requested(context: &CodeActionContext) -> bool {
    match &context.only {
        None => true,
        Some(only) => only.iter().any(|kind| {
            let kind = kind.as_str();
            FIX_ALL_KIND == kind || FIX_ALL_KIND.starts_with(&format!("{kind}."))
        }),
    }
}

/// After a rejected handshake the session is uninitialized: ignore every message
/// until the client ends it with `exit` (abnormal) or by closing the connection,
/// keeping the stdio reader draining so `run`'s join unwinds instead of blocking.
fn drain_until_session_end(connection: &Connection) -> SessionOutcome {
    for message in &connection.receiver {
        if let Message::Notification(notification) = message
            && notification.method == "exit"
        {
            return SessionOutcome::Abnormal;
        }
    }
    SessionOutcome::Clean
}

fn parse<P: serde::de::DeserializeOwned>(params: &serde_json::Value) -> Option<P> {
    serde_json::from_value(params.clone()).ok()
}

fn publish(
    connection: &Connection,
    uri: Uri,
    version: Option<i32>,
    diagnostics: Vec<lsp_types::Diagnostic>,
) {
    let params = PublishDiagnosticsParams {
        uri,
        diagnostics,
        version,
    };
    send(
        connection,
        Message::Notification(Notification::new(
            "textDocument/publishDiagnostics".to_string(),
            params,
        )),
    );
}

fn respond<R: serde::Serialize>(connection: &Connection, id: RequestId, result: R) {
    send(connection, Message::Response(Response::new_ok(id, result)));
}

fn send(connection: &Connection, message: Message) {
    // A send only fails once the client has dropped the connection, in which case
    // there is nothing left to do but let the loop wind down.
    let _ = connection.sender.send(message);
}
