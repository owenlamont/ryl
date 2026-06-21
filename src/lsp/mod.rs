//! The `ryl server` language server: a synchronous protocol adapter over ryl's lint/fix
//! engine, built on `lsp-server` + `lsp-types`. Malformed client input (a bad
//! `initialize`, an unknown request) is handled gracefully rather than panicking; the only
//! `expect` is on serialising ryl's own capabilities, which cannot fail.

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

/// Ask the client to watch ryl's config files so an out-of-editor edit re-lints open
/// documents. Fire-and-forget (the response is ignored).
///
/// Known limitation: files pulled in via a config's `extends:`, and a `configPath` changed
/// after startup, are not (re-)watched; re-open a document to refresh after editing those.
fn register_config_watchers(connection: &Connection, config_file: Option<&Path>) {
    let mut watchers = vec![FileSystemWatcher {
        glob_pattern: GlobPattern::String(
            "**/{ryl.toml,.ryl.toml,pyproject.toml,.yamllint,.yamllint.yaml,\
             .yamllint.yml}"
                .to_string(),
        ),
        kind: None,
    }];
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
    /// In-flight `workspace/diagnostic` scans, each on its own thread so the repo walk never
    /// blocks the message loop. Each carries a cancellation flag the loop flips on
    /// `$/cancelRequest` or at shutdown.
    workers: Vec<Worker>,
}

struct Worker {
    id: RequestId,
    cancel: Arc<AtomicBool>,
    handle: JoinHandle<()>,
}

/// Open-document text/version snapshot, keyed by path, handed to a worker so it can prefer
/// unsaved buffer content over on-disk without sharing the live document store.
pub type OpenText = HashMap<PathBuf, (String, i32)>;

impl Server {
    fn message_loop(mut self, connection: &Connection) -> SessionOutcome {
        let outcome = self.run_loop(connection);
        // Cancel and join in-flight scans so no thread outlives the session (each checks its
        // flag between files, so this returns promptly).
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
                    // `Ok(true)` is a clean shutdown; an `Err` means the client vanished
                    // mid-handshake. Either way the session is over: end gracefully.
                    if connection.handle_shutdown(&request).unwrap_or(true) {
                        return SessionOutcome::Clean;
                    }
                    self.handle_request(connection, request);
                }
                Message::Notification(notification) => {
                    // A bare `exit` (spec-allowed without a prior `shutdown`) is an abnormal
                    // exit; the normal sequence is consumed by `handle_shutdown` above.
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
            "workspace/didChangeWatchedFiles" => self.handle_config_change(connection),
            "workspace/didChangeConfiguration" => {
                if let Some(params) = parse::<DidChangeConfigurationParams>(&params) {
                    self.settings = Settings::from_options(Some(&params.settings));
                    self.handle_config_change(connection);
                }
            }
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
        DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport {
            related_documents: None,
            full_document_diagnostic_report: FullDocumentDiagnosticReport {
                result_id: None,
                items,
            },
        })
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

    /// Start a `workspace/diagnostic` scan on a background thread so the repo walk + parallel
    /// lint never blocks the message loop. The worker answers the request over the connection.
    fn spawn_workspace_diagnostic(&mut self, connection: &Connection, id: RequestId) {
        // Supersede any in-flight scan (it answers RequestCancelled) so rapid pulls (e.g. on
        // every save) do not accumulate concurrent walks, keeping the worker count bounded.
        // Then drop the handles of any already finished.
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

/// Lint one workspace file for a pull report, preferring the open buffer's text. `None`
/// skips a non-linted/ignored/unreadable file; a config failure becomes an error report,
/// not a silent omit (a pull client would read absence as clean).
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
/// honoured), de-duplicate across roots, then lint them in parallel ([`rayon`]). `None` when
/// `cancel` is set, so the worker answers with `RequestCancelled`. `pub` for unit testing.
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
        // The walk is per-entry cancellable (`?` propagates cancellation); the lint pass
        // below is not separately interrupted. The residual mid-`read_file` window is per
        // ryl's threat model (realistic payloads, not a degraded-fs racer).
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

/// The `workspace/diagnostic` response: the report, or a `RequestCancelled` error when the
/// scan was cancelled. `pub` for unit testing.
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
    // A send only fails once the client has dropped the connection; nothing to do but let
    // the loop wind down.
    let _ = connection.sender.send(message);
}
