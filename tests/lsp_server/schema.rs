use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use ryl::config::Env;
use ryl::lsp::schema_preparation::Limits;
use ryl::rules::schema::net::{self, Clock, Dependencies, Error, Transport};
use url::Url;

use super::*;

const ORIGIN: &str = "https://schemas.example.org";
const ROOT: &str = "https://schemas.example.org/root.json?secret=hidden";

#[derive(Default)]
struct Remote {
    calls: AtomicUsize,
    resources: Mutex<BTreeMap<String, Vec<u8>>>,
    gate: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
}
impl Transport for Remote {
    fn get(&self, url: &Url, _: Duration, _: usize) -> Result<Vec<u8>, Error> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        if let Some(gate) = self.gate.lock().unwrap().take() {
            gate.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        self.resources
            .lock()
            .unwrap()
            .get(url.as_str())
            .cloned()
            .ok_or_else(|| Error::transport(url))
    }
}
struct Time;
impl Clock for Time {
    fn unix_seconds(&self) -> u64 {
        1
    }
    fn monotonic(&self) -> Duration {
        Duration::ZERO
    }
}

struct Environment {
    root: std::path::PathBuf,
    remote: Arc<Remote>,
    offline: bool,
    grants: String,
    policy_gate: Mutex<Option<PolicyGate>>,
}
struct PolicyGate {
    started: std::sync::mpsc::SyncSender<()>,
    release: std::sync::mpsc::Receiver<()>,
}
impl Env for Environment {
    fn schema_dependencies(&self) -> Option<Dependencies> {
        Some(Dependencies {
            transport: self.remote.clone(),
            clock: Arc::new(Time),
            budget: Arc::default(),
            filesystem: Arc::new(net::cache_io::SystemFileSystem),
        })
    }
    fn current_dir(&self) -> std::path::PathBuf {
        self.root.clone()
    }
    fn config_dir(&self) -> Option<std::path::PathBuf> {
        Some(self.root.join("user"))
    }
    fn home_dir(&self) -> Option<std::path::PathBuf> {
        Some(self.root.clone())
    }
    fn read_to_string(&self, path: &Path) -> Result<String, String> {
        std::fs::read_to_string(path).map_err(|error| error.to_string())
    }
    fn path_exists(&self, path: &Path) -> bool {
        path.exists()
    }
    fn env_var(&self, key: &str) -> Option<String> {
        if key == "RYL_SCHEMA_ALLOWED_ORIGINS"
            && let Some(gate) = self.policy_gate.lock().unwrap().take()
        {
            gate.started.send(()).unwrap();
            gate.release.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        match key {
            "HOME" => Some(self.root.to_str().unwrap().to_owned()),
            "RYL_OFFLINE" => Some(u8::from(self.offline).to_string()),
            "RYL_SCHEMA_ALLOWED_ORIGINS" => Some(self.grants.clone()),
            "RYL_SCHEMA_CACHE_DIR" => {
                Some(self.root.join("cache").to_str().unwrap().to_owned())
            }
            _ => None,
        }
    }
}

struct Harness {
    dir: TempDir,
    env: Arc<Environment>,
}
impl Harness {
    fn new(grants: &str, offline: bool) -> Self {
        let dir = project(&config(ROOT));
        let remote = Arc::new(Remote::default());
        remote
            .resources
            .lock()
            .unwrap()
            .insert(ROOT.to_owned(), br#"{"type":"integer"}"#.to_vec());
        let env = Arc::new(Environment {
            root: dir.path().to_path_buf(),
            remote,
            offline,
            grants: grants.to_owned(),
            policy_gate: Mutex::new(None),
        });
        Self { dir, env }
    }
    fn client(&self, limits: Limits) -> Client {
        self.client_options(limits, json!({}), Value::Null)
    }
    fn client_options(
        &self,
        limits: Limits,
        capabilities: Value,
        options: Value,
    ) -> Client {
        let (server, client) = Connection::memory();
        let env = self.env.clone();
        let handle = thread::spawn(move || {
            let _ = ryl::lsp::serve_with_schema_env(&server, env, limits);
        });
        let mut client = Client {
            conn: Some(client),
            thread: Some(handle),
            next_id: 0,
        };
        let id = client.request("initialize", json!({"capabilities": capabilities, "initializationOptions": options, "workspaceFolders": [{"uri": file_uri(self.dir.path(), ""), "name": "test"}]}));
        client.response(&id).response_result.unwrap();
        client.notify("initialized", json!({}));
        client
    }
    fn open(&self, client: &Client) -> Uri {
        let uri = file_uri(self.dir.path(), "test.yaml");
        client.notify("textDocument/didOpen", json!({"textDocument":{"uri":uri,"languageId":"yaml","version":1,"text":"text\n"}}));
        uri
    }
    fn calls(&self) -> usize {
        self.env.remote.calls.load(Ordering::Relaxed)
    }
}
fn config(url: &str) -> String {
    format!("[[lint.rules.schema.schemas]]\nfiles = ['*.yaml']\nurl = '{url}'\n")
}
fn prompt(client: &Client) -> Request {
    let request = client.recv_request();
    assert_eq!(request.method, "window/showMessageRequest");
    request
}
fn answer(client: &Client, request: Request, title: Option<&str>) {
    client.send_raw(Message::Response(Response::new_ok(
        request.id,
        title.map(|title| json!({"title":title})),
    )));
}
fn await_schema(client: &Client) -> Vec<Diagnostic> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let message = client
            .conn()
            .receiver
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap();
        if let Message::Notification(note) = message
            && note.method == "textDocument/publishDiagnostics"
        {
            let params: PublishDiagnosticsParams =
                serde_json::from_value(note.params).unwrap();
            if params.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == Some(NumberOrString::String("schema".to_owned()))
            }) {
                return params.diagnostics;
            }
        }
    }
}

