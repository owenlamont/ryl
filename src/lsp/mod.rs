//! The `ryl server` language server: a thin synchronous protocol adapter over
//! ryl's existing lint and fix engine, built on `lsp-server` + `lsp-types` (the
//! rust-analyzer / Ruff stack). It provides diagnostics (push + pull), code actions
//! (`source.fixAll.ryl`, per-rule fix-all, and disable-rule inserts),
//! `textDocument/formatting` (= apply safe fixes), hover, anchor/alias rename, and
//! config-file watching. Schema validation, completion, and schema hover are
//! intentionally left to Red Hat's `yaml-language-server`, which ryl coexists with.
//!
//! Handling is graceful throughout: a malformed `initialize` (client-controlled)
//! ends the session cleanly, unknown requests get a `MethodNotFound` response, and
//! the loop ends when the client drops the connection or completes the
//! shutdown/exit handshake. The only `expect` is on serialising ryl's own
//! capabilities, which cannot fail.

pub mod actions;
pub mod analysis;
pub mod encoding;
pub mod hover;
pub mod rename;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};

use rayon::prelude::*;

use lsp_server::{
    Connection, ErrorCode, Message, Notification, Request, RequestId, Response,
};
use lsp_types::{
    CancelParams, CodeActionParams, CodeActionProviderCapability, CodeActionResponse,
    Diagnostic, DiagnosticOptions, DiagnosticServerCapabilities, DiagnosticSeverity,
    DidChangeConfigurationParams, DidChangeTextDocumentParams,
    DidChangeWatchedFilesRegistrationOptions, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DocumentDiagnosticParams, DocumentDiagnosticReport,
    DocumentFormattingParams, FileSystemWatcher, FullDocumentDiagnosticReport,
    GlobPattern, Hover, HoverParams, HoverProviderCapability, InitializeParams,
    InitializeResult, MessageType, NumberOrString, OneOf, Position,
    PrepareRenameResponse, PublishDiagnosticsParams, Range, Registration,
    RegistrationParams, RelatedFullDocumentDiagnosticReport, RenameOptions,
    RenameParams, ServerCapabilities, ServerInfo, ShowMessageParams,
    TextDocumentPositionParams, TextDocumentSyncCapability, TextDocumentSyncKind,
    TextEdit, Uri, WorkDoneProgressOptions, WorkspaceDiagnosticReport,
    WorkspaceDocumentDiagnosticReport, WorkspaceEdit,
    WorkspaceFullDocumentDiagnosticReport,
};

use crate::config::{ConfigContext, Overrides, SourceKind, discover_config};
use crate::discover::gather_yaml_from_dir_cancellable;
use crate::lsp::encoding::{
    PositionEncoding, negotiate, offset_at, path_to_uri, uri_to_path,
};

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

    let settings = Settings::from_options(params.initialization_options.as_ref());

    // `initialize_finish` above blocks until the client's `initialized` notification
    // arrives, so the session is fully initialized here — registering a capability now
    // respects the LSP ordering (it is not sent before `initialized`). Register a
    // config-file watcher only when the client supports dynamic registration for it;
    // older clients simply get no auto-reload (they can still re-open files).
    if client_supports_watch_registration(&params) {
        register_config_watchers(connection, settings.config_file.as_deref());
    }

    let server = Server {
        encoding,
        roots: workspace_roots(&params),
        supports_document_changes: params
            .capabilities
            .workspace
            .as_ref()
            .and_then(|workspace| workspace.workspace_edit.as_ref())
            .and_then(|workspace_edit| workspace_edit.document_changes)
            .unwrap_or(false),
        push_diagnostics: !client_supports_pull_diagnostics(&params),
        supports_diagnostic_refresh: client_supports_diagnostic_refresh(&params),
        next_refresh_id: 0,
        settings,
        documents: HashMap::new(),
        reported_errors: HashSet::new(),
        workers: Vec::new(),
    };
    server.message_loop(connection)
}

fn server_capabilities(encoding: PositionEncoding) -> ServerCapabilities {
    ServerCapabilities {
        position_encoding: Some(encoding.kind()),
        // INCREMENTAL: a change carries only the edited range (saves transfer); ryl
        // re-lints the whole reconstructed document regardless.
        text_document_sync: Some(TextDocumentSyncCapability::Kind(
            TextDocumentSyncKind::INCREMENTAL,
        )),
        code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
        document_formatting_provider: Some(OneOf::Left(true)),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        rename_provider: Some(OneOf::Right(RenameOptions {
            prepare_provider: Some(true),
            work_done_progress_options: WorkDoneProgressOptions::default(),
        })),
        diagnostic_provider: Some(DiagnosticServerCapabilities::Options(
            DiagnosticOptions {
                identifier: Some("ryl".to_string()),
                // Each YAML file is linted independently — no cross-file diagnostics.
                inter_file_dependencies: false,
                workspace_diagnostics: true,
                ..Default::default()
            },
        )),
        ..Default::default()
    }
}

