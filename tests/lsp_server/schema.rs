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

#[test]
fn root_properties_and_leading_suffixes_preserve_lsp_schema_selection() {
    let h = Harness::new("", false);
    std::fs::write(h.dir.path().join(".ryl.toml"), "[lint.rules.schema]\ndirectives = true\n[[lint.rules.schema.schemas]]\npath = 'deny.json'\n").unwrap();
    std::fs::write(h.dir.path().join("deny.json"), "false").unwrap();
    let mut client = h.client(Limits::default());
    for (index, (text, expected)) in [
        ("!!map\n# $schema: deny.json\nx: 1\n", "False schema"),
        ("&a\n# $schema: deny.json\nx: 1\n", "False schema"),
        ("!!map\n# $schema: missing.json\nx: 1\n", "missing.json"),
        ("&a\n# $schema: missing.json\nx: 1\n", "missing.json"),
        ("# $schema: none\n...\n---\nx: 1\n", "False schema"),
        ("...\n...\n---\n# $schema: none\nx: 1\n", ""),
    ]
    .into_iter()
    .enumerate()
    {
        let uri = file_uri(h.dir.path(), &format!("case{index}.yaml"));
        client.notify("textDocument/didOpen", json!({"textDocument":{"uri":uri,"languageId":"yaml","version":1,"text":text}}));
        let diagnostics = client.diagnostics();
        if expected.is_empty() {
            assert!(diagnostics.is_empty(), "{text:?}: {diagnostics:?}");
        } else {
            assert_eq!(diagnostics.len(), 1, "{text:?}: {diagnostics:?}");
            assert!(
                diagnostics[0].message.contains(expected),
                "{text:?}: {diagnostics:?}"
            );
        }
    }
    assert_eq!(h.calls(), 0);
    client.shutdown();
}

#[test]
fn directive_buffers_keep_separate_unsaved_selections_and_local_watchers() {
    let h = Harness::new("", false);
    std::fs::write(
        h.dir.path().join(".ryl.toml"),
        "[lint.rules.schema]\ndirectives = true\n",
    )
    .unwrap();
    std::fs::write(h.dir.path().join("accept.schema"), "true").unwrap();
    std::fs::write(h.dir.path().join("reject.schema"), "false").unwrap();
    let mut client = h.client_options(Limits::default(), watching(), Value::Null);
    let first = file_uri(h.dir.path(), "first.yaml");
    let second = file_uri(h.dir.path(), "second.yaml");
    client.notify("textDocument/didOpen", json!({"textDocument":{"uri":first,"languageId":"yaml","version":1,"text":"# $schema: accept.schema\nx: 1\n"}}));
    assert!(client.diagnostics().is_empty());
    schema_registration(&client);
    client.notify("textDocument/didOpen", json!({"textDocument":{"uri":second,"languageId":"yaml","version":1,"text":"# $schema: reject.schema\nx: 1\n"}}));
    assert_eq!(await_schema(&client).len(), 1);
    assert_eq!(schema_registration(&client).as_array().unwrap().len(), 2);
    client.notify("textDocument/didChange", json!({"textDocument":{"uri":first,"version":2},"contentChanges":[{"text":"# $schema: reject.schema\nx: 1\n"}]}));
    assert_eq!(await_schema(&client).len(), 1);
    assert_eq!(schema_registration(&client).as_array().unwrap().len(), 1);
    std::fs::write(h.dir.path().join("reject.schema"), "true").unwrap();
    changed_schema(&client, h.dir.path(), "reject.schema", 2);
    assert!(client.diagnostics().is_empty());
    assert!(client.diagnostics().is_empty());
    assert_eq!(h.calls(), 0);
    client.shutdown();
}

#[test]
fn remote_directives_require_approval_at_the_header_without_config_publication() {
    let h = Harness::new("", false);
    std::fs::write(
        h.dir.path().join(".ryl.toml"),
        "[lint.rules.schema]\ndirectives = true\n",
    )
    .unwrap();
    let mut client = h.client(Limits::default());
    let uri = file_uri(h.dir.path(), "test.yaml");
    client.notify("textDocument/didOpen", json!({"textDocument":{"uri":uri,"languageId":"yaml","version":1,"text":format!("# 🦀\n# $schema: {ROOT}\ntext\n")}}));
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let params = client.publish_params();
        assert_eq!(params.uri, uri);
        if let Some(diagnostic) = params
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.message.contains("approval required"))
        {
            assert_eq!(diagnostic.range.start.line, 1);
            assert!(!diagnostic.message.contains("hidden"));
            break;
        }
        assert!(Instant::now() < deadline);
    }
    assert_eq!(h.calls(), 0);
    approve(&mut client, ORIGIN, false);
    assert_eq!(await_schema(&client).len(), 1);
    assert_eq!(h.calls(), 1);
    client.shutdown();
}

#[test]
fn remote_directive_edits_supersede_late_results() {
    let h = Harness::new(ORIGIN, false);
    std::fs::write(
        h.dir.path().join(".ryl.toml"),
        "[lint.rules.schema]\ndirectives = true\n",
    )
    .unwrap();
    let (release, gate) = std::sync::mpsc::channel();
    *h.env.remote.gate.lock().unwrap() = Some(gate);
    let mut client = h.client(Limits::default());
    let uri = file_uri(h.dir.path(), "test.yaml");
    client.notify("textDocument/didOpen", json!({"textDocument":{"uri":uri,"languageId":"yaml","version":1,"text":format!("# $schema: {ROOT}\ntext\n")}}));
    await_error(&client, "pending");
    client.notify("textDocument/didChange", json!({"textDocument":{"uri":uri,"version":2},"contentChanges":[{"text":"# $schema: none\ntext\n"}]}));
    assert!(client.diagnostics().is_empty());
    release.send(()).unwrap();
    let id = client.request(
        "textDocument/diagnostic",
        json!({"textDocument":{"uri":uri}}),
    );
    assert_eq!(
        client.response(&id).response_result.unwrap()["items"],
        json!([])
    );
    client.shutdown();
}

#[test]
fn directive_formatting_never_fetches_and_offline_never_approves() {
    let h = Harness::new("", true);
    std::fs::write(
        h.dir.path().join(".ryl.toml"),
        "[lint.rules.schema]\ndirectives = true\n",
    )
    .unwrap();
    let mut client = h.client(Limits::default());
    let uri = file_uri(h.dir.path(), "test.yaml");
    client.notify("textDocument/didOpen", json!({"textDocument":{"uri":uri,"languageId":"yaml","version":1,"text":format!("# $schema: {ROOT}\nx:  1\n")}}));
    await_error(&client, "approval required");
    let id = client.request(
        "textDocument/formatting",
        json!({"textDocument":{"uri":uri},"options":{"tabSize":2,"insertSpaces":true}}),
    );
    client.response(&id).response_result.unwrap();
    assert_eq!(h.calls(), 0);
    client.shutdown();
}