#[test]
fn remote_schema_allow_once_names_redacted_urls_and_reuses_session_grant() {
    let h = Harness::new("", false);
    let mut client = h.client(Limits::default());
    h.open(&client);
    let request = prompt(&client);
    assert_eq!(h.calls(), 0);
    assert_eq!(
        request.params["message"],
        "ryl: allow remote schema access? Origin: https://schemas.example.org/ URLs: https://schemas.example.org/root.json?***"
    );
    assert_eq!(
        request.params["actions"],
        json!([{"title":"Allow once"},{"title":format!("Always allow {ORIGIN}")},{"title":"Don't allow"}])
    );
    answer(&client, request, Some("Allow once"));
    assert_eq!(await_schema(&client).len(), 1);
    assert_eq!(h.calls(), 1);
    assert!(
        !h.dir
            .path()
            .join("user/ryl/allowed-schema-origins")
            .exists()
    );
    client.shutdown();
}

#[test]
fn remote_schema_always_allow_persists_and_new_session_uses_user_grant() {
    let h = Harness::new("", false);
    let mut client = h.client(Limits::default());
    h.open(&client);
    answer(
        &client,
        prompt(&client),
        Some(&format!("Always allow {ORIGIN}")),
    );
    await_schema(&client);
    assert_eq!(
        std::fs::read_to_string(h.dir.path().join("user/ryl/allowed-schema-origins"))
            .unwrap(),
        format!("{ORIGIN}\n")
    );
    client.shutdown();
    drop(client);
    let mut second = h.client(Limits::default());
    h.open(&second);
    await_schema(&second);
    assert_eq!(h.calls(), 1);
    second.shutdown();
}

#[test]
fn remote_schema_denial_and_null_do_not_reprompt_on_document_changes() {
    for title in [Some("Don't allow"), None, Some("unrecognised")] {
        let h = Harness::new("", false);
        let mut client = h.client(Limits::default());
        let uri = h.open(&client);
        answer(&client, prompt(&client), title);
        client.assert_alive();
        client.notify("textDocument/didChange", json!({"textDocument":{"uri":uri,"version":2},"contentChanges":[{"text":"other\n"}]}));
        let id = client.request(UNHANDLED_METHOD, Value::Null);
        let (messages, _) = client.messages_until_response(&id);
        assert!(!messages.iter().any(|message| matches!(message, Message::Request(request) if request.method == "window/showMessageRequest")));
        assert_eq!(h.calls(), 0);
        client.shutdown();
    }
}