/// Whether the client can accept a dynamic `didChangeWatchedFiles` registration.
fn client_supports_watch_registration(params: &InitializeParams) -> bool {
    params
        .capabilities
        .workspace
        .as_ref()
        .and_then(|workspace| workspace.did_change_watched_files.as_ref())
        .and_then(|watched| watched.dynamic_registration)
        .unwrap_or(false)
}

/// Whether the client uses the LSP 3.17 pull-diagnostics model (it advertised the
/// `textDocument/diagnostic` capability). When it does, the server must *not* also
/// push `publishDiagnostics`: clients that keep the push and pull channels in
/// separate collections (e.g. VS Code via `vscode-languageclient`) would then list
/// every diagnostic twice. The spec is silent on how the two models interact, so the
/// robust, client-agnostic rule is to emit only one model per document.
fn client_supports_pull_diagnostics(params: &InitializeParams) -> bool {
    params
        .capabilities
        .text_document
        .as_ref()
        .and_then(|text_document| text_document.diagnostic.as_ref())
        .is_some()
}

/// Whether the client accepts a server-initiated `workspace/diagnostic/refresh`. Only a
/// pull client needs it: a config change re-pushes for a push client, but a pull client's
/// results are gated off, so without a refresh it would keep showing diagnostics computed
/// under the old config.
fn client_supports_diagnostic_refresh(params: &InitializeParams) -> bool {
    params
        .capabilities
        .workspace
        .as_ref()
        .and_then(|workspace| workspace.diagnostic.as_ref())
        .and_then(|diagnostic| diagnostic.refresh_support)
        .unwrap_or(false)
}

/// The client's workspace roots: every `workspace_folders` path, or the
/// LSP-3.17-deprecated `root_uri` for an older client that sends only it (hence the
/// scoped allow). Anchors untitled-buffer config discovery and bounds the
/// `workspace/diagnostic` walk.
fn workspace_roots(params: &InitializeParams) -> Vec<PathBuf> {
    if let Some(folders) = params.workspace_folders.as_ref()
        && !folders.is_empty()
    {
        return folders
            .iter()
            .filter_map(|folder| uri_to_path(folder.uri.as_str()))
            .collect();
    }
    #[allow(deprecated)]
    params
        .root_uri
        .as_ref()
        .and_then(|uri| uri_to_path(uri.as_str()))
        .into_iter()
        .collect()
}

/// Ask the client to watch ryl's config files so an out-of-editor edit re-lints open
/// documents. Watches the standard config filenames anywhere in the workspace, plus an
/// explicitly-configured `config_file` (whose name need not match the standard set).
/// Fire-and-forget: the response is irrelevant and is ignored in the loop.
///
/// Known limitation: files pulled in via a config's `extends:`, and a `configPath` changed
/// after startup, are not (re-)watched — re-open a document to refresh after editing those.
fn register_config_watchers(connection: &Connection, config_file: Option<&Path>) {
    let mut watchers = vec![FileSystemWatcher {
        glob_pattern: GlobPattern::String(
            "**/{ryl.toml,.ryl.toml,pyproject.toml,.yamllint,.yamllint.yaml,\
             .yamllint.yml}"
                .to_string(),
        ),
        kind: None,
    }];
    // An explicit config path may live outside the roots or use a non-standard name, so
    // watch it directly (the `**/` glob above would miss it).
    if let Some(path) = config_file.and_then(Path::to_str) {
        watchers.push(FileSystemWatcher {
            glob_pattern: GlobPattern::String(path.replace('\\', "/")),
            kind: None,
        });
    }
    let options = DidChangeWatchedFilesRegistrationOptions { watchers };
    let registration = Registration {
        id: "ryl-watch-config".to_string(),
        method: "workspace/didChangeWatchedFiles".to_string(),
        register_options: serde_json::to_value(options).ok(),
    };
    let params = RegistrationParams {
        registrations: vec![registration],
    };
    send(
        connection,
        Message::Request(Request::new(
            RequestId::from("ryl-register-watchers".to_string()),
            "client/registerCapability".to_string(),
            params,
        )),
    );
}

