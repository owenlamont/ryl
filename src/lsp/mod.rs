//! The `ryl server` language server: a synchronous protocol adapter over ryl's lint/fix
//! engine, built on `lsp-server` + `lsp-types`. Malformed client input (a bad
//! `initialize`, an unknown request) is handled gracefully rather than panicking; the two
//! `expect`s are on serialising ryl's own capabilities and on a channel whose sender the
//! receiving loop owns, neither of which can fail.

pub mod actions;
pub mod analysis;
pub mod encoding;
pub mod hover;
pub mod rename;

use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, select, unbounded};
use rayon::prelude::*;

use lsp_server::{
    Connection, ErrorCode, Message, Notification, Request, RequestId, Response,
};
use lsp_types::{
    CancelParams, CodeActionParams, CodeActionProviderCapability, CodeActionResponse,
    Diagnostic, DiagnosticOptions, DiagnosticServerCapabilities, DiagnosticSeverity,
    DidChangeConfigurationParams, DidChangeTextDocumentParams,
    DidChangeWatchedFilesParams, DidChangeWatchedFilesRegistrationOptions,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DocumentDiagnosticParams,
    DocumentDiagnosticReport, DocumentFormattingParams, FileSystemWatcher,
    FullDocumentDiagnosticReport, GlobPattern, Hover, HoverParams,
    HoverProviderCapability, InitializeParams, InitializeResult, MessageType,
    NumberOrString, OneOf, Position, PrepareRenameResponse, PreviousResultId,
    ProgressToken, PublishDiagnosticsParams, Range, Registration, RegistrationParams,
    RelatedFullDocumentDiagnosticReport, RelatedUnchangedDocumentDiagnosticReport,
    RenameOptions, RenameParams, ServerCapabilities, ServerInfo, ShowMessageParams,
    TextDocumentPositionParams, TextDocumentSyncCapability, TextDocumentSyncKind,
    TextEdit, UnchangedDocumentDiagnosticReport, Uri, WorkDoneProgressOptions,
    WorkspaceDiagnosticParams, WorkspaceDiagnosticReport,
    WorkspaceDiagnosticReportPartialResult, WorkspaceDocumentDiagnosticReport,
    WorkspaceEdit, WorkspaceFullDocumentDiagnosticReport,
    WorkspaceUnchangedDocumentDiagnosticReport,
};

use crate::config::{ConfigContext, Overrides, SourceKind, discover_config};
use crate::discover::gather_yaml_from_dir_cancellable;
use crate::lsp::encoding::{
    PositionEncoding, negotiate, offset_at, path_to_uri, uri_to_path,
};

/// How a session ended, mapped to a process exit code by [`run`]. Per the LSP spec an
/// `exit` without a prior `shutdown` is abnormal (exit 1); every other ending is exit 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionOutcome {
    Clean,
    Abnormal,
}

/// Run the language server over stdio, returning the process exit code.
///
/// # Panics
/// Only if the stdio reader/writer threads fail to join, which a working transport never
/// triggers.
#[must_use]
pub fn run() -> ExitCode {
    let (connection, io_threads) = Connection::stdio();
    let outcome = serve(&connection);
    // Drop the connection so the outgoing channel closes; else the stdio writer thread
    // never finishes and `io_threads.join()` hangs.
    drop(connection);
    io_threads
        .join()
        .expect("LSP stdio reader/writer threads should join cleanly");
    match outcome {
        SessionOutcome::Clean => ExitCode::SUCCESS,
        SessionOutcome::Abnormal => ExitCode::from(1),
    }
}