#[test]
fn remote_schema_code_action_recovers_dismissed_prompt() {
    let h = Harness::new("", false);
    let mut client = h.client(Limits::default());
    let uri = h.open(&client);
    answer(&client, prompt(&client), None);
    let diagnostic = Diagnostic {
        message: "schema origin approval required".to_owned(),
        source: Some("ryl".to_owned()),
        ..Default::default()
    };
    let id = client.request("textDocument/codeAction", json!({"textDocument":{"uri":uri},"range":Range::default(),"context":{"diagnostics":[diagnostic]}}));
    let actions = client.response(&id).response_result.unwrap();
    assert_eq!(actions[0]["title"], format!("Allow {ORIGIN}"));
    let id = client.request("workspace/executeCommand", actions[0]["command"].clone());
    let request = prompt(&client);
    answer(&client, request, Some("Allow once"));
    client.response(&id);
    await_schema(&client);
    client.shutdown();
}

#[test]
fn remote_schema_offline_never_prompts_or_fetches_and_can_reuse_cache() {
    let mut h = Harness::new(ORIGIN, true);
    let mut client = h.client(Limits::default());
    h.open(&client);
    loop {
        let diagnostics = client.diagnostics();
        if diagnostics
            .iter()
            .any(|d| d.message.contains("offline cache"))
        {
            break;
        }
    }
    assert_eq!(h.calls(), 0);
    client.shutdown();
    drop(client);
    Arc::get_mut(&mut h.env).unwrap().offline = false;
    net::Network::from_env(h.env.as_ref(), BTreeMap::new())
        .unwrap()
        .fetch(ROOT, 1024)
        .unwrap();
    Arc::get_mut(&mut h.env).unwrap().offline = true;
    let mut client = h.client(Limits::default());
    h.open(&client);
    await_schema(&client);
    assert_eq!(h.calls(), 1);
    client.shutdown();
}
fn await_error(client: &Client, needle: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let message = client
            .conn()
            .receiver
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap();
        if let Message::Notification(note) = message
            && note.method == "textDocument/publishDiagnostics"
        {
            let report: PublishDiagnosticsParams =
                serde_json::from_value(note.params).unwrap();
            if report
                .diagnostics
                .iter()
                .any(|d| d.message.contains(needle))
            {
                return;
            }
        }
    }
}

#[test]
fn remote_schema_late_answer_after_timeout_does_not_grant() {
    let h = Harness::new("", false);
    let mut client = h.client(Limits {
        prompt: Duration::ZERO,
        ..Limits::default()
    });
    h.open(&client);
    answer(&client, prompt(&client), Some("Allow once"));
    client.assert_alive();
    assert_eq!(h.calls(), 0);
    client.shutdown();
}

#[cfg(unix)]
#[test]
fn remote_schema_always_allow_refuses_symlinked_allowlist() {
    let h = Harness::new("", false);
    let mut client = h.client(Limits::default());
    h.open(&client);
    let request = prompt(&client);
    let target = h.dir.path().join("target");
    std::fs::write(&target, "").unwrap();
    std::fs::create_dir_all(h.dir.path().join("user/ryl")).unwrap();
    std::os::unix::fs::symlink(
        &target,
        h.dir.path().join("user/ryl/allowed-schema-origins"),
    )
    .unwrap();
    answer(&client, request, Some(&format!("Always allow {ORIGIN}")));
    loop {
        if let Message::Notification(note) = client.recv()
            && note.method == "window/showMessage"
            && note.params["message"]
                .as_str()
                .unwrap()
                .contains("refuses symlinks")
        {
            break;
        }
    }
    assert_eq!(std::fs::read_to_string(target).unwrap(), "");
    assert_eq!(h.calls(), 0);
    client.shutdown();
}