#[test]
fn closing_a_directive_buffer_releases_its_selection_without_watch_support() {
    let h = Harness::new(ORIGIN, false);
    std::fs::write(
        h.dir.path().join(".ryl.toml"),
        "[lint.rules.schema]\ndirectives = true\n",
    )
    .unwrap();
    let mut client = h.client(Limits::default());
    let uri = file_uri(h.dir.path(), "test.yaml");
    client.notify("textDocument/didOpen", json!({"textDocument":{"uri":uri,"languageId":"yaml","version":1,"text":format!("# $schema: {ROOT}\ntext\n")}}));
    assert_eq!(await_schema(&client).len(), 1);
    client.notify("textDocument/didClose", json!({"textDocument":{"uri":uri}}));
    assert!(client.diagnostics().is_empty());
    client.notify("textDocument/didOpen", json!({"textDocument":{"uri":uri,"languageId":"yaml","version":1,"text":"# $schema: none\ntext\n"}}));
    assert!(client.diagnostics().is_empty());
    client.shutdown();
}

#[test]
fn queued_directive_selections_are_discarded_on_edit_close_and_untracked_pulls() {
    let h = Harness::new(ORIGIN, false);
    std::fs::write(
        h.dir.path().join(".ryl.toml"),
        "[lint.rules.schema]\ndirectives = true\n",
    )
    .unwrap();
    std::fs::write(
        h.dir.path().join("closed.yaml"),
        format!("# $schema: {ORIGIN}/closed.json\ntext\n"),
    )
    .unwrap();
    let (began, release) = install_policy_gate(&h);
    let mut client = h.client_options(Limits::default(), watching(), Value::Null);
    let first = file_uri(h.dir.path(), "first.yaml");
    client.notify("textDocument/didOpen", json!({"textDocument":{"uri":first,"languageId":"yaml","version":1,"text":format!("# $schema: {ROOT}\ntext\n")}}));
    await_error(&client, "pending");
    began.recv_timeout(Duration::from_secs(5)).unwrap();
    let closed = file_uri(h.dir.path(), "closed.yaml");
    let id = client.request(
        "textDocument/diagnostic",
        json!({"textDocument":{"uri":closed}}),
    );
    assert!(
        pull_items(client.response(&id))[0]
            .message
            .contains("pending")
    );
    let second = file_uri(h.dir.path(), "second.yaml");
    let third = file_uri(h.dir.path(), "third.yaml");
    for uri in [&second, &third] {
        client.notify("textDocument/didOpen", json!({"textDocument":{"uri":uri,"languageId":"yaml","version":1,"text":format!("# $schema: {ORIGIN}/queued.json\ntext\n")}}));
        await_error(&client, "pending");
    }
    client.notify("textDocument/didChange", json!({"textDocument":{"uri":second,"version":2},"contentChanges":[{"text":"# $schema: none\ntext\n"}]}));
    assert!(client.diagnostics().is_empty());
    for uri in [&third, &first, &second] {
        client.notify("textDocument/didClose", json!({"textDocument":{"uri":uri}}));
        assert!(client.diagnostics().is_empty());
    }
    release.send(()).unwrap();
    client.shutdown();
    assert_eq!(h.calls(), 0);
}

#[test]
fn a_stale_workspace_snapshot_cannot_restore_an_unsaved_directive() {
    for reopen in [false, true] {
        let h = Harness::new(ORIGIN, false);
        std::fs::write(
            h.dir.path().join(".ryl.toml"),
            "[lint.rules.schema]\ndirectives = true\n",
        )
        .unwrap();
        std::fs::write(h.dir.path().join("test.yaml"), "text\n").unwrap();
        let mut client =
            h.client_options(Limits::default(), watching(), workspace_mode());
        let uri = file_uri(h.dir.path(), "test.yaml");
        client.notify("textDocument/didOpen", json!({"textDocument":{"uri":uri,"languageId":"yaml","version":1,"text":format!("# $schema: {ROOT}\ntext\n")}}));
        await_schema(&client);
        let (started, began) = std::sync::mpsc::sync_channel(1);
        let (release, gate) = std::sync::mpsc::channel();
        *h.env.read_gate.lock().unwrap() = Some(PolicyGate {
            started,
            release: gate,
        });
        let id = client.request(
            "workspace/diagnostic",
            json!({"previousResultIds":[{"uri":uri,"value":"old"}]}),
        );
        began.recv_timeout(Duration::from_secs(5)).unwrap();
        if reopen {
            client.notify("textDocument/didClose", json!({"textDocument":{"uri":uri}}));
            client.notify("textDocument/didOpen", json!({"textDocument":{"uri":uri,"languageId":"yaml","version":1,"text":"# $schema: none\ntext\n"}}));
        } else {
            client.notify("textDocument/didChange", json!({"textDocument":{"uri":uri,"version":2},"contentChanges":[{"text":"# $schema: none\ntext\n"}]}));
        }
        let barrier = client.request(UNHANDLED_METHOD, Value::Null);
        let (messages, _) = client.messages_until_response(&barrier);
        release.send(()).unwrap();
        if !messages.iter().any(
        |message| matches!(message,Message::Response(response) if response.id == id),
    ) {
        client.response(&id).response_result.unwrap();
    }
        let id = client.request(
            "textDocument/diagnostic",
            json!({"textDocument":{"uri":uri}}),
        );
        assert_eq!(
            client.response(&id).response_result.unwrap()["items"],
            json!([])
        );
        client.shutdown();
    }
}

#[cfg(unix)]
#[test]
fn directive_watchers_refuse_dangling_symlinks_and_non_directory_components() {
    let h = Harness::new("", false);
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(
        h.dir.path().join(".ryl.toml"),
        "[lint.rules.schema]\ndirectives = true\n",
    )
    .unwrap();
    std::fs::write(h.dir.path().join("plain-file"), "true").unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("missing"),
        h.dir.path().join("escape"),
    )
    .unwrap();
    let mut client = h.client_options(Limits::default(), watching(), Value::Null);
    let uri = file_uri(h.dir.path(), "test.yaml");
    for (version, value) in [
        (1, "escape/schema.json"),
        (2, "plain-file/schema.json"),
        (3, "missing/../../escaped.json"),
        (4, "../outside-schema.json"),
    ] {
        client.notify("textDocument/didOpen", json!({"textDocument":{"uri":uri,"languageId":"yaml","version":version,"text":format!("# $schema: {value}\nx: 1\n")}}));
        await_error(&client, "schema");
        let id = client.request(
            "textDocument/diagnostic",
            json!({"textDocument":{"uri":uri}}),
        );
        assert_eq!(
            client.response(&id).response_result.unwrap()["items"][0]["range"]["start"]
                ["line"],
            0
        );
    }
    assert_eq!(h.calls(), 0);
    client.shutdown();
}