/// Drive the protocol over an established connection: the `initialize` handshake then the
/// message loop. Works over any [`Connection`] (`run` wires stdio, tests use
/// `Connection::memory()`); the caller must drop the connection after this returns so the
/// stdio writer thread can finish (see [`run`]).
///
/// # Panics
/// Only if serialising ryl's own server capabilities fails, which cannot happen.
#[must_use]
pub fn serve(connection: &Connection) -> SessionOutcome {
    // The initialize request is client-controlled, so a malformed one ends the session
    // cleanly rather than panicking.
    let Ok((id, raw_params)) = connection.initialize_start() else {
        return SessionOutcome::Clean;
    };
    // Read before `from_value` consumes `raw_params`: this capability lives at a JSON key
    // `lsp-types` cannot reach (see `client_supports_diagnostic_refresh`).
    let supports_diagnostic_refresh = client_supports_diagnostic_refresh(&raw_params);
    let params: InitializeParams = match serde_json::from_value(raw_params) {
        Ok(params) => params,
        Err(error) => {
            // Reject the handshake, then drain until the client ends the session: returning
            // here would leave a stdio client's reader thread blocked, hanging the join.
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
    // `to_value` of our own capabilities cannot fail; a transport error finishing the
    // handshake just means the client is gone, so the loop below ends at once.
    let result =
        serde_json::to_value(result).expect("server capabilities always serialize");
    let _ = connection.initialize_finish(id, result);

    let settings = Settings::from_options(params.initialization_options.as_ref());

    // `initialize_finish` blocks until the client's `initialized` arrives, so registering a
    // capability now respects the LSP ordering (not sent before `initialized`). Only when
    // the client supports dynamic registration; older clients get no auto-reload.
    if client_supports_watch_registration(&params) {
        register_config_watchers(connection, settings.config_file.as_deref());
    }

    let (scan_tx, scan_rx) = unbounded();
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
        supports_diagnostic_refresh,
        next_refresh_id: 0,
        settings,
        documents: HashMap::new(),
        reported_errors: HashSet::new(),
        workers: Vec::new(),
        pull: None,
        revision: 0,
        scan_tx,
        scan_rx,
    };
    server.message_loop(connection)
}

fn server_capabilities(encoding: PositionEncoding) -> ServerCapabilities {
    ServerCapabilities {
        position_encoding: Some(encoding.kind()),
        // INCREMENTAL: a change carries only the edited range; ryl re-lints the whole
        // reconstructed document regardless.
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
                // Each YAML file is linted independently.
                inter_file_dependencies: false,
                workspace_diagnostics: true,
                ..Default::default()
            },
        )),
        ..Default::default()
    }
}

fn client_supports_watch_registration(params: &InitializeParams) -> bool {
    params
        .capabilities
        .workspace
        .as_ref()
        .and_then(|workspace| workspace.did_change_watched_files.as_ref())
        .and_then(|watched| watched.dynamic_registration)
        .unwrap_or(false)
}

/// Whether the client uses the LSP 3.17 pull model. When it does the server must *not*
/// also push `publishDiagnostics`: a client keeping the two channels in separate
/// collections (e.g. VS Code) would list every diagnostic twice. Emit only one model per
/// document.
fn client_supports_pull_diagnostics(params: &InitializeParams) -> bool {
    params
        .capabilities
        .text_document
        .as_ref()
        .and_then(|text_document| text_document.diagnostic.as_ref())
        .is_some()
}