/// Ask a pull-capable client to re-pull every diagnostic (LSP `workspace/diagnostic/refresh`,
/// which takes no params). Fire-and-forget — the response is ignored by the message loop —
/// but `seq` still makes the id unique so a client can correlate concurrent refreshes.
///
/// `Value::Null` is the spec-correct no-params shape here, not a malformed `"params":null`:
/// `lsp_server::Request` tags `params` with `skip_serializing_if = Value::is_null`, so a null
/// is omitted from the wire entirely (`{"id":..,"method":..}`).
fn request_diagnostic_refresh(connection: &Connection, seq: i32) {
    send(
        connection,
        Message::Request(Request::new(
            RequestId::from(format!("ryl-refresh-diagnostics-{seq}")),
            "workspace/diagnostic/refresh".to_string(),
            serde_json::Value::Null,
        )),
    );
}

/// An open document: its URI, latest full text (reconstructed from incremental changes),
/// and version. The version is stamped into edits so a client can drop a stale edit if
/// the buffer moved on before it was applied; the URI is kept so re-linting open
/// documents needs no re-parse of the key.
struct Document {
    uri: Uri,
    version: i32,
    text: String,
}

/// Client-provided settings (from `initializationOptions` or
/// `workspace/didChangeConfiguration`): a config-file path / inline data override
/// (mirroring the CLI's `-c`/`-d`) and an on/off toggle (`ryl.enable`). `pub` only so the
/// free `workspace_scan` (which a worker thread runs) is unit-testable.
#[derive(Debug, Clone)]
pub struct Settings {
    config_file: Option<PathBuf>,
    config_data: Option<String>,
    enable: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            config_file: None,
            config_data: None,
            enable: true,
        }
    }
}

impl Settings {
    /// Parse settings from a client payload, falling back to defaults for absent keys.
    fn from_options(value: Option<&serde_json::Value>) -> Self {
        let mut settings = Self::default();
        // Accept either a bare settings object (our `initializationOptions`) or one
        // nested under a `ryl` section (the `didChangeConfiguration` convention).
        if let Some(section) = value.map(|value| value.get("ryl").unwrap_or(value)) {
            if let Some(path) = section.get("configPath").and_then(as_nonempty_str) {
                settings.config_file = Some(PathBuf::from(path));
            }
            if let Some(data) = section.get("configData").and_then(as_nonempty_str) {
                settings.config_data = Some(data.to_string());
            }
            if let Some(enable) =
                section.get("enable").and_then(serde_json::Value::as_bool)
            {
                settings.enable = enable;
            }
        }
        settings
    }

    fn overrides(&self) -> Overrides {
        Overrides {
            config_file: self.config_file.clone(),
            config_data: self.config_data.clone(),
        }
    }
}

fn as_nonempty_str(value: &serde_json::Value) -> Option<&str> {
    value.as_str().filter(|text| !text.is_empty())
}

struct Server {
    encoding: PositionEncoding,
    /// Workspace roots (all client folders) for anchoring config discovery of non-file
    /// (untitled) URIs and for enumerating files in a `workspace/diagnostic` pull. Empty
    /// when the client sent no folders.
    roots: Vec<PathBuf>,
    /// Whether the client supports `WorkspaceEdit.documentChanges` (versioned edits).
    supports_document_changes: bool,
    /// Whether to proactively push `publishDiagnostics`. False when the client uses the
    /// pull model (see [`client_supports_pull_diagnostics`]), so it gets diagnostics
    /// once via pull instead of twice.
    push_diagnostics: bool,
    /// Whether the client accepts `workspace/diagnostic/refresh`, so a pull client can be
    /// asked to re-pull after a config change (see [`client_supports_diagnostic_refresh`]).
    supports_diagnostic_refresh: bool,
    /// Monotonic counter for `workspace/diagnostic/refresh` request ids. Each refresh needs
    /// a distinct id so a client can correlate its response (JSON-RPC forbids reusing an id
    /// for a still-outstanding request, and config changes can fire several in a row).
    next_refresh_id: i32,
    /// Client-provided overrides and on/off toggle.
    settings: Settings,
    /// Open documents, keyed by their URI string.
    documents: HashMap<String, Document>,
    /// Config-discovery error messages already surfaced via `window/showMessage`,
    /// so a broken config is reported once rather than on every file/keystroke.
    reported_errors: HashSet<String>,
    /// In-flight `workspace/diagnostic` scans, each on its own thread so the (potentially
    /// large) repo walk never blocks the message loop. Each carries a cancellation flag
    /// the loop flips on `$/cancelRequest` or at shutdown.
    workers: Vec<Worker>,
}