#[test]
fn remote_schema_preparation_saturation_is_visible_and_dispatch_stays_responsive() {
    let h = Harness::new(ORIGIN, false);
    let (release, gate) = std::sync::mpsc::channel();
    *h.env.remote.gate.lock().unwrap() = Some(gate);
    let mut client = h.client(Limits {
        pending: 1,
        ..Limits::default()
    });
    h.open(&client);
    let deadline = Instant::now() + Duration::from_secs(5);
    while h.calls() == 0 {
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
    std::fs::create_dir(h.dir.path().join("nested")).unwrap();
    std::fs::write(h.dir.path().join("nested/.ryl.toml"), config(ROOT)).unwrap();
    let uri = file_uri(h.dir.path(), "nested/other.yaml");
    client.notify("textDocument/didOpen", json!({"textDocument":{"uri":uri,"languageId":"yaml","version":1,"text":"text\n"}}));
    await_error(&client, "saturated");
    client.assert_alive();
    release.send(()).unwrap();
    await_schema(&client);
    client.shutdown();
}

#[test]
fn remote_schema_supersession_discards_cancelled_results() {
    let h = Harness::new(ORIGIN, false);
    let (release, gate) = std::sync::mpsc::channel();
    *h.env.remote.gate.lock().unwrap() = Some(gate);
    let mut client = h.client(Limits::default());
    let uri = h.open(&client);
    let deadline = Instant::now() + Duration::from_secs(5);
    while h.calls() == 0 {
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
    client.notify(
        "workspace/didChangeConfiguration",
        json!({"settings":{"configData":TRAILING}}),
    );
    let id = client.request(
        "textDocument/diagnostic",
        json!({"textDocument":{"uri":uri}}),
    );
    assert!(pull_items(client.response(&id)).is_empty());
    release.send(()).unwrap();
    client.assert_alive();
    client.shutdown();
}

#[test]
fn remote_schema_enqueue_deadline_is_visible_without_fetching() {
    let h = Harness::new(ORIGIN, false);
    let mut client = h.client(Limits {
        preparation: Duration::ZERO,
        ..Limits::default()
    });
    h.open(&client);
    await_error(&client, "deadline exceeded");
    assert_eq!(h.calls(), 0);
    client.shutdown();
}

#[test]
fn remote_schema_transitive_origin_needs_its_own_grant() {
    let h = Harness::new(ORIGIN, false);
    let other = "https://other.example.org/type.json";
    h.env.remote.resources.lock().unwrap().insert(
        ROOT.to_owned(),
        json!({"$ref":other}).to_string().into_bytes(),
    );
    h.env
        .remote
        .resources
        .lock()
        .unwrap()
        .insert(other.to_owned(), br#"{"type":"integer"}"#.to_vec());
    let mut client = h.client(Limits::default());
    h.open(&client);
    let request = prompt(&client);
    assert!(request.params["message"].as_str().unwrap().contains(other));
    assert_eq!(h.calls(), 1);
    answer(&client, request, Some("Allow once"));
    await_schema(&client);
    assert_eq!(h.calls(), 2);
    client.shutdown();
}

#[test]
fn local_schema_remote_reference_defers_without_fetching() {
    let h = Harness::new("", false);
    std::fs::write(
        h.dir.path().join("root.json"),
        json!({"$ref":ROOT}).to_string(),
    )
    .unwrap();
    std::fs::write(
        h.dir.path().join(".ryl.toml"),
        "[[lint.rules.schema.schemas]]\nfiles=['*.yaml']\npath='root.json'\n",
    )
    .unwrap();
    let mut client = h.client(Limits::default());
    h.open(&client);
    answer(&client, prompt(&client), Some("Allow once"));
    await_schema(&client);
    assert_eq!(h.calls(), 1);
    client.shutdown();
}

#[test]
fn all_remote_association_origins_are_prompted_before_first_fetch() {
    let h = Harness::new("", false);
    let other = "https://other.example.org/type.json";
    std::fs::write(
        h.dir.path().join(".ryl.toml"),
        format!("{}{}", config(ROOT), config(other)),
    )
    .unwrap();
    h.env
        .remote
        .resources
        .lock()
        .unwrap()
        .insert(other.to_owned(), br#"{"type":"integer"}"#.to_vec());
    let mut client = h.client(Limits::default());
    h.open(&client);
    let first = prompt(&client);
    let second = prompt(&client);
    assert_eq!(h.calls(), 0);
    answer(&client, first, Some("Allow once"));
    answer(&client, second, Some("Allow once"));
    await_schema(&client);
    assert_eq!(h.calls(), 2);
    client.shutdown();
}

#[test]
fn closing_last_document_cancels_remaining_remote_reference_fetches() {
    let h = Harness::new(ORIGIN, false);
    h.env.remote.resources.lock().unwrap().insert(
        ROOT.to_owned(),
        json!({"$ref":"https://schemas.example.org/child.json"})
            .to_string()
            .into_bytes(),
    );
    let (release, gate) = std::sync::mpsc::channel();
    *h.env.remote.gate.lock().unwrap() = Some(gate);
    let mut client = h.client(Limits::default());
    let uri = h.open(&client);
    let deadline = Instant::now() + Duration::from_secs(5);
    while h.calls() == 0 {
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
    client.notify("textDocument/didClose", json!({"textDocument":{"uri":uri}}));
    client.assert_alive();
    release.send(()).unwrap();
    client.shutdown();
    drop(client);
    assert_eq!(h.calls(), 1);
}

#[test]
fn formatting_does_not_prepare_newly_configured_remote_schemas() {
    let h = Harness::new(ORIGIN, false);
    std::fs::write(h.dir.path().join(".ryl.toml"), TRAILING).unwrap();
    let mut client = h.client(Limits::default());
    let uri = h.open(&client);
    client.diagnostics();
    std::fs::write(h.dir.path().join(".ryl.toml"), config(ROOT)).unwrap();
    let id = client.request(
        "textDocument/formatting",
        json!({"textDocument":{"uri":uri},"options":{"tabSize":2,"insertSpaces":true}}),
    );
    assert!(client.response(&id).response_result.is_ok());
    assert_eq!(h.calls(), 0);
    client.shutdown();
}

#[test]
fn remote_schema_prompt_cannot_display_control_bidi_or_query_secrets() {
    let h = Harness::new("", false);
    std::fs::write(
        h.dir.path().join(".ryl.toml"),
        config("https://schemas.example.org/\u{202e}Allow?token=secret"),
    )
    .unwrap();
    let mut client = h.client(Limits::default());
    h.open(&client);
    let request = prompt(&client);
    let message = request.params["message"].as_str().unwrap();
    assert!(!message.contains('\u{202e}'));
    assert!(!message.contains("secret"));
    assert!(message.contains("%E2%80%AEAllow?***"));
    answer(&client, request, None);
    assert_eq!(h.calls(), 0);
    client.shutdown();
}

#[test]
fn workspace_scans_and_document_pulls_share_remote_preparation() {
    let h = Harness::new("", false);
    std::fs::write(h.dir.path().join("test.yaml"), "text\n").unwrap();
    let mut client = h.client_options(Limits::default(), json!({"textDocument":{"diagnostic":{}}, "workspace":{"diagnostics":{"refreshSupport":true}}}), json!({"diagnosticMode":"workspace"}));
    let id = client.request("workspace/diagnostic", json!({"previousResultIds":[]}));
    let mut request = None;
    let mut response = None;
    while request.is_none() || response.is_none() {
        match client.recv() {
            Message::Request(prompt)
                if prompt.method == "window/showMessageRequest" =>
            {
                request = Some(prompt)
            }
            Message::Response(result) if result.id == id => response = Some(result),
            Message::Notification(note)
                if note.method == "textDocument/publishDiagnostics" =>
            {
                assert_eq!(
                    note.params["uri"],
                    file_uri(h.dir.path(), ".ryl.toml").as_str()
                )
            }
            _ => {}
        }
    }
    assert_eq!(h.calls(), 0);
    assert!(response.unwrap().response_result.is_ok());
    answer(&client, request.unwrap(), Some("Allow once"));
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline);
        let report = client.workspace_diagnostic();
        if report.items.iter().any(|item| match item {
            WorkspaceDocumentDiagnosticReport::Full(full) => {
                full.full_document_diagnostic_report.items.iter().any(|d| {
                    d.code == Some(NumberOrString::String("schema".to_owned()))
                })
            }
            WorkspaceDocumentDiagnosticReport::Unchanged(_) => false,
        }) {
            break;
        }
    }
    let uri = file_uri(h.dir.path(), "test.yaml");
    h.open(&client);
    let id = client.request(
        "textDocument/diagnostic",
        json!({"textDocument":{"uri":uri}}),
    );
    assert_eq!(pull_items(client.response(&id)).len(), 1);
    assert_eq!(h.calls(), 1);
    client.shutdown();
}

fn install_policy_gate(
    h: &Harness,
) -> (std::sync::mpsc::Receiver<()>, std::sync::mpsc::Sender<()>) {
    let (started, began) = std::sync::mpsc::sync_channel(1);
    let (release, gate) = std::sync::mpsc::channel();
    *h.env.policy_gate.lock().unwrap() = Some(PolicyGate {
        started,
        release: gate,
    });
    (began, release)
}

#[test]
fn grant_answered_during_running_preflight_retries_with_latest_grants() {
    let h = Harness::new("", false);
    let other = "https://other.example.org/type.json";
    std::fs::write(
        h.dir.path().join(".ryl.toml"),
        format!("{}{}", config(ROOT), config(other)),
    )
    .unwrap();
    h.env
        .remote
        .resources
        .lock()
        .unwrap()
        .insert(other.to_owned(), br#"{"type":"integer"}"#.to_vec());
    let mut client = h.client(Limits::default());
    h.open(&client);
    let first = prompt(&client);
    let second = prompt(&client);
    let (began, release) = install_policy_gate(&h);
    answer(&client, first, Some("Allow once"));
    began.recv_timeout(Duration::from_secs(5)).unwrap();
    answer(&client, second, Some("Allow once"));
    client.assert_alive();
    release.send(()).unwrap();
    await_schema(&client);
    assert_eq!(h.calls(), 2);
    client.shutdown();
}

#[test]
fn existing_waiter_deadline_cancels_blocked_preparation() {
    let h = Harness::new(ORIGIN, false);
    let (began, release) = install_policy_gate(&h);
    let mut client = h.client(Limits {
        preparation: Duration::ZERO,
        ..Limits::default()
    });
    let uri = h.open(&client);
    began.recv_timeout(Duration::from_secs(5)).unwrap();
    client.notify("textDocument/didChange", json!({"textDocument":{"uri":uri,"version":2},"contentChanges":[{"text":"other\n"}]}));
    await_error(&client, "schema preparation deadline exceeded");
    release.send(()).unwrap();
    assert_eq!(h.calls(), 0);
    client.shutdown();
}

#[test]
fn config_reload_rejects_old_result_even_when_config_identity_matches() {
    let h = Harness::new(ORIGIN, false);
    let (release, gate) = std::sync::mpsc::channel();
    *h.env.remote.gate.lock().unwrap() = Some(gate);
    let mut client = h.client(Limits::default());
    h.open(&client);
    let deadline = Instant::now() + Duration::from_secs(5);
    while h.calls() == 0 {
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
    client.notify(
        "workspace/didChangeWatchedFiles",
        changed_config(h.dir.path()),
    );
    client.assert_alive();
    release.send(()).unwrap();
    await_schema(&client);
    assert_eq!(h.calls(), 1);
    client.shutdown();
}

#[test]
fn newer_queued_config_replaces_obsolete_preparations() {
    let h = Harness::new(ORIGIN, false);
    let (release, gate) = std::sync::mpsc::channel();
    *h.env.remote.gate.lock().unwrap() = Some(gate);
    let mut client = h.client(Limits::default());
    let uri = h.open(&client);
    let deadline = Instant::now() + Duration::from_secs(5);
    while h.calls() == 0 {
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
    for version in [2, 3] {
        let url = format!("https://schemas.example.org/{version}.json");
        h.env
            .remote
            .resources
            .lock()
            .unwrap()
            .insert(url.clone(), br#"{"type":"integer"}"#.to_vec());
        std::fs::write(h.dir.path().join(".ryl.toml"), config(&url)).unwrap();
        client.notify("textDocument/didChange", json!({"textDocument":{"uri":uri,"version":version},"contentChanges":[{"text":"other\n"}]}));
        client.assert_alive();
    }
    release.send(()).unwrap();
    await_schema(&client);
    assert_eq!(h.calls(), 2);
    client.shutdown();
}

#[test]
fn config_changes_reprompt_and_clear_association_errors() {
    let h = Harness::new("", false);
    let mut client = h.client(Limits::default());
    let uri = h.open(&client);
    let request = prompt(&client);
    client.send_raw(Message::Response(Response::new_err(
        request.id,
        ErrorCode::RequestCanceled as i32,
        "dismissed".to_owned(),
    )));
    client.notify(
        "workspace/didChangeWatchedFiles",
        changed_config(h.dir.path()),
    );
    let request = prompt(&client);
    answer(&client, request, None);
    client.notify("textDocument/didClose", json!({"textDocument":{"uri":uri}}));
    client.assert_alive();
    assert_eq!(h.calls(), 0);
    client.shutdown();
}

#[test]
fn offline_and_unprepared_sessions_ignore_approval_commands_and_actions() {
    let h = Harness::new("", true);
    let mut client = h.client(Limits::default());
    let uri = file_uri(h.dir.path(), "test.yaml");
    let diagnostic = Diagnostic {
        message: "schema origin approval required".to_owned(),
        source: Some("ryl".to_owned()),
        ..Default::default()
    };
    for active in [false, true] {
        if active {
            h.open(&client);
            await_error(&client, "approval required");
        }
        for params in [
            json!({"command":"foreign"}),
            json!({"command":"ryl.allowSchemaOrigin"}),
            json!({"command":"ryl.allowSchemaOrigin","arguments":[ORIGIN]}),
        ] {
            let id = client.request("workspace/executeCommand", params);
            assert!(client.response(&id).response_result.unwrap().is_null());
        }
        let id = client.request("textDocument/codeAction", json!({"textDocument":{"uri":uri},"range":Range::default(),"context":{"diagnostics":[diagnostic]}}));
        assert!(client.response(&id).response_result.unwrap().is_null());
    }
    assert_eq!(h.calls(), 0);
    client.shutdown();
}

#[test]
fn approval_code_action_can_replace_an_outstanding_prompt() {
    let h = Harness::new("", false);
    let mut client = h.client(Limits::default());
    h.open(&client);
    let old = prompt(&client);
    let id = client.request(
        "workspace/executeCommand",
        json!({"command":"ryl.allowSchemaOrigin","arguments":[ORIGIN]}),
    );
    let new = prompt(&client);
    assert_ne!(old.id, new.id);
    client.response(&id);
    answer(&client, old, Some("Allow once"));
    client.assert_alive();
    assert_eq!(h.calls(), 0);
    answer(&client, new, Some("Allow once"));
    await_schema(&client);
    client.shutdown();
}

#[test]
fn approval_prompt_saturation_is_visible() {
    let h = Harness::new("", false);
    std::fs::write(
        h.dir.path().join(".ryl.toml"),
        format!(
            "{}{}",
            config(ROOT),
            config("https://other.example.org/type.json")
        ),
    )
    .unwrap();
    let mut client = h.client(Limits {
        pending: 1,
        ..Limits::default()
    });
    h.open(&client);
    let first = prompt(&client);
    loop {
        if let Message::Notification(note) = client.recv()
            && note.method == "window/showMessage"
            && note.params["message"]
                .as_str()
                .unwrap()
                .contains("prompts saturated")
        {
            break;
        }
    }
    answer(&client, first, None);
    assert_eq!(h.calls(), 0);
    client.shutdown();
}

#[test]
fn invalid_launcher_grant_is_a_config_error_without_prompting() {
    let h = Harness::new("http://invalid.example.org", false);
    let mut client = h.client(Limits::default());
    h.open(&client);
    await_error(&client, "HTTPS on port 443");
    assert_eq!(h.calls(), 0);
    client.shutdown();
}

#[test]
fn connection_free_workspace_scan_reports_unavailable_remote_preparation() {
    let h = Harness::new("", false);
    std::fs::write(h.dir.path().join("test.yaml"), "text\n").unwrap();
    assert!(format!("{:?}", ryl::lsp::Settings::default()).contains("enable: true"));
    let scan = ryl::lsp::workspace_scan(
        &[h.dir.path().to_path_buf()],
        &Default::default(),
        &ryl::lsp::Settings::default(),
        ryl::lsp::encoding::PositionEncoding::Utf16,
        &Default::default(),
        &std::sync::atomic::AtomicBool::new(false),
        ryl::lsp::ReportSink::bulk(),
    )
    .unwrap();
    assert_eq!(scan.items.len(), 1);
}

#[test]
fn a_denied_origin_is_not_prompted_again_for_another_config() {
    let h = Harness::new("", false);
    let mut client = h.client(Limits::default());
    h.open(&client);
    answer(&client, prompt(&client), None);
    std::fs::create_dir(h.dir.path().join("nested")).unwrap();
    std::fs::write(h.dir.path().join("nested/.ryl.toml"), config(ROOT)).unwrap();
    let uri = file_uri(h.dir.path(), "nested/other.yaml");
    client.notify("textDocument/didOpen", json!({"textDocument":{"uri":uri,"languageId":"yaml","version":1,"text":"text\n"}}));
    loop {
        match client.recv() {
            Message::Notification(note)
                if note.method == "textDocument/publishDiagnostics"
                    && note.params["uri"]
                        == file_uri(h.dir.path(), "nested/.ryl.toml").as_str() =>
            {
                break;
            }
            Message::Request(request) => {
                panic!("unexpected repeated prompt: {request:?}")
            }
            _ => {}
        }
    }
    client.assert_alive();
    assert_eq!(h.calls(), 0);
    client.shutdown();
}

#[test]
fn inline_workspace_configuration_can_request_schemas_but_cannot_grant_origins() {
    let h = Harness::new("", false);
    let mut client = h.client_options(Limits::default(), json!({}), json!({"configData":config(ROOT), "schemaAllowedOrigins":[ORIGIN], "allowedSchemaOrigins":[ORIGIN]}));
    h.open(&client);
    let request = prompt(&client);
    assert_eq!(h.calls(), 0);
    answer(&client, request, Some("Allow once"));
    await_schema(&client);
    client.shutdown();
}

#[test]
fn user_global_yaml_fallback_uses_injected_home_independently_of_host_xdg() {
    let h = Harness::new("", false);
    std::fs::remove_file(h.dir.path().join(".ryl.toml")).unwrap();
    let parent = h.dir.path().join(".config/yamllint");
    std::fs::create_dir_all(&parent).unwrap();
    std::fs::write(parent.join("config"), "rules:\n  trailing-spaces: enable\n")
        .unwrap();
    let mut client = h.client(Limits::default());
    let uri = h.open(&client);
    client.notify("textDocument/didChange", json!({"textDocument":{"uri":uri,"version":2},"contentChanges":[{"text":"text  \n"}]}));
    loop {
        let report = client.publish_params();
        if report.version == Some(2) {
            assert_eq!(report.diagnostics.len(), 1);
            assert_eq!(
                report.diagnostics[0].code,
                Some(NumberOrString::String("trailing-spaces".to_owned()))
            );
            break;
        }
    }
    assert_eq!(h.calls(), 0);
    client.shutdown();
}