/// Whether the client accepts a server-initiated `workspace/diagnostic/refresh`. Only a
/// pull client needs it: its results are gated off, so without a refresh a config change
/// would leave it showing diagnostics computed under the old config.
///
/// Read from the *raw* JSON: the spec key is the plural
/// `workspace.diagnostics.refreshSupport`, but `lsp-types` 0.97 deserializes its
/// `WorkspaceClientCapabilities::diagnostic` from the singular `workspace.diagnostic` key
/// (a known bug: tower-lsp-community/tower-lsp-server#50), so the typed field is always
/// `None` for a conforming client. The textDocument pull capability above is genuinely
/// singular per spec, so it stays on the typed path.
fn client_supports_diagnostic_refresh(raw_params: &serde_json::Value) -> bool {
    raw_params
        .pointer("/capabilities/workspace/diagnostics/refreshSupport")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

/// The client's workspace roots: every `workspace_folders` path, falling back to the
/// deprecated `root_uri` (hence the scoped allow) for an older client that sends only it.
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

/// The config file names ryl discovers: watched so an out-of-editor edit re-lints, and
/// reused by [`Server::is_config_uri`] to tell a config change from a source one.
const WATCHED_CONFIG_NAMES: [&str; 6] = [
    "ryl.toml",
    ".ryl.toml",
    "pyproject.toml",
    ".yamllint",
    ".yamllint.yaml",
    ".yamllint.yml",
];

/// Ask the client to watch ryl's config files and YAML sources, so an out-of-editor edit
/// re-lints open documents and wakes a long-polling pull. Fire-and-forget.
///
/// Known limitation: files pulled in via a config's `extends:`, and a `configPath` changed
/// after startup, are not (re-)watched; re-open a document to refresh after editing those.
fn register_config_watchers(connection: &Connection, config_file: Option<&Path>) {
    let mut watchers = vec![
        FileSystemWatcher {
            glob_pattern: GlobPattern::String(format!(
                "**/{{{}}}",
                WATCHED_CONFIG_NAMES.join(",")
            )),
            kind: None,
        },
        // Else a pull suspended for long polling never learns of a `git checkout`.
        FileSystemWatcher {
            glob_pattern: GlobPattern::String("**/*.{yaml,yml}".to_string()),
            kind: None,
        },
    ];
    // An explicit config path may live outside the roots or use a non-standard name, which
    // the `**/` glob above would miss, so watch it directly.
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

/// Ask a pull-capable client to re-pull every diagnostic. Fire-and-forget; `seq` keeps the
/// id unique so a client can correlate concurrent refreshes.
///
/// `Value::Null` is the spec-correct no-params shape, not a malformed `"params":null`:
/// `lsp_server::Request` tags `params` `skip_serializing_if = Value::is_null`, so it is
/// omitted from the wire entirely.
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

/// An open document. `version` is stamped into edits so a client can drop one whose buffer
/// moved on before it was applied.
struct Document {
    uri: Uri,
    version: i32,
    text: String,
}

/// Client-provided settings: a config-file path / inline data override (the CLI's
/// `-c`/`-d`) and an on/off toggle (`ryl.enable`). `pub` only so the free `workspace_scan`
/// is unit-testable.
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
    fn from_options(value: Option<&serde_json::Value>) -> Self {
        let mut settings = Self::default();
        // Accept either a bare settings object (`initializationOptions`) or one nested under
        // a `ryl` section (the `didChangeConfiguration` convention).
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
    /// All client folders, for anchoring config discovery of untitled URIs and enumerating
    /// files in a `workspace/diagnostic` pull. Empty when the client sent no folders.
    roots: Vec<PathBuf>,
    supports_document_changes: bool,
    /// Whether to push `publishDiagnostics`. False for a pull client so it gets diagnostics
    /// once via pull, not twice (see [`client_supports_pull_diagnostics`]).
    push_diagnostics: bool,
    supports_diagnostic_refresh: bool,
    /// Each `workspace/diagnostic/refresh` needs a distinct id (JSON-RPC forbids reusing one
    /// for an outstanding request, and config changes can fire several in a row).
    next_refresh_id: i32,
    settings: Settings,
    documents: HashMap<String, Document>,
    /// Config errors already surfaced via `window/showMessage`, so a broken config is
    /// reported once rather than on every file/keystroke.
    reported_errors: HashSet<String>,
    /// In-flight `workspace/diagnostic` scans, each on its own thread (so the repo walk
    /// never blocks the message loop) with a flag the loop flips to cancel it.
    workers: Vec<Worker>,
    /// The client's outstanding `workspace/diagnostic` pull; at most one, since the client
    /// sends the next only once this is answered.
    pull: Option<Pull>,
    /// Bumped by every notification that can change a lint, marking older scans stale.
    revision: u64,
    /// Scans report back here rather than answering, so the hold-or-respond decision is
    /// made where the session state lives.
    scan_tx: Sender<ScanResult>,
    scan_rx: Receiver<ScanResult>,
}

struct Worker {
    cancel: Arc<AtomicBool>,
    handle: JoinHandle<()>,
}

/// A `workspace/diagnostic` request, being scanned or held open (LSP long polling).
struct Pull {
    id: RequestId,
    /// Replayed on every retry: the client's knowledge cannot move on while unanswered.
    previous: Vec<PreviousResultId>,
    /// Present when the client offered to receive results as they are produced.
    token: Option<ProgressToken>,
}

struct ScanResult {
    id: RequestId,
    revision: u64,
    /// `None` when the scan was cancelled.
    scan: Option<ScanOutcome>,
}

/// Open-document text/version snapshot, keyed by path, handed to a worker so it can prefer
/// unsaved buffer content over on-disk without sharing the live document store.
pub type OpenText = HashMap<PathBuf, (String, i32)>;

impl Server {
    fn message_loop(mut self, connection: &Connection) -> SessionOutcome {
        let outcome = self.run_loop(connection);
        // Else a suspended pull is never answered, hanging a client that waits on it.
        self.cancel_pull(connection);
        // Cancel and join in-flight scans so no thread outlives the session (each checks its
        // flag between files, so this returns promptly).
        for worker in self.workers.drain(..) {
            worker.cancel.store(true, Ordering::Relaxed);
            let _ = worker.handle.join();
        }
        outcome
    }

    /// Wait on the client and on finished scans at once, so a report is decided on the
    /// thread that owns the session state.
    fn run_loop(&mut self, connection: &Connection) -> SessionOutcome {
        let scans = self.scan_rx.clone();
        loop {
            select! {
                recv(connection.receiver) -> message => {
                    // Connection dropped without a shutdown/exit: a normal end.
                    let Ok(message) = message else {
                        return SessionOutcome::Clean;
                    };
                    match message {
                        Message::Request(request) => {
                            // `Ok(true)` is a clean shutdown; an `Err` means the client
                            // vanished mid-handshake. Either way the session is over.
                            if connection.handle_shutdown(&request).unwrap_or(true) {
                                return SessionOutcome::Clean;
                            }
                            self.handle_request(connection, request);
                        }
                        Message::Notification(notification) => {
                            // A bare `exit` (spec-allowed without a prior `shutdown`) is an
                            // abnormal exit; the normal sequence is consumed above.
                            if notification.method == "exit" {
                                return SessionOutcome::Abnormal;
                            }
                            self.handle_notification(connection, notification);
                        }
                        Message::Response(_) => {}
                    }
                }
                recv(scans) -> result => {
                    // `scan_tx` is a field of `self`, so it outlives this loop.
                    let result = result.expect("the scan channel outlives the loop");
                    self.finish_scan(connection, result);
                }
            }
        }
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
            "workspace/diagnostic" => {
                self.start_workspace_diagnostic(connection, id, &params);
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
                    self.wake(connection);
                }
            }
            "textDocument/didChange" => {
                if let Some(params) = parse::<DidChangeTextDocumentParams>(&params) {
                    self.apply_changes(connection, params);
                    self.wake(connection);
                }
            }
            "textDocument/didClose" => {
                if let Some(params) = parse::<DidCloseTextDocumentParams>(&params) {
                    let uri = params.text_document.uri;
                    self.documents.remove(uri.as_str());
                    self.push(connection, uri, None, Vec::new());
                    self.wake(connection);
                }
            }
            "workspace/didChangeWatchedFiles" => {
                // Source files are watched too, so only a config-named change moved the
                // config; unparsable params keep the conservative reading.
                let config_changed = parse::<DidChangeWatchedFilesParams>(&params)
                    .is_none_or(|watched| {
                        watched
                            .changes
                            .iter()
                            .any(|event| self.is_config_uri(event.uri.as_str()))
                    });
                if config_changed {
                    self.handle_config_change(connection);
                }
                self.wake(connection);
            }
            "workspace/didChangeConfiguration" => {
                if let Some(params) = parse::<DidChangeConfigurationParams>(&params) {
                    self.settings = Settings::from_options(Some(&params.settings));
                    self.handle_config_change(connection);
                    self.wake(connection);
                }
            }
            "$/cancelRequest" => {
                if let Some(params) = parse::<CancelParams>(&params)
                    && self
                        .pull
                        .as_ref()
                        .is_some_and(|pull| pull.id == request_id(params.id))
                {
                    self.cancel_pull(connection);
                }
            }
            _ => {}
        }
    }

    /// Reconstruct the document from incremental (or full-replace) changes, store it, and
    /// publish fresh diagnostics. A ranged change patches the text at that range; a
    /// range-less change replaces the whole document.
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
                    // Ignore a reversed range; offsets are clamped and land on char
                    // boundaries, so the splice itself is always valid.
                    if start <= end {
                        text.replace_range(start..end, &change.text);
                    }
                }
                None => text = change.text,
            }
        }
        self.update(connection, uri, version, text);
    }

    /// Store the latest text/version for `uri`, publish fresh diagnostics, and surface a
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
            // A broken config disables linting silently; tell the user once, then publish
            // empty diagnostics.
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

    /// Push diagnostics, unless the client uses the pull model (a second push would
    /// double-report). The single chokepoint for the push/pull policy.
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

    /// React to a config or watched-file change. A push client gets a re-push of every open
    /// document; a pull client is asked to re-pull via `workspace/diagnostic/refresh` so it
    /// does not keep showing diagnostics computed under the old config. A pull client without
    /// refresh support re-pulls only on its own cadence.
    fn handle_config_change(&mut self, connection: &Connection) {
        // Clear the surfaced-errors set so a still-broken config re-reports once.
        self.reported_errors.clear();
        if self.push_diagnostics {
            self.relint_open_documents(connection);
        } else if self.supports_diagnostic_refresh {
            self.next_refresh_id += 1;
            request_diagnostic_refresh(connection, self.next_refresh_id);
        }
    }

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
        // Recompute for hit-testing (sub-ms/file) rather than caching published diagnostics.
        // A config error here is silent: already surfaced on open/change.
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

    /// The pull-diagnostic report for one document (open buffer if tracked, else disk).
    fn document_diagnostic(
        &self,
        params: &DocumentDiagnosticParams,
    ) -> DocumentDiagnosticReport {
        let uri = params.text_document.uri.as_str();
        let items = self.document_text(uri).map_or_else(Vec::new, |text| {
            // Surface a config failure as an error diagnostic, not an empty (clean) report,
            // so a pull-only client is not misled into thinking the file is fine.
            self.diagnostics_for(uri, &text)
                .unwrap_or_else(|error| vec![config_error_diagnostic(&error)])
        });
        let result_id = analysis::result_id(&items);
        match &result_id {
            Some(id) if params.previous_result_id.as_ref() == Some(id) => {
                DocumentDiagnosticReport::Unchanged(
                    RelatedUnchangedDocumentDiagnosticReport {
                        related_documents: None,
                        unchanged_document_diagnostic_report:
                            UnchangedDocumentDiagnosticReport {
                                result_id: id.clone(),
                            },
                    },
                )
            }
            _ => DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport {
                related_documents: None,
                full_document_diagnostic_report: FullDocumentDiagnosticReport {
                    result_id,
                    items,
                },
            }),
        }
    }

    /// Snapshot open documents by path, so a worker can prefer unsaved buffer content over
    /// on-disk without sharing the live document store.
    fn open_snapshot(&self) -> OpenText {
        self.documents
            .iter()
            .filter_map(|(uri, document)| {
                uri_to_path(uri)
                    .map(|path| (path, (document.text.clone(), document.version)))
            })
            .collect()
    }

    /// Begin a `workspace/diagnostic` pull, closing one still outstanding (a client that
    /// did not wait) with an empty report: it was told nothing, and this supersedes it.
    fn start_workspace_diagnostic(
        &mut self,
        connection: &Connection,
        id: RequestId,
        params: &serde_json::Value,
    ) {
        if let Some(superseded) = self.pull.take() {
            self.cancel_workers();
            let empty = WorkspaceDiagnosticReport { items: Vec::new() };
            respond(connection, superseded.id, empty);
        }
        let params = parse::<WorkspaceDiagnosticParams>(params);
        let token = params.as_ref().and_then(|params| {
            params.partial_result_params.partial_result_token.clone()
        });
        let previous = params
            .map(|params| params.previous_result_ids)
            .unwrap_or_default();
        let by_path = previous_by_path(&previous);
        self.pull = Some(Pull {
            id: id.clone(),
            previous,
            token: token.clone(),
        });
        self.spawn_scan(connection, id, by_path, token);
    }

    /// Scan for the outstanding pull on a background thread, reporting over `scan_tx`.
    fn spawn_scan(
        &mut self,
        connection: &Connection,
        id: RequestId,
        previous: PreviousIds,
        token: Option<ProgressToken>,
    ) {
        // Supersede any earlier scan (its result is dropped as stale), then reap the dead.
        self.cancel_workers();
        self.workers.retain(|worker| !worker.handle.is_finished());
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancel);
        let sink = ReportSink::new(connection.sender.clone(), token);
        let results = self.scan_tx.clone();
        let roots = self.roots.clone();
        let settings = self.settings.clone();
        let encoding = self.encoding;
        let open = self.open_snapshot();
        let revision = self.revision;
        let handle = thread::spawn(move || {
            let scan = workspace_scan(
                &roots, &open, &settings, encoding, &previous, &flag, sink,
            );
            let _ = results.send(ScanResult { id, revision, scan });
        });
        self.workers.push(Worker { cancel, handle });
    }

    /// Answer the outstanding pull, or leave it open.
    fn finish_scan(&mut self, connection: &Connection, result: ScanResult) {
        let Some(pull) = self.pull.take() else {
            return;
        };
        match self.scan_answer(&pull, result) {
            Some(items) => {
                respond(connection, pull.id, WorkspaceDiagnosticReport { items });
            }
            None => self.pull = Some(pull),
        }
    }

    /// The report to answer `pull` with, or `None` to leave it outstanding. A cancelled or
    /// overtaken scan says nothing, and neither does an all-`Unchanged` report (an empty one
    /// included) — answering that would only invite the client's fixed 2 s re-pull, so it is
    /// held open and resumed by [`Self::wake`], the LSP's own suggestion for a request that
    /// "can be long running and is not bound to a specific workspace or document state".
    fn scan_answer(
        &self,
        pull: &Pull,
        result: ScanResult,
    ) -> Option<Vec<WorkspaceDocumentDiagnosticReport>> {
        if pull.id != result.id || result.revision != self.revision {
            return None;
        }
        result
            .scan
            .filter(|outcome| outcome.streamed || !outcome.says_nothing())
            .map(|outcome| outcome.items)
    }

    /// Note that something a lint depends on changed, and rescan for a pull being held
    /// open so the client hears about it now rather than on its own cadence.
    fn wake(&mut self, connection: &Connection) {
        self.revision += 1;
        if let Some(pull) = &self.pull {
            let id = pull.id.clone();
            let previous = previous_by_path(&pull.previous);
            let token = pull.token.clone();
            self.spawn_scan(connection, id, previous, token);
        }
    }

    /// Answer the outstanding pull `RequestCancelled` and stop its scan; unanswered would
    /// hang a client that drains its outstanding requests before exiting.
    fn cancel_pull(&mut self, connection: &Connection) {
        let Some(pull) = self.pull.take() else {
            return;
        };
        self.cancel_workers();
        send(
            connection,
            Message::Response(Response::new_err(
                pull.id,
                ErrorCode::RequestCanceled as i32,
                "workspace diagnostic cancelled".to_string(),
            )),
        );
    }

    fn cancel_workers(&self) {
        for worker in &self.workers {
            worker.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Whether a watched-file event names a config ryl discovers, not a linted source.
    fn is_config_uri(&self, uri: &str) -> bool {
        let Some(path) = uri_to_path(uri) else {
            return false;
        };
        self.settings.config_file.as_ref() == Some(&path)
            || path
                .file_name()
                .and_then(OsStr::to_str)
                .is_some_and(|name| WATCHED_CONFIG_NAMES.contains(&name))
    }

    fn document_text(&self, uri: &str) -> Option<String> {
        if let Some(document) = self.documents.get(uri) {
            return Some(document.text.clone());
        }
        let path = uri_to_path(uri)?;
        crate::decoder::read_file(&path).ok()
    }

    /// Diagnostics for `text` against `uri`'s config; `Err` on a config failure (callers
    /// decide whether to surface it).
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

    /// Resolve the path, config, and source kind for a URI. `Ok(None)` means nothing to lint
    /// (disabled, no config, no rules, ignored, or not a linted kind); `Err` is a config
    /// failure the caller surfaces.
    fn resolve(&self, uri: &str) -> Result<Option<Target>, String> {
        let (path, is_file) = self.uri_path(uri);
        self.resolve_path(path, is_file, true)
    }

    /// As [`Self::resolve`] but from an already-decoded path. `require_rules` gates on the
    /// config enabling at least one rule (true for linting/fixing; false for rename, which
    /// works regardless of lint config).
    fn resolve_path(
        &self,
        path: PathBuf,
        is_file: bool,
        require_rules: bool,
    ) -> Result<Option<Target>, String> {
        resolve_for_path(path, is_file, require_rules, &self.settings)
    }

    /// The source kind of `uri` ignoring whether any rule is enabled (rename works
    /// regardless of lint config). `None` when disabled, ignored, config fails, or the kind
    /// is not YAML/markdown.
    fn document_kind(&self, uri: &str) -> Option<SourceKind> {
        let (path, is_file) = self.uri_path(uri);
        self.resolve_path(path, is_file, false)
            .ok()
            .flatten()
            .map(|target| target.kind)
    }

    /// Decode a URI to `(path, is_file)`. A non-file URI is an untitled buffer with no real
    /// path, anchored at the workspace fallback and linted as YAML.
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

/// Resolve config + source kind for an already-decoded path, layering `settings` onto
/// CLI-precedence discovery. `require_rules` gates on the config enabling at least one rule
/// (true for linting/fixing; false for rename). Free (no `&self`) so a worker thread can
/// call it too.
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
        if context.config.is_file_ignored(&path, &context.base_dir) {
            return Ok(None);
        }
        match context.config.source_kind(&path, &context.base_dir)? {
            Some(kind) => kind,
            None => return Ok(None),
        }
    } else {
        // A non-file buffer has no real path: like stdin without `--stdin-filename`, disable
        // every path-based filter and lint it as YAML.
        context.config.disable_path_based_rule_ignores();
        SourceKind::Yaml
    };
    Ok(Some(Target {
        path,
        context,
        kind,
    }))
}