#[cfg(unix)]
#[test]
fn confined_targets_are_watched_without_registering_an_outside_alias() {
    let h = Harness::new("", false);
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(
        h.dir.path().join(".ryl.toml"),
        "[lint.rules.schema]\ndirectives = true\n",
    )
    .unwrap();
    std::fs::write(h.dir.path().join("accept.schema"), "true").unwrap();
    std::os::unix::fs::symlink(h.dir.path(), outside.path().join("link")).unwrap();
    let value = format!(
        "../{}/link/accept.schema",
        outside.path().file_name().unwrap().to_str().unwrap()
    );
    let mut client = h.client_options(Limits::default(), watching(), Value::Null);
    let uri = file_uri(h.dir.path(), "test.yaml");
    client.notify("textDocument/didOpen", json!({"textDocument":{"uri":uri,"languageId":"yaml","version":1,"text":format!("# $schema: {value}\nx: 1\n")}}));
    assert!(client.diagnostics().is_empty());
    assert_eq!(
        schema_registration(&client),
        json!([{"globPattern":globset::escape(&h.dir.path().join("accept.schema").to_string_lossy().replace('\\', "/"))}])
    );
    client.shutdown();
}

#[test]
fn directive_preparations_share_bounded_retention_and_release_closed_buffers() {
    for (limits, error) in [
        (
            Limits {
                retained_bytes: 6,
                ..Limits::default()
            },
            "size limit",
        ),
        (
            Limits {
                retained_resources: 1,
                ..Limits::default()
            },
            "resource limit",
        ),
        (
            Limits {
                document_selections: 1,
                ..Limits::default()
            },
            "resource limit",
        ),
    ] {
        let h = Harness::new(ORIGIN, false);
        std::fs::write(
            h.dir.path().join(".ryl.toml"),
            "[lint.rules.schema]\ndirectives = true\n",
        )
        .unwrap();
        let other = format!("{ORIGIN}/other.json");
        for url in [ROOT, other.as_str()] {
            h.env
                .remote
                .resources
                .lock()
                .unwrap()
                .insert(url.to_owned(), b"true".to_vec());
        }
        let mut client = h.client(limits);
        let first = file_uri(h.dir.path(), "first.yaml");
        let second = file_uri(h.dir.path(), "second.yaml");
        client.notify("textDocument/didOpen", json!({"textDocument":{"uri":first,"languageId":"yaml","version":1,"text":format!("# $schema: {ROOT}\ntext\n")}}));
        await_clean(&client);
        client.notify("textDocument/didOpen", json!({"textDocument":{"uri":second,"languageId":"yaml","version":1,"text":format!("# $schema: {other}\ntext\n")}}));
        await_error(&client, error);
        client.notify(
            "textDocument/didClose",
            json!({"textDocument":{"uri":first}}),
        );
        assert!(client.diagnostics().is_empty());
        client.notify("textDocument/didChange", json!({"textDocument":{"uri":second,"version":2},"contentChanges":[{"text":format!("# $schema: {other}\ntext\n")}]}));
        await_clean(&client);
        client.shutdown();
    }
}