/// A `workspace/diagnostic` scan running on a background thread.
struct Worker {
    id: RequestId,
    cancel: Arc<AtomicBool>,
    handle: JoinHandle<()>,
}

/// Open-document text/version snapshot, keyed by path, handed to a worker so it can prefer
/// unsaved buffer content over on-disk content without sharing the live document store.
pub type OpenText = HashMap<PathBuf, (String, i32)>;

impl Server {
    fn message_loop(mut self, connection: &Connection) -> SessionOutcome {
        let outcome = self.run_loop(connection);
        // Signal and join any in-flight workspace scans so no detached thread outlives the
        // session (each checks its cancel flag between files, so this returns promptly).
        for worker in self.workers.drain(..) {
            worker.cancel.store(true, Ordering::Relaxed);
            let _ = worker.handle.join();
        }
        outcome
    }

    fn run_loop(&mut self, connection: &Connection) -> SessionOutcome {
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
                // Responses (e.g. to our fire-and-forget registerCapability) are ignored.
                Message::Response(_) => {}
            }
        }
        // The client dropped the connection without a shutdown/exit: a normal end.
        SessionOutcome::Clean
    }

    fn handle_request(&mut self, connection: &Connection, request: Request) {
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
            "textDocument/hover" => {
                let result = parse::<HoverParams>(&params)
                    .and_then(|params| self.hover(&params));
                respond(connection, id, result);
            }
            "textDocument/prepareRename" => {
                let result = parse::<TextDocumentPositionParams>(&params)
                    .and_then(|params| self.prepare_rename(&params));
                respond(connection, id, result);
            }
            "textDocument/rename" => self.rename(connection, id, &params),
            "textDocument/diagnostic" => {
                let result = parse::<DocumentDiagnosticParams>(&params)
                    .map(|params| self.document_diagnostic(&params));
                respond(connection, id, result);
            }
            "workspace/diagnostic" => self.spawn_workspace_diagnostic(connection, id),
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
                if let Some(params) = parse::<DidChangeTextDocumentParams>(&params) {
                    self.apply_changes(connection, params);
                }
            }
            "textDocument/didClose" => {
                if let Some(params) = parse::<DidCloseTextDocumentParams>(&params) {
                    let uri = params.text_document.uri;
                    self.documents.remove(uri.as_str());
                    self.push(connection, uri, None, Vec::new());
                }
            }
            // A watched config file changed: config is resolved per request (no cache), so
            // make open documents' diagnostics catch up (see `handle_config_change`).
            "workspace/didChangeWatchedFiles" => self.handle_config_change(connection),
            "workspace/didChangeConfiguration" => {
                if let Some(params) = parse::<DidChangeConfigurationParams>(&params) {
                    self.settings = Settings::from_options(Some(&params.settings));
                    self.handle_config_change(connection);
                }
            }
            // Cancel an in-flight workspace scan: flip its cancel flag so the worker stops
            // between files and answers with a RequestCancelled error.
            "$/cancelRequest" => {
                if let Some(params) = parse::<CancelParams>(&params) {
                    let target = request_id(params.id);
                    for worker in &self.workers {
                        if worker.id == target {
                            worker.cancel.store(true, Ordering::Relaxed);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Reconstruct the document from incremental (or full-replace) changes, store it, and
    /// publish fresh diagnostics. A change with a range patches the current text at that
    /// range; a range-less change replaces the whole document.
    fn apply_changes(
        &mut self,
        connection: &Connection,
        params: DidChangeTextDocumentParams,
    ) {
        let uri = params.text_document.uri;
        let version = params.text_document.version;
        let mut text = self
            .documents
            .get(uri.as_str())
            .map_or_else(String::new, |document| document.text.clone());
        for change in params.content_changes {
            match change.range {
                Some(range) => {
                    let start = offset_at(&text, range.start, self.encoding);
                    let end = offset_at(&text, range.end, self.encoding);
                    // Ignore a reversed range (offsets are clamped to the text and land
                    // on char boundaries, so the splice itself is always valid).
                    if start <= end {
                        text.replace_range(start..end, &change.text);
                    }
                }
                None => text = change.text,
            }
        }
        self.update(connection, uri, version, text);
    }

    /// Store the latest text/version for `uri` and publish fresh diagnostics, surfacing a
    /// config error once if discovery fails.
    fn update(
        &mut self,
        connection: &Connection,
        uri: Uri,
        version: i32,
        text: String,
    ) {
        let diagnostics = match self.diagnostics_for(uri.as_str(), &text) {
            Ok(diagnostics) => diagnostics,
            // A broken config disables linting silently, which is confusing — tell the
            // user once, then publish empty diagnostics.
            Err(error) => {
                self.report_config_error(connection, &error);
                Vec::new()
            }
        };
        self.documents.insert(
            uri.as_str().to_string(),
            Document {
                uri: uri.clone(),
                version,
                text,
            },
        );
        self.push(connection, uri, Some(version), diagnostics);
    }

    /// Push diagnostics to the client, unless it uses the pull model (then it owns
    /// when to fetch them and a second push would double-report). Both the `update`
    /// republish and the `didClose` clear route through here so the policy lives in
    /// one place.
    fn push(
        &self,
        connection: &Connection,
        uri: Uri,
        version: Option<i32>,
        diagnostics: Vec<Diagnostic>,
    ) {
        if self.push_diagnostics {
            publish(connection, uri, version, diagnostics);
        }
    }

    /// React to a config or watched-file change. A push client gets a fresh re-push of
    /// every open document; a pull client (whose push results are gated off) is asked to
    /// re-pull via `workspace/diagnostic/refresh`, so it does not keep showing diagnostics
    /// computed under the old config — config errors included, which it surfaces as a pull
    /// diagnostic. A pull client that did not advertise refresh support re-pulls only on
    /// its own cadence (an unavoidable client limitation).
    fn handle_config_change(&mut self, connection: &Connection) {
        // Clear the surfaced-errors set so a still-broken config re-reports once (push
        // path); the pull path resurfaces it per document on the triggered re-pull.
        self.reported_errors.clear();
        if self.push_diagnostics {
            self.relint_open_documents(connection);
        } else if self.supports_diagnostic_refresh {
            self.next_refresh_id += 1;
            request_diagnostic_refresh(connection, self.next_refresh_id);
        }
    }

    /// Re-lint and republish every open document (after a config or watched-file change).
    fn relint_open_documents(&mut self, connection: &Connection) {
        // Snapshot to avoid borrowing `documents` while `update` mutates it.
        let snapshot: Vec<(Uri, i32, String)> = self
            .documents
            .values()
            .map(|document| {
                (
                    document.uri.clone(),
                    document.version,
                    document.text.clone(),
                )
            })
            .collect();
        for (uri, version, text) in snapshot {
            self.update(connection, uri, version, text);
        }
    }

    /// Surface a config-discovery error to the user once (deduped by message).
    fn report_config_error(&mut self, connection: &Connection, error: &str) {
        if self.reported_errors.insert(error.to_string()) {
            let params = ShowMessageParams {
                typ: MessageType::ERROR,
                message: config_error_text(error),
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
        let uri = &params.text_document.uri;
        let document = self.documents.get(uri.as_str())?;
        let target = self.resolve(uri.as_str()).ok().flatten()?;
        let input = actions::Input {
            uri,
            text: &document.text,
            version: document.version,
            path: &target.path,
            cfg: &target.context.config,
            base_dir: &target.context.base_dir,
            kind: target.kind,
            enc: self.encoding,
            supports_document_changes: self.supports_document_changes,
        };
        actions::build(&input, &params.context)
    }

    fn formatting(&self, params: &DocumentFormattingParams) -> Option<Vec<TextEdit>> {
        let uri = params.text_document.uri.as_str();
        let document = self.documents.get(uri)?;
        let target = self.resolve(uri).ok().flatten()?;
        Some(vec![analysis::fix_all_edit(
            &document.text,
            &target.path,
            &target.context.config,
            &target.context.base_dir,
            target.kind,
            self.encoding,
        )?])
    }

    fn hover(&self, params: &HoverParams) -> Option<Hover> {
        let position = &params.text_document_position_params;
        let document = self.documents.get(position.text_document.uri.as_str())?;
        // Recompute diagnostics for hit-testing; the engine is sub-ms/file and this
        // avoids caching published diagnostics. A config error here is silent (it was
        // already surfaced on open/change).
        let diagnostics = self
            .diagnostics_for(position.text_document.uri.as_str(), &document.text)
            .unwrap_or_default();
        hover::hover(&diagnostics, position.position)
    }

    fn prepare_rename(
        &self,
        params: &TextDocumentPositionParams,
    ) -> Option<PrepareRenameResponse> {
        let uri = params.text_document.uri.as_str();
        if !matches!(self.document_kind(uri), Some(SourceKind::Yaml)) {
            return None;
        }
        let document = self.documents.get(uri)?;
        rename::prepare_rename(&document.text, params.position, self.encoding)
    }

    fn rename(
        &self,
        connection: &Connection,
        id: RequestId,
        params: &serde_json::Value,
    ) {
        let null = || respond(connection, id.clone(), Option::<WorkspaceEdit>::None);
        let Some(params) = parse::<RenameParams>(params) else {
            null();
            return;
        };
        let uri = &params.text_document_position.text_document.uri;
        let Some(document) = self.documents.get(uri.as_str()) else {
            null();
            return;
        };
        if !matches!(self.document_kind(uri.as_str()), Some(SourceKind::Yaml)) {
            null();
            return;
        }
        match rename::rename_edits(
            &document.text,
            params.text_document_position.position,
            &params.new_name,
            self.encoding,
        ) {
            Ok(Some(edits)) => {
                let edit = actions::workspace_edit(
                    uri.clone(),
                    document.version,
                    edits,
                    self.supports_document_changes,
                );
                respond(connection, id, Some(edit));
            }
            Ok(None) => null(),
            // An illegal new name is a request error, per the LSP rename spec.
            Err(message) => send(
                connection,
                Message::Response(Response::new_err(
                    id,
                    ErrorCode::InvalidParams as i32,
                    message,
                )),
            ),
        }
    }

    /// The pull-diagnostic report for one document (stored text if open, else read from
    /// disk).
    fn document_diagnostic(
        &self,
        params: &DocumentDiagnosticParams,
    ) -> DocumentDiagnosticReport {
        let uri = params.text_document.uri.as_str();
        let items = self.document_text(uri).map_or_else(Vec::new, |text| {
            // Surface a config failure as an error diagnostic rather than an empty (clean)
            // report, so a pull-only client is not misled into thinking the file is fine.
            self.diagnostics_for(uri, &text)
                .unwrap_or_else(|error| vec![config_error_diagnostic(&error)])
        });
        DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport {
            related_documents: None,
            full_document_diagnostic_report: FullDocumentDiagnosticReport {
                result_id: None,
                items,
            },
        })
    }

    /// Snapshot open documents (text + version) by path, so a worker can prefer unsaved
    /// buffer content over on-disk content without sharing the live document store.
    fn open_snapshot(&self) -> OpenText {
        self.documents
            .iter()
            .filter_map(|(uri, document)| {
                uri_to_path(uri)
                    .map(|path| (path, (document.text.clone(), document.version)))
            })
            .collect()
    }

    /// Start a `workspace/diagnostic` scan on a background thread so the (potentially
    /// large) repo walk + parallel lint never blocks the message loop. The worker answers
    /// the request itself over the connection; the loop stays responsive and can cancel it.
    fn spawn_workspace_diagnostic(&mut self, connection: &Connection, id: RequestId) {
        // Supersede any in-flight scan: cancel it (it answers RequestCancelled) so a client
        // issuing rapid pulls (e.g. on every save) does not accumulate concurrent
        // walks/snapshots — only the latest pull does real work, keeping the worker count
        // bounded. Then drop the handles of any that have already finished.
        for worker in &self.workers {
            worker.cancel.store(true, Ordering::Relaxed);
        }
        self.workers.retain(|worker| !worker.handle.is_finished());
        let cancel = Arc::new(AtomicBool::new(false));
        let token = Arc::clone(&cancel);
        let sender = connection.sender.clone();
        let roots = self.roots.clone();
        let settings = self.settings.clone();
        let encoding = self.encoding;
        let open = self.open_snapshot();
        let response_id = id.clone();
        let handle = thread::spawn(move || {
            let scan = workspace_scan(&roots, &open, &settings, encoding, &token);
            let _ =
                sender.send(Message::Response(workspace_response(response_id, scan)));
        });
        self.workers.push(Worker { id, cancel, handle });
    }

    /// Text of `uri`: the open buffer if tracked, else the file decoded from disk.
    fn document_text(&self, uri: &str) -> Option<String> {
        if let Some(document) = self.documents.get(uri) {
            return Some(document.text.clone());
        }
        let path = uri_to_path(uri)?;
        crate::decoder::read_file(&path).ok()
    }

    /// Diagnostics for `text` resolved against `uri`'s config, or `Err` on a config
    /// failure (callers decide whether to surface it).
    fn diagnostics_for(
        &self,
        uri: &str,
        text: &str,
    ) -> Result<Vec<Diagnostic>, String> {
        Ok(match self.resolve(uri)? {
            Some(target) => analysis::diagnostics(
                text,
                &target.path,
                &target.context.config,
                &target.context.base_dir,
                target.kind,
                self.encoding,
            ),
            None => Vec::new(),
        })
    }

    /// Resolve the path, config, and source kind for a URI. `Ok(None)` means nothing to
    /// lint (disabled, no config, no rules, ignored, or not a linted kind); `Err` is a
    /// config-discovery/parse failure the caller surfaces to the user.
    fn resolve(&self, uri: &str) -> Result<Option<Target>, String> {
        let (path, is_file) = self.uri_path(uri);
        self.resolve_path(path, is_file, true)
    }

    /// As [`Self::resolve`] but from an already-decoded path. `require_rules` gates on the
    /// config enabling at least one rule (true for linting/fixing; false for rename, which
    /// works regardless of lint config). Used by `document_kind`; the free
    /// [`resolve_for_path`] backs both this and the worker (which has no `&self`).
    fn resolve_path(
        &self,
        path: PathBuf,
        is_file: bool,
        require_rules: bool,
    ) -> Result<Option<Target>, String> {
        resolve_for_path(path, is_file, require_rules, &self.settings)
    }

    /// The source kind of `uri` ignoring whether any rule is enabled — rename works
    /// regardless of lint config, but only on YAML documents. `None` when disabled,
    /// ignored, config fails, or the kind is not YAML/markdown.
    fn document_kind(&self, uri: &str) -> Option<SourceKind> {
        let (path, is_file) = self.uri_path(uri);
        self.resolve_path(path, is_file, false)
            .ok()
            .flatten()
            .map(|target| target.kind)
    }

    /// Decode a URI to `(path, is_file)`: a non-file URI is an untitled/unsaved buffer
    /// with no real path, anchored at the workspace fallback and linted as YAML.
    fn uri_path(&self, uri: &str) -> (PathBuf, bool) {
        match uri_to_path(uri) {
            Some(path) => (path, true),
            None => (self.fallback_base().join("untitled.yaml"), false),
        }
    }

    fn fallback_base(&self) -> PathBuf {
        // Anchor untitled buffers at the first workspace root, else the process cwd.
        self.roots
            .first()
            .cloned()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
    }
}

struct Target {
    path: PathBuf,
    context: ConfigContext,
    kind: SourceKind,
}

/// Resolve config + source kind for an already-decoded path, layering `settings` (the
/// client overrides + enable toggle) onto CLI-precedence discovery. `require_rules` gates
/// on the config enabling at least one rule (true for linting/fixing; false for rename).
/// Free (no `&self`) so a `workspace/diagnostic` worker thread can call it too.
fn resolve_for_path(
    path: PathBuf,
    is_file: bool,
    require_rules: bool,
    settings: &Settings,
) -> Result<Option<Target>, String> {
    if !settings.enable {
        return Ok(None);
    }
    let mut context =
        discover_config(std::slice::from_ref(&path), &settings.overrides())?;
    if require_rules && !context.config.enables_any_rule() {
        return Ok(None);
    }
    let kind = if is_file {
        // A real file: honour path-based ignores, and take its kind from `[files]`.
        if context.config.is_file_ignored(&path, &context.base_dir) {
            return Ok(None);
        }
        match context.config.source_kind(&path, &context.base_dir)? {
            Some(kind) => kind,
            None => return Ok(None),
        }
    } else {
        // A non-file (untitled/unsaved) buffer has no real path: like stdin without
        // `--stdin-filename`, disable every path-based filter and lint it as YAML.
        context.config.disable_path_based_rule_ignores();
        SourceKind::Yaml
    };
    Ok(Some(Target {
        path,
        context,
        kind,
    }))
}

/// Lint one workspace file for a pull report, preferring the open buffer's text. Returns
/// `None` to skip a non-linted/ignored file or one that can't be read; a config failure
/// becomes an error report rather than a silent omit (a pull client would read absence as
/// clean).
fn file_report(
    path: &Path,
    settings: &Settings,
    encoding: PositionEncoding,
    open: &OpenText,
) -> Option<WorkspaceDocumentDiagnosticReport> {
    let target = match resolve_for_path(path.to_path_buf(), true, true, settings) {
        Ok(Some(target)) => target,
        Ok(None) => return None,
        Err(error) => {
            return Some(workspace_report(
                path_to_uri(path),
                None,
                vec![config_error_diagnostic(&error)],
            ));
        }
    };
    let (text, version) = match open.get(path) {
        Some((text, version)) => (text.clone(), Some(i64::from(*version))),
        None => (crate::decoder::read_file(path).ok()?, None),
    };
    let items = analysis::diagnostics(
        &text,
        &target.path,
        &target.context.config,
        &target.context.base_dir,
        target.kind,
        encoding,
    );
    Some(workspace_report(path_to_uri(path), version, items))
}

/// The `workspace/diagnostic` scan: enumerate `*.yaml`/`*.yml` under each root (git-ignore
/// honoured), de-duplicate across roots, then lint them in parallel ([`rayon`], like the
/// CLI). Returns `None` if `cancel` is set (the request was cancelled), so the worker
/// answers with `RequestCancelled`. The walk + lint run off the message loop, so the server
/// stays responsive however large the tree. `pub` for unit testing (a worker runs it).
pub fn workspace_scan(
    roots: &[PathBuf],
    open: &OpenText,
    settings: &Settings,
    encoding: PositionEncoding,
    cancel: &AtomicBool,
) -> Option<Vec<WorkspaceDocumentDiagnosticReport>> {
    let mut files = Vec::new();
    let mut seen = HashSet::new();
    for root in roots {
        // The walk is per-entry cancellable, so a cancelled pull stops enumerating a (huge
        // or slow) tree instead of finishing it; `?` propagates the cancellation. The lint
        // pass below is fast and rayon-parallel and is not separately interrupted — the
        // residual mid-`read_file` window is per ryl's threat model (realistic payloads,
        // not a degraded-fs racer), and that in-progress failure mode cannot be tested
        // deterministically.
        for path in gather_yaml_from_dir_cancellable(root, cancel)? {
            // De-duplicate so a file reachable from two (e.g. nested) roots is linted once.
            if seen.insert(path.clone()) {
                files.push(path);
            }
        }
    }
    Some(
        files
            .par_iter()
            .filter_map(|path| file_report(path, settings, encoding, open))
            .collect(),
    )
}

/// Build the `workspace/diagnostic` response: the report, or a `RequestCancelled` error
/// when the scan was cancelled. `pub` for unit testing.
#[must_use]
pub fn workspace_response(
    id: RequestId,
    scan: Option<Vec<WorkspaceDocumentDiagnosticReport>>,
) -> Response {
    match scan {
        Some(items) => Response::new_ok(id, WorkspaceDiagnosticReport { items }),
        None => Response::new_err(
            id,
            ErrorCode::RequestCanceled as i32,
            "workspace diagnostic cancelled".to_string(),
        ),
    }
}

/// Convert an LSP `$/cancelRequest` id to an `lsp-server` request id for matching.
fn request_id(id: NumberOrString) -> RequestId {
    match id {
        NumberOrString::Number(number) => RequestId::from(number),
        NumberOrString::String(text) => RequestId::from(text),
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

/// The user-facing message for a config-discovery failure, shared by the push path's
/// `window/showMessage` and the pull path's synthetic diagnostic.
fn config_error_text(error: &str) -> String {
    format!("ryl: configuration error, linting is off: {error}")
}

/// A single error diagnostic standing in for a config-discovery failure, so a pull
/// request surfaces the failure (in the report the client consumes) rather than reporting
/// the file as clean.
fn config_error_diagnostic(error: &str) -> Diagnostic {
    Diagnostic {
        range: Range {
            start: Position::new(0, 0),
            end: Position::new(0, 0),
        },
        severity: Some(DiagnosticSeverity::ERROR),
        source: Some("ryl".to_string()),
        message: config_error_text(error),
        ..Default::default()
    }
}

/// One file's full report for a `workspace/diagnostic` result.
fn workspace_report(
    uri: Uri,
    version: Option<i64>,
    items: Vec<Diagnostic>,
) -> WorkspaceDocumentDiagnosticReport {
    WorkspaceDocumentDiagnosticReport::Full(WorkspaceFullDocumentDiagnosticReport {
        uri,
        version,
        full_document_diagnostic_report: FullDocumentDiagnosticReport {
            result_id: None,
            items,
        },
    })
}

fn publish(
    connection: &Connection,
    uri: Uri,
    version: Option<i32>,
    diagnostics: Vec<Diagnostic>,
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