/// The result ids the client already holds, keyed by path (see [`previous_by_path`]).
pub type PreviousIds = HashMap<PathBuf, (Uri, String)>;

/// Index the client's result ids by path: the URI it echoes back need not be
/// byte-identical to ryl's (percent-encoding and drive-letter case vary), but the path it
/// decodes to is. The URI is kept to address a file the walk has lost.
#[must_use]
pub fn previous_by_path(previous: &[PreviousResultId]) -> PreviousIds {
    previous
        .iter()
        .filter_map(|entry| {
            uri_to_path(entry.uri.as_str())
                .map(|path| (path, (entry.uri.clone(), entry.value.clone())))
        })
        .collect()
}

/// Lint one workspace file for a pull report, preferring the open buffer's text. `None`
/// skips a non-linted/ignored/unreadable file; a config failure becomes an error report,
/// not a silent omit (a pull client would read absence as clean).
fn file_report(
    path: &Path,
    settings: &Settings,
    encoding: PositionEncoding,
    open: &OpenText,
    previous: &PreviousIds,
) -> Option<WorkspaceDocumentDiagnosticReport> {
    let previous_id = previous.get(path).map(|(_, id)| id.as_str());
    let target = match resolve_for_path(path.to_path_buf(), true, true, settings) {
        Ok(Some(target)) => target,
        Ok(None) => return None,
        Err(error) => {
            let items = vec![config_error_diagnostic(&error)];
            return file_pull_report(path, None, items, previous_id);
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
    file_pull_report(path, version, items, previous_id)
}

/// One file's entry in a workspace pull: `Unchanged` while the client's result id matches,
/// a full report when it does not, an empty one to clear a file the client still holds
/// diagnostics for, and `None` for an untracked clean file — nothing to say, which is what
/// lets an idle pull suspend.
fn file_pull_report(
    path: &Path,
    version: Option<i64>,
    items: Vec<Diagnostic>,
    previous_id: Option<&str>,
) -> Option<WorkspaceDocumentDiagnosticReport> {
    let uri = path_to_uri(path);
    match analysis::result_id(&items) {
        Some(id) if previous_id == Some(id.as_str()) => {
            Some(WorkspaceDocumentDiagnosticReport::Unchanged(
                WorkspaceUnchangedDocumentDiagnosticReport {
                    uri,
                    version,
                    unchanged_document_diagnostic_report:
                        UnchangedDocumentDiagnosticReport { result_id: id },
                },
            ))
        }
        None if previous_id.is_none() => None,
        result_id => Some(workspace_report(uri, version, result_id, items)),
    }
}

/// Files linted between two cancellation checks, and the granularity at which a streaming
/// scan hands results to the client; the per-batch [`rayon`] join costs nothing beside it.
const SCAN_BATCH: usize = 64;

/// At most one `$/progress` batch this often, so findings repaint steadily.
const STREAM_INTERVAL: Duration = Duration::from_millis(50);

/// What a completed scan has to say; `streamed` commits the request to being answered.
pub struct ScanOutcome {
    pub items: Vec<WorkspaceDocumentDiagnosticReport>,
    /// Whether part of the report already went out as `$/progress` partial results.
    pub streamed: bool,
}

impl ScanOutcome {
    /// Whether the report leaves the client exactly where it was, so the pull can be held.
    #[must_use]
    pub fn says_nothing(&self) -> bool {
        !self.streamed
            && self.items.iter().all(|item| {
                matches!(item, WorkspaceDocumentDiagnosticReport::Unchanged(_))
            })
    }
}

/// Where a scan's reports go: held for the response, or — given a partial-result token —
/// streamed as `$/progress` batches as they are produced, leaving only the `Unchanged`
/// remainder to answer with, as a streamed report must not be repeated.
pub struct ReportSink {
    stream: Option<Stream>,
    held: Vec<WorkspaceDocumentDiagnosticReport>,
    streamed: bool,
}

struct Stream {
    client: Sender<Message>,
    token: ProgressToken,
    /// `None` until the first batch, which goes out at once so something paints early.
    last_flush: Option<Instant>,
    pending: Vec<WorkspaceDocumentDiagnosticReport>,
}

impl ReportSink {
    /// Holds everything for the response, for a client that offered no token.
    #[must_use]
    pub fn bulk() -> Self {
        Self {
            stream: None,
            held: Vec::new(),
            streamed: false,
        }
    }

    fn new(client: Sender<Message>, token: Option<ProgressToken>) -> Self {
        Self {
            stream: token.map(|token| Stream {
                client,
                token,
                last_flush: None,
                pending: Vec::new(),
            }),
            held: Vec::new(),
            streamed: false,
        }
    }

    /// Route one file's report; `Unchanged` is always held, streaming it says nothing.
    fn push(&mut self, report: WorkspaceDocumentDiagnosticReport) {
        match (&mut self.stream, &report) {
            (Some(stream), WorkspaceDocumentDiagnosticReport::Full(_)) => {
                stream.pending.push(report);
            }
            _ => self.held.push(report),
        }
    }

    /// Send what has accumulated, if enough time has passed since the last batch.
    fn flush_batch(&mut self) {
        if let Some(stream) = &mut self.stream
            && !stream.pending.is_empty()
            && stream
                .last_flush
                .is_none_or(|at| at.elapsed() >= STREAM_INTERVAL)
        {
            stream.send();
            self.streamed = true;
        }
    }

    fn finish(mut self) -> ScanOutcome {
        if let Some(stream) = &mut self.stream
            && !stream.pending.is_empty()
        {
            stream.send();
            self.streamed = true;
        }
        ScanOutcome {
            items: self.held,
            streamed: self.streamed,
        }
    }
}

impl Stream {
    /// A `$/progress` carrying the batch. The spec asks for a `WorkspaceDiagnosticReport`
    /// first and partial results after, but the two share a wire shape, so one form serves.
    fn send(&mut self) {
        let partial = WorkspaceDiagnosticReportPartialResult {
            items: std::mem::take(&mut self.pending),
        };
        let params = serde_json::json!({ "token": self.token, "value": partial });
        let _ = self.client.send(Message::Notification(Notification::new(
            "$/progress".to_string(),
            params,
        )));
        self.last_flush = Some(Instant::now());
    }
}

/// The `workspace/diagnostic` scan: enumerate `*.yaml`/`*.yml` under each root (git-ignore
/// honoured), de-duplicate across roots, then lint them in [`rayon`]-parallel batches routed
/// through `sink`. `None` when `cancel` is set, answering the pull `RequestCancelled`.
pub fn workspace_scan(
    roots: &[PathBuf],
    open: &OpenText,
    settings: &Settings,
    encoding: PositionEncoding,
    previous: &PreviousIds,
    cancel: &AtomicBool,
    mut sink: ReportSink,
) -> Option<ScanOutcome> {
    let mut files = Vec::new();
    let mut seen = HashSet::new();
    for root in roots {
        for path in gather_yaml_from_dir_cancellable(root, cancel)? {
            // De-duplicate so a file reachable from two (e.g. nested) roots is linted once.
            if seen.insert(path.clone()) {
                files.push(path);
            }
        }
    }
    let mut covered: HashSet<&PathBuf> = HashSet::new();
    for batch in files.chunks(SCAN_BATCH) {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let reports: Vec<(&PathBuf, WorkspaceDocumentDiagnosticReport)> = batch
            .par_iter()
            .filter_map(|path| {
                file_report(path, settings, encoding, open, previous)
                    .map(|report| (path, report))
            })
            .collect();
        for (path, report) in reports {
            covered.insert(path);
            sink.push(report);
        }
        sink.flush_batch();
    }
    // A path the client holds an id for that the walk no longer reports was deleted,
    // renamed, or newly ignored: an empty, id-less report clears it and stops the echo.
    for (uri, _) in previous
        .iter()
        .filter(|(path, _)| !covered.contains(path))
        .map(|(_, entry)| entry)
    {
        sink.push(workspace_report(uri.clone(), None, None, Vec::new()));
    }
    Some(sink.finish())
}

fn request_id(id: NumberOrString) -> RequestId {
    match id {
        NumberOrString::Number(number) => RequestId::from(number),
        NumberOrString::String(text) => RequestId::from(text),
    }
}

/// After a rejected handshake, ignore every message until the client ends the session,
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

/// The user-facing message for a config-discovery failure (push and pull paths share it).
fn config_error_text(error: &str) -> String {
    format!("ryl: configuration error, linting is off: {error}")
}

/// An error diagnostic standing in for a config-discovery failure, so a pull request
/// surfaces it rather than reporting the file as clean.
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

fn workspace_report(
    uri: Uri,
    version: Option<i64>,
    result_id: Option<String>,
    items: Vec<Diagnostic>,
) -> WorkspaceDocumentDiagnosticReport {
    WorkspaceDocumentDiagnosticReport::Full(WorkspaceFullDocumentDiagnosticReport {
        uri,
        version,
        full_document_diagnostic_report: FullDocumentDiagnosticReport {
            result_id,
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
    // A send only fails once the client has dropped the connection; nothing to do but let
    // the loop wind down.
    let _ = connection.sender.send(message);
}