#[test]
fn directive_errors_preserve_bom_untitled_and_markdown_positions() {
    let h = Harness::new("", false);
    std::fs::write(
        h.dir.path().join(".ryl.toml"),
        "[files]\nmarkdown = ['*.md']\n[lint.rules.schema]\ndirectives = true\n",
    )
    .unwrap();
    let mut client = h.client(Limits::default());
    let uri = "untitled:directive-test";
    client.notify("textDocument/didOpen", json!({"textDocument":{"uri":uri,"languageId":"yaml","version":1,"text":"\u{feff}# $schema: missing.json\nx: 1\n"}}));
    let diagnostics = client.diagnostics();
    assert!(diagnostics[0].message.contains("filename"));
    assert_eq!(diagnostics[0].range.start, Position::new(0, 1));
    client.notify("textDocument/didChange", json!({"textDocument":{"uri":uri,"version":2},"contentChanges":[{"text":"# $schema: none\nx: 1\n"}]}));
    assert!(client.diagnostics().is_empty());
    client.notify("textDocument/didClose", json!({"textDocument":{"uri":uri}}));
    assert!(client.diagnostics().is_empty());
    let uri = file_uri(h.dir.path(), "guide.md");
    client.notify("textDocument/didOpen", json!({"textDocument":{"uri":uri,"languageId":"markdown","version":1,"text":"🦀 heading\n\n> ```yaml\n> # $schema: missing.json\n> x: 1\n> ```\n"}}));
    assert_eq!(client.diagnostics()[0].range.start, Position::new(3, 2));
    std::fs::write(h.dir.path().join("closed.yaml"), "# $schema: none\nx: 1\n")
        .unwrap();
    let id = client.request(
        "textDocument/diagnostic",
        json!({"textDocument":{"uri":file_uri(h.dir.path(), "closed.yaml")}}),
    );
    assert_eq!(
        client.response(&id).response_result.unwrap()["items"],
        json!([])
    );
    client.shutdown();
}

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
    config_gate: Mutex<Option<PolicyGate>>,
    read_gate: Mutex<Option<PolicyGate>>,
    config_unavailable: std::sync::atomic::AtomicBool,
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
        if let Some(gate) = self.config_gate.lock().unwrap().take() {
            gate.started.send(()).unwrap();
            gate.release.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        (!self.config_unavailable.load(Ordering::Relaxed))
            .then(|| self.root.join("user"))
    }
    fn home_dir(&self) -> Option<std::path::PathBuf> {
        Some(self.root.clone())
    }
    fn read_to_string(&self, path: &Path) -> Result<String, String> {
        let gate = self.read_gate.lock().unwrap().take();
        if let Some(gate) = gate {
            gate.started.send(()).unwrap();
            gate.release.recv_timeout(Duration::from_secs(5)).unwrap();
        }
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
            config_gate: Mutex::new(None),
            read_gate: Mutex::new(None),
            config_unavailable: std::sync::atomic::AtomicBool::new(false),
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
fn approve(client: &mut Client, origin: &str, persist: bool) {
    let id = client.request(
        "workspace/executeCommand",
        json!({"command":"ryl.allowSchemaOrigin","arguments":[origin, persist]}),
    );
    client.response(&id).response_result.unwrap();
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

fn await_clean(client: &Client) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !client.diagnostics().is_empty() {
        assert!(
            Instant::now() < deadline,
            "directive preparation did not settle"
        );
    }
}

fn schema_registration(client: &Client) -> Value {
    loop {
        let request = client.recv_request();
        if request.method == "client/registerCapability"
            && request.params["registrations"][0]["id"] == "ryl-watch-schemas"
        {
            return request.params["registrations"][0]["registerOptions"]["watchers"]
                .clone();
        }
    }
}

fn watching() -> Value {
    json!({"workspace":{"didChangeWatchedFiles":{"dynamicRegistration":true}}})
}

fn local_config(path: &str) -> String {
    format!("[[lint.rules.schema.schemas]]\nfiles = ['*.yaml']\npath = '{path}'\n")
}

fn changed_schema(client: &Client, dir: &Path, path: &str, kind: u8) {
    client.notify(
        "workspace/didChangeWatchedFiles",
        json!({"changes":[{"uri":file_uri(dir,path),"type":kind}]}),
    );
}

#[test]
fn local_schema_watchers_reload_roots_references_missing_files_and_deletions() {
    for reference in [false, true] {
        let h = Harness::new("", false);
        let root = h.dir.path().join("root.schema");
        std::fs::write(
            h.dir.path().join(".ryl.toml"),
            local_config("./root.schema"),
        )
        .unwrap();
        if reference {
            std::fs::write(&root, r#"{"$ref":"child.data"}"#).unwrap();
        }
        let changed = if reference {
            "child.data"
        } else {
            "root.schema"
        };
        let mut client = h.client_options(Limits::default(), watching(), Value::Null);
        h.open(&client);
        await_error(&client, "schema");
        let watchers = schema_registration(&client);
        let mut expected = vec![
            json!({"globPattern":globset::escape(&root.to_string_lossy().replace('\\',"/"))}),
        ];
        if reference {
            expected.insert(0,json!({"globPattern":globset::escape(&h.dir.path().join(changed).to_string_lossy().replace('\\',"/"))}));
        }
        assert_eq!(watchers, json!(expected));
        std::fs::write(h.dir.path().join(changed), r#"{"type":"string"}"#).unwrap();
        changed_schema(&client, h.dir.path(), changed, 1);
        assert!(client.diagnostics().is_empty());
        std::fs::write(h.dir.path().join(changed), r#"{"type":"integer"}"#).unwrap();
        changed_schema(&client, h.dir.path(), changed, 2);
        assert_eq!(await_schema(&client).len(), 1);
        std::fs::remove_file(h.dir.path().join(changed)).unwrap();
        changed_schema(&client, h.dir.path(), changed, 3);
        await_error(&client, "schema");
        std::fs::write(h.dir.path().join(changed), r#"{"type":"string"}"#).unwrap();
        changed_schema(&client, h.dir.path(), changed, 1);
        assert!(client.diagnostics().is_empty());
        assert_eq!(h.calls(), 0);
        client.shutdown();
    }
}

#[test]
fn symlinked_schema_targets_are_watched_and_revalidated() {
    for reference in [false, true] {
        let h = Harness::new("", false);
        let target = h.dir.path().join("target.data");
        let alias = h.dir.path().join("alias.data");
        std::fs::write(&target, r#"{"type":"string"}"#).unwrap();
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&target, &alias);
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_file(&target, &alias);
        if linked.is_err() {
            return;
        }
        std::fs::write(
            h.dir.path().join(".ryl.toml"),
            local_config(if reference { "root.data" } else { "alias.data" }),
        )
        .unwrap();
        if reference {
            std::fs::write(h.dir.path().join("root.data"), r#"{"$ref":"alias.data"}"#)
                .unwrap();
        }
        let mut client = h.client_options(Limits::default(), watching(), Value::Null);
        h.open(&client);
        assert!(client.diagnostics().is_empty());
        let watchers = schema_registration(&client);
        let canonical = Url::from_file_path(target.canonicalize().unwrap())
            .unwrap()
            .to_file_path()
            .unwrap();
        for path in [&alias, &canonical] {
            assert!(watchers.as_array().unwrap().contains(&json!({"globPattern": globset::escape(&path.to_string_lossy().replace('\\', "/"))})));
        }
        std::fs::write(&target, r#"{"type":"integer"}"#).unwrap();
        changed_schema(&client, h.dir.path(), "target.data", 2);
        assert_eq!(await_schema(&client).len(), 1);
        std::fs::remove_file(&alias).unwrap();
        std::fs::write(&alias, r#"{"type":"string"}"#).unwrap();
        changed_schema(&client, h.dir.path(), "alias.data", 2);
        assert!(client.diagnostics().is_empty());
        client.shutdown();
    }
}

#[test]
fn closing_documents_removes_only_unused_schema_watchers() {
    for mixed in [false, true] {
        for workspace in [false, true] {
            let h = Harness::new(ORIGIN, false);
            let mut config_text = local_config("root.data");
            if mixed {
                config_text = config_text.replace("*.yaml", "local.yaml");
                config_text.push_str(&config(ROOT));
            }
            std::fs::write(h.dir.path().join(".ryl.toml"), config_text).unwrap();
            std::fs::write(h.dir.path().join("root.data"), "true").unwrap();
            std::fs::write(h.dir.path().join("test.yaml"), "text\n").unwrap();
            let mut client = h.client_options(
                Limits::default(),
                watching(),
                if workspace {
                    workspace_mode()
                } else {
                    Value::Null
                },
            );
            let first = h.open(&client);
            schema_registration(&client);
            if mixed {
                await_schema(&client);
            }
            let second = file_uri(h.dir.path(), "second.yaml");
            client.notify("textDocument/didOpen", json!({"textDocument":{"uri":second,"languageId":"yaml","version":1,"text":"text\n"}}));
            let barrier = client.request(UNHANDLED_METHOD, Value::Null);
            client.messages_until_response(&barrier);
            for (uri, last) in [(first, false), (second, true)] {
                client.notify(
                    "textDocument/didClose",
                    json!({"textDocument":{"uri":uri}}),
                );
                let barrier = client.request(UNHANDLED_METHOD, Value::Null);
                let (messages, _) = client.messages_until_response(&barrier);
                let unregistered = messages.iter().any(|message| matches!(message, Message::Request(request) if request.method == "client/unregisterCapability" && request.params["unregisterations"][0]["id"] == "ryl-watch-schemas"));
                assert_eq!(
                    unregistered,
                    last && !workspace,
                    "mixed={mixed}, workspace={workspace}, last={last}"
                );
            }
            let id = client.request(
                "textDocument/diagnostic",
                json!({"textDocument":{"uri":file_uri(h.dir.path(), "test.yaml")}}),
            );
            client.response(&id).response_result.unwrap();
            let barrier = client.request(UNHANDLED_METHOD, Value::Null);
            let (messages, _) = client.messages_until_response(&barrier);
            assert!(!messages.iter().any(|message| matches!(message, Message::Request(request) if request.params["registrations"][0]["id"] == "ryl-watch-schemas")));
            client.shutdown();
        }
    }
}

#[test]
fn local_schema_watching_refreshes_pull_diagnostics() {
    let h = Harness::new("", false);
    std::fs::write(h.dir.path().join(".ryl.toml"), local_config("root.data")).unwrap();
    std::fs::write(h.dir.path().join("root.data"), r#"{"type":"string"}"#).unwrap();
    let mut capabilities = watching();
    capabilities["textDocument"] = json!({"diagnostic":{}});
    capabilities["workspace"]["diagnostics"] = json!({"refreshSupport":true});
    let mut client = h.client_options(Limits::default(), capabilities, Value::Null);
    let uri = h.open(&client);
    schema_registration(&client);
    let id = client.request(
        "textDocument/diagnostic",
        json!({"textDocument":{"uri":uri}}),
    );
    assert!(pull_items(client.response(&id)).is_empty());
    std::fs::write(h.dir.path().join("root.data"), r#"{"type":"integer"}"#).unwrap();
    changed_schema(&client, h.dir.path(), "root.data", 2);
    assert_eq!(client.recv_request().method, "workspace/diagnostic/refresh");
    let id = client.request(
        "textDocument/diagnostic",
        json!({"textDocument":{"uri":uri}}),
    );
    assert_eq!(pull_items(client.response(&id)).len(), 1);
    client.shutdown();
}

#[test]
fn schema_watchers_require_associations_and_dynamic_registration() {
    for dynamic in [false, true] {
        let h = Harness::new("", false);
        std::fs::write(
            h.dir.path().join(".ryl.toml"),
            if dynamic {
                TRAILING.to_owned()
            } else {
                local_config("root.data")
            },
        )
        .unwrap();
        std::fs::write(h.dir.path().join("root.data"), "true").unwrap();
        let mut client = h.client_options(
            Limits::default(),
            if dynamic { watching() } else { json!({}) },
            Value::Null,
        );
        h.open(&client);
        let id = client.request(UNHANDLED_METHOD, Value::Null);
        let (messages, _) = client.messages_until_response(&id);
        assert!(!messages.iter().any(|message|matches!(message,Message::Request(request) if request.params["registrations"][0]["id"] == "ryl-watch-schemas")));
        assert_eq!(h.calls(), 0);
        if !dynamic {
            std::fs::write(h.dir.path().join("root.data"), "false").unwrap();
            client.notify(
                "textDocument/didClose",
                json!({"textDocument":{"uri":file_uri(h.dir.path(),"test.yaml")}}),
            );
            h.open(&client);
            await_schema(&client);
        }
        client.shutdown();
    }
}

#[test]
fn changed_references_and_config_paths_replace_watchers() {
    let h = Harness::new("", false);
    let root = h.dir.path().join("root.data");
    std::fs::write(h.dir.path().join(".ryl.toml"), local_config("root.data")).unwrap();
    std::fs::write(&root, r#"{"$ref":"first.data"}"#).unwrap();
    std::fs::write(h.dir.path().join("first.data"), "true").unwrap();
    std::fs::write(h.dir.path().join("second.data"), "false").unwrap();
    let mut client = h.client_options(Limits::default(), watching(), Value::Null);
    h.open(&client);
    let first = schema_registration(&client);
    assert_eq!(first.as_array().unwrap().len(), 2);
    std::fs::write(&root, r#"{"$ref":"second.data"}"#).unwrap();
    changed_schema(&client, h.dir.path(), "root.data", 2);
    let second = schema_registration(&client);
    assert!(second.to_string().contains("second.data"));
    assert!(!second.to_string().contains("first.data"));
    let alternate = h.dir.path().join("custom.toml");
    std::fs::write(&alternate, TRAILING).unwrap();
    client.notify(
        "workspace/didChangeConfiguration",
        json!({"settings":{"configPath":alternate}}),
    );
    let id = client.request(UNHANDLED_METHOD, Value::Null);
    let (messages, _) = client.messages_until_response(&id);
    assert!(messages.iter().any(|message|matches!(message,Message::Request(request) if request.method == "client/unregisterCapability" && request.params["unregisterations"][0]["id"] == "ryl-watch-schemas")));
    assert!(messages.iter().any(|message|matches!(message,Message::Request(request) if request.method == "client/registerCapability" && request.params["registrations"][0]["registerOptions"]["watchers"].as_array().is_some_and(|watchers|watchers.iter().any(|watcher|watcher["globPattern"] == alternate.to_string_lossy().replace('\\',"/"))))));
    assert_eq!(h.calls(), 0);
    client.shutdown();
}

#[test]
fn obsolete_workspace_discovery_cannot_restore_schema_watchers() {
    for schemas in [false, true] {
        let h = Harness::new("", false);
        let config_path = h.dir.path().join(".ryl.toml");
        std::fs::write(&config_path, local_config("root.data")).unwrap();
        std::fs::write(h.dir.path().join("root.data"), "false").unwrap();
        std::fs::write(h.dir.path().join("test.yaml"), "text\n").unwrap();
        let mut client =
            h.client_options(Limits::default(), watching(), workspace_mode());
        let uri = h.open(&client);
        schema_registration(&client);
        if !schemas {
            std::fs::write(&config_path, TRAILING).unwrap();
        }
        let (started, began) = std::sync::mpsc::sync_channel(1);
        let (release, gate) = std::sync::mpsc::channel();
        *h.env.read_gate.lock().unwrap() = Some(PolicyGate {
            started,
            release: gate,
        });
        let id = client.request(
            "workspace/diagnostic",
            json!({"previousResultIds":[{"uri":uri,"value":"old"}]}),
        );
        began.recv_timeout(Duration::from_secs(5)).unwrap();
        client.notify(
            "workspace/didChangeConfiguration",
            json!({"settings":{"enable":false}}),
        );
        let barrier = client.request(UNHANDLED_METHOD, Value::Null);
        let (messages, _) = client.messages_until_response(&barrier);
        release.send(()).unwrap();
        if !messages.iter().any(|message|matches!(message,Message::Response(response) if response.id == id)) {
            client.response(&id).response_result.unwrap();
        }
        client.shutdown();
    }
}

#[test]
fn local_schema_watchers_reject_absolute_roots() {
    let h = Harness::new("", false);
    std::fs::write(
        h.dir.path().join(".ryl.toml"),
        local_config("/outside.schema"),
    )
    .unwrap();
    let mut client = h.client_options(Limits::default(), watching(), Value::Null);
    h.open(&client);
    await_error(&client, "relative");
    client.shutdown();
}

#[cfg(unix)]
#[test]
fn schema_reload_rechecks_symlink_confinement() {
    let h = Harness::new("", false);
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("schema"), "true").unwrap();
    let root = h.dir.path().join("root.data");
    std::fs::write(h.dir.path().join(".ryl.toml"), local_config("root.data")).unwrap();
    std::fs::write(&root, "true").unwrap();
    let mut client = h.client_options(Limits::default(), watching(), Value::Null);
    h.open(&client);
    schema_registration(&client);
    std::fs::remove_file(&root).unwrap();
    std::os::unix::fs::symlink(outside.path().join("schema"), &root).unwrap();
    changed_schema(&client, h.dir.path(), "root.data", 2);
    await_error(&client, "escapes config base");
    client.shutdown();
}

#[test]
fn watched_local_schema_changes_reload_mixed_and_local_only_configs() {
    for mixed in [false, true] {
        let h = Harness::new("", false);
        let local = h.dir.path().join("local.json");
        std::fs::write(&local, r#"{"type":"string"}"#).unwrap();
        let mut config_text = "[[lint.rules.schema.schemas]]\nfiles = ['local.yaml']\npath = 'local.json'\n".to_owned();
        if mixed {
            config_text.push_str(&config(ROOT).replace("*.yaml", "remote.yaml"));
        }
        std::fs::write(h.dir.path().join(".ryl.toml"), config_text).unwrap();
        let mut client = h.client(Limits::default());
        if mixed {
            client.did_open(file_uri(h.dir.path(), "remote.yaml"), "text\n");
            await_error(&client, "approval required");
            approve(&mut client, ORIGIN, false);
            await_schema(&client);
        }
        let uri = file_uri(h.dir.path(), "local.yaml");
        client.did_open(uri.clone(), "text\n");
        let params = json!({"textDocument":{"uri":uri}});
        let id = client.request("textDocument/diagnostic", params.clone());
        assert!(pull_items(client.response(&id)).is_empty());

        std::fs::write(&local, r#"{"type":"integer"}"#).unwrap();
        client.notify(
            "workspace/didChangeWatchedFiles",
            json!({"changes":[{"uri":file_uri(h.dir.path(), "local.json"),"type":2}]}),
        );
        let id = client.request("textDocument/diagnostic", params.clone());
        let (mut messages, response) = client.messages_until_response(&id);
        let mut items = pull_items(response);
        if items
            .iter()
            .any(|item| item.message.contains("preparation pending"))
        {
            await_schema(&client);
            let id = client.request("textDocument/diagnostic", params);
            let (ready_messages, response) = client.messages_until_response(&id);
            messages.extend(ready_messages);
            items = pull_items(response);
        }
        assert!(
            items
                .iter()
                .any(|item| item.code
                    == Some(NumberOrString::String("schema".to_owned()))),
            "edited local schema must reject text (mixed={mixed}): {items:?}"
        );
        assert!(!messages.iter().any(|message| matches!(message, Message::Request(request) if request.method == "window/showMessageRequest")));
        assert_eq!(h.calls(), usize::from(mixed));
        client.shutdown();
    }
}

#[test]
fn allowlist_lock_timeout_keeps_session_grant() {
    let h = Harness::new("", false);
    let parent = h.dir.path().join("user/ryl");
    std::fs::create_dir_all(&parent).unwrap();
    let lock =
        std::fs::File::create(parent.join("allowed-schema-origins.lock")).unwrap();
    lock.lock().unwrap();
    let mut client = h.client(Limits::default());
    h.open(&client);
    await_error(&client, "approval required");
    approve(&mut client, ORIGIN, true);
    assert_eq!(await_schema(&client).len(), 1);
    assert_eq!(h.calls(), 1);
    assert!(!parent.join("allowed-schema-origins").exists());
    client.shutdown();
}

#[test]
fn blocked_schema_diagnostic_source_read_keeps_dispatch_responsive() {
    let h = Harness::new("", false);
    let mut client = h.client(Limits::default());
    let (policy_began, policy_release) = install_policy_gate(&h);
    h.open(&client);
    policy_began.recv_timeout(Duration::from_secs(5)).unwrap();
    let (started, began) = std::sync::mpsc::sync_channel(1);
    let (release, gate) = std::sync::mpsc::channel();
    *h.env.read_gate.lock().unwrap() = Some(PolicyGate {
        started,
        release: gate,
    });
    policy_release.send(()).unwrap();
    began.recv_timeout(Duration::from_secs(5)).unwrap();
    let id = client.request("ryl/unrelated", json!({}));
    let response = client.response(&id);
    release.send(()).unwrap();
    assert!(response.response_result.is_err());
    await_error(&client, "approval required");
    client.shutdown();
}

#[test]
fn unrelated_watched_file_preserves_pending_approval() {
    let h = Harness::new("", false);
    let mut client = h.client(Limits::default());
    h.open(&client);
    await_error(&client, "approval required");
    client.assert_alive();
    client.notify(
        "workspace/didChangeWatchedFiles",
        json!({"changes":[{"uri":file_uri(h.dir.path(), "unrelated.yaml"),"type":2}]}),
    );
    let id = client.request(
        "textDocument/diagnostic",
        json!({"textDocument":{"uri":file_uri(h.dir.path(), "test.yaml")}}),
    );
    assert!(
        pull_items(client.response(&id))[0]
            .message
            .contains("approval required")
    );
    approve(&mut client, ORIGIN, false);
    assert_eq!(await_schema(&client).len(), 1);
    assert_eq!(h.calls(), 1);
    client.shutdown();
}

#[test]
fn watched_file_after_last_close_clears_config_approval_diagnostics() {
    let h = Harness::new("", false);
    let mut client = h.client(Limits::default());
    let uri = h.open(&client);
    let config_uri = file_uri(h.dir.path(), ".ryl.toml");
    loop {
        if let Message::Notification(note) = client.recv()
            && note.method == "textDocument/publishDiagnostics"
        {
            let report: PublishDiagnosticsParams =
                serde_json::from_value(note.params).unwrap();
            if report.uri == config_uri && !report.diagnostics.is_empty() {
                assert!(report.diagnostics[0].message.contains("approval required"));
                break;
            }
        }
    }
    client.notify("textDocument/didClose", json!({"textDocument":{"uri":uri}}));
    client.assert_alive();
    client.notify(
        "workspace/didChangeWatchedFiles",
        changed_config(h.dir.path()),
    );
    let id = client.request("ryl/barrier", json!({}));
    let (messages, _) = client.messages_until_response(&id);
    assert!(messages.iter().any(|message| {
        matches!(message, Message::Notification(note)
            if note.method == "textDocument/publishDiagnostics"
                && note.params["uri"] == config_uri.as_str()
                && note.params["diagnostics"] == json!([]))
    }));
    client.shutdown();
}

#[test]
fn approval_actions_are_scoped_to_context_origins_and_range() {
    let h = Harness::new("", false);
    let mut client = h.client(Limits::default());
    let mut reports = Vec::new();
    for (name, origin) in [("a", ORIGIN), ("b", "https://other.example.org")] {
        let dir = h.dir.path().join(name);
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(
            dir.join(".ryl.toml"),
            config(&format!("{origin}/root.json")),
        )
        .unwrap();
        let uri = file_uri(&dir, "test.yaml");
        client.notify("textDocument/didOpen", json!({"textDocument":{"uri":uri,"languageId":"yaml","version":1,"text":"text\n"}}));
        loop {
            if let Message::Notification(note) = client.recv()
                && note.method == "textDocument/publishDiagnostics"
            {
                let report: PublishDiagnosticsParams =
                    serde_json::from_value(note.params).unwrap();
                if report.uri == file_uri(&dir, ".ryl.toml")
                    && !report.diagnostics.is_empty()
                {
                    reports.push((report, origin));
                    break;
                }
            }
        }
    }
    for (report, origin) in reports {
        let range = report.diagnostics[0].range;
        for (requested_range, expected) in [
            (range, 2),
            (Range::default(), 0),
            (Range::new(Position::new(99, 0), Position::new(99, 0)), 0),
        ] {
            let id = client.request("textDocument/codeAction", json!({"textDocument":{"uri":report.uri},"range":requested_range,"context":{"diagnostics":report.diagnostics}}));
            let actions = client.response(&id).response_result.unwrap();
            let actions = actions.as_array().cloned().unwrap_or_default();
            assert_eq!(actions.len(), expected);
            for action in actions {
                assert_eq!(action["command"]["arguments"][0], origin);
            }
        }
    }
    client.shutdown();
}

#[test]
fn closing_documents_retains_approval_and_completed_preparations() {
    let h = Harness::new("", false);
    let mut client = h.client(Limits::default());
    let uri = h.open(&client);
    await_error(&client, "approval required");
    client.notify("textDocument/didClose", json!({"textDocument":{"uri":uri}}));
    client.assert_alive();
    h.open(&client);
    await_error(&client, "approval required");
    approve(&mut client, ORIGIN, false);
    await_schema(&client);
    approve(&mut client, ORIGIN, true);
    client.notify("textDocument/didClose", json!({"textDocument":{"uri":uri}}));
    client.assert_alive();
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
fn remote_schema_allow_once_does_not_persist_grant() {
    let h = Harness::new("", false);
    let mut client = h.client(Limits::default());
    h.open(&client);
    await_error(&client, "approval required");
    assert_eq!(h.calls(), 0);
    approve(&mut client, ORIGIN, false);
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
    await_error(&client, "approval required");
    approve(&mut client, ORIGIN, true);
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
fn remote_schema_code_actions_approve_without_prompt() {
    for persist in [true, false] {
        let h = Harness::new("", false);
        let mut client = h.client(Limits::default());
        let uri = h.open(&client);
        await_error(&client, "approval required");
        let id = client.request(
            "textDocument/diagnostic",
            json!({"textDocument":{"uri":uri}}),
        );
        let diagnostics = pull_items(client.response(&id));
        let id = client.request("textDocument/codeAction", json!({"textDocument":{"uri":uri},"range":Range::default(),"context":{"diagnostics":diagnostics}}));
        let (messages, response) = client.messages_until_response(&id);
        assert!(!messages.iter().any(|message| matches!(message, Message::Request(request) if request.method == "window/showMessageRequest")));
        let actions = response.response_result.unwrap();
        assert_eq!(actions.as_array().unwrap().len(), 2);
        assert_eq!(actions[0]["title"], format!("Always allow {ORIGIN}"));
        assert_eq!(actions[1]["title"], format!("Allow once {ORIGIN}"));
        let action = &actions[usize::from(!persist)];
        let id = client.request("workspace/executeCommand", action["command"].clone());
        client.response(&id).response_result.unwrap();
        await_schema(&client);
        assert_eq!(h.calls(), 1);
        assert_eq!(
            h.dir
                .path()
                .join("user/ryl/allowed-schema-origins")
                .exists(),
            persist
        );
        client.shutdown();
    }
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
fn approval_diagnostics_on_open_documents_match_config_and_offer_quickfix() {
    for (pull, name) in [
        (false, "test.yaml"),
        (true, "test.yaml"),
        (true, "test.md"),
        (false, "test.md"),
    ] {
        let h = Harness::new("", false);
        let other = "https://other.example.org";
        std::fs::write(
            h.dir.path().join(".ryl.toml"),
            format!("{}{}", config(ROOT), config(&format!("{other}/type.json"))),
        )
        .unwrap();
        let capabilities = if pull {
            json!({"textDocument":{"diagnostic":{}}})
        } else {
            json!({})
        };
        let mut client = h.client_options(Limits::default(), capabilities, Value::Null);
        let uri = file_uri(h.dir.path(), name);
        client.notify("textDocument/didOpen", json!({"textDocument":{"uri":uri,"languageId":"yaml","version":1,"text":"text\n"}}));
        let mut config_diagnostics = Vec::new();
        let mut source_diagnostics = Vec::new();
        while config_diagnostics.is_empty() {
            match client.recv() {
                Message::Request(request) => panic!("unexpected request: {request:?}"),
                Message::Notification(note)
                    if note.method == "textDocument/publishDiagnostics" =>
                {
                    let report: PublishDiagnosticsParams =
                        serde_json::from_value(note.params).unwrap();
                    if report.uri == uri {
                        source_diagnostics = report.diagnostics;
                    } else if report.uri == file_uri(h.dir.path(), ".ryl.toml") {
                        config_diagnostics = report.diagnostics;
                    }
                }
                _ => {}
            }
        }
        let id = client.request(
            "textDocument/diagnostic",
            json!({"textDocument":{"uri":uri}}),
        );
        let diagnostics = pull_items(client.response(&id));
        assert_eq!(diagnostics.len(), 2);
        assert_eq!(
            diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>(),
            config_diagnostics
                .iter()
                .map(|d| &d.message)
                .collect::<Vec<_>>()
        );
        if !pull {
            assert_eq!(source_diagnostics, diagnostics);
        }
        for (diagnostic, origin) in diagnostics.iter().zip([other, ORIGIN]) {
            assert_eq!(diagnostic.range, Range::default());
            assert!(diagnostic.message.ends_with(&net::redact_message(&format!(
                "schema origin approval required: {origin}"
            ))));
        }
        let id = client.request("textDocument/codeAction", json!({"textDocument":{"uri":uri},"range":Range::default(),"context":{"diagnostics":diagnostics}}));
        let actions = client.response(&id).response_result.unwrap();
        assert_eq!(actions.as_array().unwrap().len(), 4);
        std::fs::write(h.dir.path().join(".ryl.toml"), TRAILING).unwrap();
        client.notify(
            "workspace/didChangeWatchedFiles",
            changed_config(h.dir.path()),
        );
        let id = client.request(
            "textDocument/diagnostic",
            json!({"textDocument":{"uri":uri}}),
        );
        assert!(pull_items(client.response(&id)).is_empty());
        client.shutdown();
    }
}

#[cfg(unix)]
#[test]
fn remote_schema_always_allow_refuses_symlinked_allowlist() {
    let h = Harness::new("", false);
    let mut client = h.client(Limits::default());
    h.open(&client);
    await_error(&client, "approval required");
    let target = h.dir.path().join("target");
    std::fs::write(&target, "").unwrap();
    std::fs::create_dir_all(h.dir.path().join("user/ryl")).unwrap();
    std::os::unix::fs::symlink(
        &target,
        h.dir.path().join("user/ryl/allowed-schema-origins"),
    )
    .unwrap();
    approve(&mut client, ORIGIN, true);
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
    await_error(&client, "approval required");
    assert_eq!(h.calls(), 1);
    approve(&mut client, "https://other.example.org", false);
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
    await_error(&client, "approval required");
    approve(&mut client, ORIGIN, false);
    await_schema(&client);
    assert_eq!(h.calls(), 1);
    client.shutdown();
}

#[test]
fn all_remote_association_origins_need_approval_before_first_fetch() {
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
    await_error(&client, "approval required");
    assert_eq!(h.calls(), 0);
    approve(&mut client, ORIGIN, false);
    approve(&mut client, "https://other.example.org", false);
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
fn workspace_scans_and_document_pulls_share_remote_preparation() {
    let h = Harness::new("", false);
    std::fs::write(h.dir.path().join("test.yaml"), "text\n").unwrap();
    let mut client = h.client_options(Limits::default(), json!({"textDocument":{"diagnostic":{}}, "workspace":{"diagnostics":{"refreshSupport":true}}}), json!({"diagnosticMode":"workspace"}));
    let id = client.request("workspace/diagnostic", json!({"previousResultIds":[]}));
    let (messages, response) = client.messages_until_response(&id);
    assert!(response.response_result.is_ok());
    if !messages.iter().any(|message| matches!(message, Message::Notification(note) if note.method == "textDocument/publishDiagnostics" && note.params.to_string().contains("approval required"))) {
        await_error(&client, "approval required");
    }
    assert_eq!(h.calls(), 0);
    approve(&mut client, ORIGIN, false);
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
    await_error(&client, "approval required");
    let (began, release) = install_policy_gate(&h);
    approve(&mut client, ORIGIN, false);
    began.recv_timeout(Duration::from_secs(5)).unwrap();
    approve(&mut client, "https://other.example.org", false);
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
fn inline_workspace_configuration_can_request_schemas_but_cannot_grant_origins() {
    let h = Harness::new("", false);
    let mut client = h.client_options(Limits::default(), json!({}), json!({"configData":config(ROOT), "schemaAllowedOrigins":[ORIGIN], "allowedSchemaOrigins":[ORIGIN]}));
    h.open(&client);
    await_error(&client, "approval required");
    assert_eq!(h.calls(), 0);
    approve(&mut client, ORIGIN, false);
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

#[test]
fn always_allow_persistence_keeps_dispatch_and_shutdown_responsive() {
    let h = Harness::new("", false);
    let mut client = h.client(Limits::default());
    h.open(&client);
    await_error(&client, "approval required");
    let (started, began) = std::sync::mpsc::sync_channel(1);
    let (release, gate) = std::sync::mpsc::channel();
    *h.env.config_gate.lock().unwrap() = Some(PolicyGate {
        started,
        release: gate,
    });
    approve(&mut client, ORIGIN, true);
    began.recv_timeout(Duration::from_secs(5)).unwrap();
    let id = client.request("shutdown", Value::Null);
    let deadline = Instant::now() + Duration::from_millis(500);
    let responsive = loop {
        match client
            .conn()
            .receiver
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        {
            Ok(Message::Response(response)) if response.id == id => break true,
            Ok(_) => {}
            Err(_) => break false,
        }
    };
    release.send(()).unwrap();
    if !responsive {
        client.response(&id);
    }
    client.notify("exit", Value::Null);
    drop(client);
    assert!(
        responsive,
        "shutdown must respond while persistence is blocked"
    );
}

#[test]
fn always_allow_write_failure_reports_error_and_grants_session_access() {
    let h = Harness::new("", false);
    let mut client = h.client(Limits::default());
    h.open(&client);
    await_error(&client, "approval required");
    h.env.config_unavailable.store(true, Ordering::Relaxed);
    approve(&mut client, ORIGIN, true);
    await_config_message(&client, "user config directory unavailable");
    await_schema(&client);
    assert_eq!(h.calls(), 1);
    client.shutdown();
}

fn await_config_message(client: &Client, needle: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let message = client
            .conn()
            .receiver
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap();
        if let Message::Notification(note) = message
            && note.method == "window/showMessage"
            && note.params["message"].as_str().unwrap().contains(needle)
        {
            return;
        }
    }
}

#[test]
fn seventeen_remote_configs_settle_after_input_stops() {
    let h = Harness::new(ORIGIN, true);
    let mut client = h.client(Limits::default());
    for index in 0..17 {
        let dir = h.dir.path().join(index.to_string());
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join(".ryl.toml"), config(ROOT)).unwrap();
        client.did_open(file_uri(&dir, "test.yaml"), "text\n");
    }
    client.assert_alive();
    let end = Instant::now() + Duration::from_secs(1);
    let mut publications = 0;
    while let Ok(message) = client
        .conn()
        .receiver
        .recv_timeout(end.saturating_duration_since(Instant::now()))
    {
        if let Message::Notification(note) = message
            && note.method == "textDocument/publishDiagnostics"
        {
            publications += 1;
            assert!(
                publications <= 17 * 18,
                "preparations must not cycle: {publications}"
            );
        }
    }
    assert_eq!(h.calls(), 0);
    client.shutdown();
}

#[test]
fn expired_preflight_never_resurrects_an_approval_or_retry() {
    for requested_timeout in [false, true] {
        let h = Harness::new("", false);
        let mut client = h.client(Limits {
            preparation: Duration::from_millis(100),
            ..Limits::default()
        });
        let (began, release_policy) = install_policy_gate(&h);
        let uri = h.open(&client);
        began.recv_timeout(Duration::from_secs(5)).unwrap();
        let (started, began) = std::sync::mpsc::sync_channel(1);
        let (release, gate) = std::sync::mpsc::channel();
        *h.env.read_gate.lock().unwrap() = Some(PolicyGate {
            started,
            release: gate,
        });
        release_policy.send(()).unwrap();
        began.recv_timeout(Duration::from_secs(5)).unwrap();
        thread::sleep(Duration::from_millis(150));
        let timeout = if requested_timeout {
            let id = client.request(
                "textDocument/diagnostic",
                json!({"textDocument":{"uri":uri}}),
            );
            let items = pull_items(client.response(&id));
            assert!(items[0].message.contains("deadline exceeded"));
            Some(items[0].message.clone())
        } else {
            None
        };
        release.send(()).unwrap();
        if !requested_timeout {
            await_error(&client, "cancelled or deadline exceeded");
        }
        let end = Instant::now() + Duration::from_millis(200);
        while let Ok(message) = client
            .conn()
            .receiver
            .recv_timeout(end.saturating_duration_since(Instant::now()))
        {
            assert!(
                !matches!(message, Message::Request(ref request) if request.method == "window/showMessageRequest"),
                "expired preparation cannot request approval"
            );
        }
        let id = client.request(
            "textDocument/diagnostic",
            json!({"textDocument":{"uri":uri}}),
        );
        let items = pull_items(client.response(&id));
        assert!(items[0].message.contains("deadline exceeded"));
        if let Some(timeout) = timeout {
            assert_eq!(
                items[0].message, timeout,
                "worker must preserve request's terminal result"
            );
        }
        assert_eq!(h.calls(), 0);
        client.shutdown();
    }
}
