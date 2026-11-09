#![cfg(feature = "schema-network")]
mod cache;

use std::collections::BTreeMap;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};
use std::time::Duration;

use ryl::config::{Overrides, discover_config_with};
use ryl::rules::schema::net::{Clock, Dependencies, Error, Network, Transport};
use serde_json::json;
use url::Url;

use crate::common::fake_env::FakeEnv;

#[derive(Default)]
struct Time(AtomicU64);
impl Clock for Time {
    fn unix_seconds(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
    fn monotonic(&self) -> Duration {
        Duration::ZERO
    }
}

#[derive(Default)]
struct Memory {
    resources: Mutex<BTreeMap<String, Vec<u8>>>,
    calls: Mutex<Vec<String>>,
}
impl Transport for Memory {
    fn get(&self, url: &Url, _: Duration, _: usize) -> Result<Vec<u8>, Error> {
        self.calls.lock().unwrap().push(url.to_string());
        self.resources
            .lock()
            .unwrap()
            .get(url.as_str())
            .cloned()
            .ok_or_else(|| Error::transport(url))
    }
}

struct Harness {
    dir: tempfile::TempDir,
    transport: Arc<Memory>,
    clock: Arc<Time>,
    env: FakeEnv,
}
impl Harness {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let transport = Arc::new(Memory::default());
        let clock = Arc::new(Time::default());
        let dependencies = Dependencies {
            transport: transport.clone(),
            clock: clock.clone(),
            budget: Arc::default(),
            filesystem: Arc::new(ryl::rules::schema::net::cache_io::SystemFileSystem),
        };
        let env = FakeEnv::new()
            .with_cwd(dir.path())
            .with_var("RYL_SCHEMA_ALLOWED_ORIGINS", "https://schemas.example.org")
            .with_var(
                "RYL_SCHEMA_CACHE_DIR",
                dir.path()
                    .canonicalize()
                    .unwrap()
                    .join("cache")
                    .to_str()
                    .unwrap(),
            )
            .with_schema_dependencies(dependencies);
        Self {
            dir,
            transport,
            clock,
            env,
        }
    }
    fn resource(&self, url: &str, value: serde_json::Value) {
        self.transport
            .resources
            .lock()
            .unwrap()
            .insert(url.to_owned(), value.to_string().into_bytes());
    }
    fn network(&self) -> Network {
        Network::from_env(&self.env, BTreeMap::new()).unwrap()
    }
    fn prepare(&self, content: &str) -> Result<ryl::config::ConfigContext, String> {
        discover_config_with(
            &[],
            &Overrides {
                config_file: None,
                config_data: Some(content.to_owned()),
            },
            &self.env,
        )
    }
}

const ROOT: &str = "https://schemas.example.org/root.json";
const CONFIG: &str =
    "[[lint.rules.schema.schemas]]\nurl = 'https://schemas.example.org/root.json'\n";

#[test]
fn request_and_origin_grant_are_both_required() {
    let h = Harness::new();
    h.resource(ROOT, json!({"type":"object"}));
    let cfg = h.prepare(CONFIG).unwrap();
    assert!(
        ryl::lint_str(
            "{}\n",
            std::path::Path::new("a.yaml"),
            &cfg.config,
            h.dir.path()
        )
        .is_empty()
    );
    assert_eq!(h.transport.calls.lock().unwrap().len(), 1);
    let denied = h.env.clone().with_var("RYL_SCHEMA_ALLOWED_ORIGINS", "");
    let error = Network::from_env(&denied, BTreeMap::new())
        .unwrap()
        .fetch(ROOT, 1024)
        .unwrap_err();
    assert!(error.to_string().contains("origin not granted"));
    assert_eq!(h.transport.calls.lock().unwrap().len(), 1);
}

#[test]
fn ungranted_refs_rebased_ids_and_remote_file_refs_fail_preparation() {
    let h = Harness::new();
    for (schema, expected) in [
        (
            json!({"$ref":"https://other.example.org/child?token=secret"}),
            "origin not granted",
        ),
        (
            json!({"$id":"https://other.example.org/base/root", "$ref":"child"}),
            "origin not granted",
        ),
        (json!({"$ref":"file:///secret.json"}), "cannot read file"),
        (
            json!({"$schema":"https://other.example.org/meta", "type":"object"}),
            "origin not granted",
        ),
    ] {
        h.resource(ROOT, schema);
        if h.dir.path().join("cache").exists() {
            std::fs::remove_dir_all(h.dir.path().join("cache")).unwrap();
        }
        let error = h.prepare(CONFIG).unwrap_err();
        assert!(error.contains(expected), "{error}");
        assert!(!error.contains("secret"), "{error}");
    }
}

#[test]
fn invalid_destinations_and_grants_never_reach_transport() {
    let h = Harness::new();
    for raw in [
        "http://schemas.example.org/root",
        "https://schemas.example.org:444/root",
        "https://user:secret@schemas.example.org/root",
        "file:///secret",
        "ftp://schemas.example.org/root",
        "not a URL",
    ] {
        assert!(h.network().fetch(raw, 1024).is_err(), "{raw}");
    }
    for grant in [
        "https://*.example.org",
        "https://schemas.example.org/path",
        "https://schemas.example.org?token=secret",
        "https://schemas.example.org#fragment",
        "http://schemas.example.org",
    ] {
        assert!(
            Network::from_env(
                &h.env.clone().with_var("RYL_SCHEMA_ALLOWED_ORIGINS", grant),
                BTreeMap::new()
            )
            .is_err(),
            "{grant}"
        );
    }
    assert!(h.transport.calls.lock().unwrap().is_empty());
}

#[test]
fn cache_freshness_offline_and_pins_use_actual_bytes() {
    let h = Harness::new();
    h.resource(ROOT, json!(true));
    assert_eq!(h.network().fetch(ROOT, 100).unwrap(), b"true");
    assert_eq!(h.network().fetch(ROOT, 100).unwrap(), b"true");
    assert_eq!(h.transport.calls.lock().unwrap().len(), 1);
    h.clock.0.store(24 * 60 * 60, Ordering::Relaxed);
    let offline = h.env.clone().with_var("RYL_OFFLINE", "1");
    assert_eq!(
        Network::from_env(&offline, BTreeMap::new())
            .unwrap()
            .fetch(ROOT, 100)
            .unwrap(),
        b"true"
    );
    assert_eq!(h.transport.calls.lock().unwrap().len(), 1);
    let pinned = BTreeMap::from([(
        ROOT.to_owned(),
        "sha256:b5bea41b6c623f7c09f1bf24dcae58ebab3c0cdd90ad966bc43a45b44867e12b"
            .to_owned(),
    )]);
    assert_eq!(
        Network::from_env(&h.env, pinned)
            .unwrap()
            .fetch(ROOT, 100)
            .unwrap(),
        b"true"
    );
    assert_eq!(h.transport.calls.lock().unwrap().len(), 1);
    h.resource(ROOT, json!(false));
    assert_eq!(h.network().fetch(ROOT, 100).unwrap(), b"false");
    assert_eq!(h.transport.calls.lock().unwrap().len(), 2);
    let wrong =
        BTreeMap::from([(ROOT.to_owned(), format!("sha256:{}", "0".repeat(64)))]);
    assert!(
        Network::from_env(&h.env, wrong)
            .unwrap()
            .fetch(ROOT, 100)
            .unwrap_err()
            .to_string()
            .contains("pin mismatch")
    );
    assert!(
        Network::from_env(&offline, BTreeMap::new())
            .unwrap()
            .fetch("https://schemas.example.org/missing", 100)
            .unwrap_err()
            .to_string()
            .contains("offline cache")
    );
}

#[test]
fn updated_pin_refetches_a_cached_entry_but_offline_refuses_it() {
    let pin_for =
        |digest: &str| BTreeMap::from([(ROOT.to_owned(), format!("sha256:{digest}"))]);
    let h = Harness::new();
    h.resource(ROOT, json!(true));
    let old =
        pin_for("b5bea41b6c623f7c09f1bf24dcae58ebab3c0cdd90ad966bc43a45b44867e12b");
    Network::from_env(&h.env, old)
        .unwrap()
        .fetch(ROOT, 100)
        .unwrap();
    h.resource(ROOT, json!(false));
    let new =
        pin_for("fcbcf165908dd18a9e49f7ff27810176db8e9f63b4352213741664245224f8aa");
    let offline = h.env.clone().with_var("RYL_OFFLINE", "1");
    let refused = Network::from_env(&offline, new.clone())
        .unwrap()
        .fetch(ROOT, 100);
    assert!(refused.unwrap_err().to_string().contains("pin mismatch"));
    assert_eq!(
        Network::from_env(&h.env, new)
            .unwrap()
            .fetch(ROOT, 100)
            .unwrap(),
        b"false",
        "a pin no longer matching the cached copy must refetch, not fail"
    );
    assert_eq!(h.transport.calls.lock().unwrap().len(), 2);
}

#[test]
fn corrupt_entries_miss_and_full_urls_have_distinct_cache_keys() {
    let h = Harness::new();
    h.resource(ROOT, json!(true));
    let other = "https://schemas.example.org/other/root.json?token=secret";
    h.resource(other, json!(false));
    assert_eq!(h.network().fetch(ROOT, 100).unwrap(), b"true");
    assert_eq!(h.network().fetch(other, 100).unwrap(), b"false");
    let entries: Vec<_> = std::fs::read_dir(h.dir.path().join("cache"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(entries.len(), 2);
    for entry in entries {
        std::fs::write(entry, "broken").unwrap();
    }
    assert_eq!(h.network().fetch(ROOT, 100).unwrap(), b"true");
    assert_eq!(h.transport.calls.lock().unwrap().len(), 3);
    assert!(
        h.network()
            .fetch(ROOT, 1)
            .unwrap_err()
            .to_string()
            .contains("size limit")
    );
}

#[test]
fn local_roots_can_request_granted_remote_refs_without_remote_file_capabilities() {
    let h = Harness::new();
    std::fs::write(
        h.dir.path().join("local.json"),
        format!("{{\"$ref\":\"{ROOT}\"}}"),
    )
    .unwrap();
    h.resource(ROOT, json!({"type":"object"}));
    assert!(
        h.prepare("[[lint.rules.schema.schemas]]\npath = 'local.json'\n")
            .is_ok()
    );
}

#[test]
fn enabling_the_network_mid_build_does_not_charge_the_local_root_twice() {
    let h = Harness::new();
    let padding = "x".repeat(9 * 1024 * 1024);
    std::fs::write(
        h.dir.path().join("local.json"),
        format!("{{\"description\":\"{padding}\",\"$ref\":\"{ROOT}\"}}"),
    )
    .unwrap();
    h.resource(ROOT, json!(true));
    h.prepare("[[lint.rules.schema.schemas]]\npath = 'local.json'\n")
        .unwrap();
}

#[test]
fn process_request_budget_is_shared_across_preparations() {
    let h = Harness::new();
    for index in 0..64 {
        let url = format!("https://schemas.example.org/{index}");
        h.resource(&url, json!(true));
        h.network().fetch(&url, 100).unwrap();
    }
    assert!(
        h.network()
            .fetch(ROOT, 100)
            .unwrap_err()
            .to_string()
            .contains("budget exhausted")
    );
    assert_eq!(h.transport.calls.lock().unwrap().len(), 64);
}

#[test]
fn output_redacts_query_values_credentials_and_bidi_controls() {
    let text = ryl::rules::schema::net::display_url(
        "https://user:password@schemas.example.org/a?token=secret&x=value#hidden",
    );
    for secret in ["password", "user", "secret", "value", "hidden"] {
        assert!(!text.contains(secret), "{text}");
    }
    assert!(text.ends_with("/a?***"), "{text}");
    for echoed in [
        "Invalid URI reference 'https://o.example.org/r?token=secret with spaces': detail",
        "Invalid URI reference 'https://o.example.org/r?token=a\"secret b': detail",
        "Invalid URI reference 'child.json?token=secret with spaces': detail",
        "Invalid URI reference '//user:secret@o.example.org/r': detail",
        "Invalid URI reference 'https:child.json?token=a'secret b': detail",
        "Invalid URI reference \"//o.example.org/r?token=secret\": detail",
    ] {
        let redacted = ryl::rules::schema::net::redact_message(echoed);
        assert!(!redacted.contains("secret"), "{redacted}");
        assert!(redacted.ends_with(": detail"), "{redacted}");
    }
    assert_eq!(
        ryl::rules::schema::net::redact_message(
            "can't read https://o.example.org/r?token=secret"
        ),
        "can't read https://o.example.org/r?***"
    );
    assert_eq!(
        ryl::cli_support::sanitize_control("a\u{202e}b\u{200f}\u{2066}\n"),
        "a\\u{202e}b\\u{200f}\\u{2066}\\u{a}"
    );
}

#[derive(Debug)]
struct Addresses(Mutex<Vec<std::net::SocketAddr>>);
impl ryl::rules::schema::net::http::AddressSource for Addresses {
    fn resolve(
        &self,
        _: &str,
        _: std::time::Instant,
    ) -> std::io::Result<Vec<std::net::SocketAddr>> {
        Ok(self.0.lock().unwrap().clone())
    }
}

#[test]
fn complete_dns_answers_rebinding_and_all_denied_address_classes() {
    use ryl::rules::schema::net::http::CheckedResolver;
    use ureq::unversioned::{resolver::Resolver, transport::NextTimeout};
    let source = Arc::new(Addresses(Mutex::new(vec!["8.8.8.8:443".parse().unwrap()])));
    let resolver = CheckedResolver::new(source.clone());
    let uri = "https://schemas.example.org/root".parse().unwrap();
    let config = ureq::config::Config::default();
    assert_eq!(
        resolver
            .resolve(&uri, &config, NextTimeout::default())
            .unwrap()
            .len(),
        1
    );
    for ip in [
        "0.0.0.0",
        "10.0.0.1",
        "127.0.0.1",
        "169.254.169.254",
        "172.16.0.1",
        "192.168.1.1",
        "100.64.0.1",
        "224.0.0.1",
        "255.255.255.255",
        "192.0.2.1",
        "192.0.0.1",
        "192.88.99.1",
        "198.18.0.1",
        "198.51.100.1",
        "203.0.113.1",
        "::",
        "::1",
        "::ffff:8.8.8.8",
        "::8.8.8.8",
        "64:ff9b::808:808",
        "64:ff9b:1::1",
        "2002:808:808::",
        "2001::1",
        "2001:db8::1",
        "3fff::1",
        "fc00::1",
        "fe80::1",
        "ff02::1",
    ] {
        let mut all = vec!["8.8.8.8:443".parse().unwrap(); 17];
        all.push(std::net::SocketAddr::new(ip.parse().unwrap(), 443));
        *source.0.lock().unwrap() = all;
        assert!(
            resolver
                .resolve(&uri, &config, NextTimeout::default())
                .is_err(),
            "{ip}"
        );
    }
    *source.0.lock().unwrap() = vec!["[2606:4700:4700::1111]:443".parse().unwrap(); 20];
    assert_eq!(
        resolver
            .resolve(&uri, &config, NextTimeout::default())
            .unwrap()
            .len(),
        16
    );
    source.0.lock().unwrap().clear();
    assert!(
        resolver
            .resolve(&uri, &config, NextTimeout::default())
            .is_err()
    );
}

#[derive(Debug)]
struct LoopbackConnector(std::net::SocketAddr);
impl ureq::unversioned::transport::Connector for LoopbackConnector {
    type Out = <ureq::unversioned::transport::TcpConnector as ureq::unversioned::transport::Connector>::Out;
    fn connect(
        &self,
        details: &ureq::unversioned::transport::ConnectionDetails,
        chained: Option<()>,
    ) -> Result<Option<Self::Out>, ureq::Error> {
        use ureq::unversioned::{
            resolver::ResolvedSocketAddrs,
            transport::{ConnectionDetails, TcpConnector},
        };
        assert!(details.addrs.iter().all(|addr| addr.ip() == "8.8.8.8".parse::<std::net::IpAddr>().unwrap()));
        let mut addrs = ResolvedSocketAddrs::from_fn(|_| self.0);
        addrs.push(self.0);
        let mapped = ConnectionDetails {
            uri: details.uri,
            addrs,
            config: details.config,
            request_level: details.request_level,
            resolver: details.resolver,
            now: details.now,
            timeout: details.timeout,
            current_time: details.current_time.clone(),
            run_connector: details.run_connector.clone(),
            register_shutdown: details.register_shutdown.clone(),
        };
        TcpConnector::default().connect(&mapped, chained)
    }
}

struct TlsAgent(std::net::SocketAddr);
impl ryl::rules::schema::net::http::AgentFactory for TlsAgent {
    fn tls_config(&self) -> ureq::tls::TlsConfig {
        let cert = ureq::tls::Certificate::from_pem(include_bytes!(
            "fixtures/schema-tls/ca.pem"
        ))
        .unwrap();
        ureq::tls::TlsConfig::builder()
            .root_certs(ureq::tls::RootCerts::Specific(Arc::new(vec![cert])))
            .build()
    }
    fn build(&self, config: ureq::config::Config) -> ureq::Agent {
        use ureq::unversioned::transport::{Connector, RustlsConnector};
        let connector =
            ().chain(LoopbackConnector(self.0))
                .chain(RustlsConnector::default());
        let resolver = ryl::rules::schema::net::http::CheckedResolver::new(Arc::new(
            Addresses(Mutex::new(vec!["8.8.8.8:443".parse().unwrap()])),
        ));
        ureq::Agent::with_parts(config, connector, resolver)
    }
}

fn with_tls_server<R>(
    response: &[u8],
    run: impl FnOnce(ryl::rules::schema::net::http::Http) -> R,
) -> R {
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let cert =
        CertificateDer::from_pem_slice(include_bytes!("fixtures/schema-tls/cert.pem"))
            .unwrap();
    let key =
        PrivateKeyDer::from_pem_slice(include_bytes!("fixtures/schema-tls/key.pem"))
            .unwrap();
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert], key)
        .unwrap();
    let response = response.to_vec();
    let server = std::thread::spawn(move || {
        let start = std::time::Instant::now();
        let socket = loop {
            match listener.accept() {
                Ok((socket, _)) => break socket,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && start.elapsed() < Duration::from_secs(3) =>
                {
                    std::thread::sleep(Duration::from_millis(2))
                }
                Err(error) => panic!("TLS fixture accept failed: {error}"),
            }
        };
        socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        socket
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut stream = rustls::StreamOwned::new(
            rustls::ServerConnection::new(Arc::new(config)).unwrap(),
            socket,
        );
        let mut buf = [0; 4096];
        let amount = stream.read(&mut buf).unwrap();
        assert!(
            String::from_utf8_lossy(&buf[..amount]).contains("schemas.example.org")
        );
        if let Some(split) = response.windows(6).position(|part| part == b"|slow|") {
            stream.write_all(&response[..split]).unwrap();
            std::thread::sleep(Duration::from_millis(400));
            let _ = stream.write_all(&response[split + 6..]);
        } else {
            let _ = stream.write_all(&response);
        }
    });
    let transport =
        ryl::rules::schema::net::http::Http::new(Arc::new(TlsAgent(address)));
    let result = run(transport);
    server.join().unwrap();
    result
}

fn tls_response(response: &[u8], timeout: Duration) -> Result<Vec<u8>, Error> {
    with_tls_server(response, |transport| {
        transport.get(&Url::parse(ROOT).unwrap(), timeout, 100)
    })
}

#[test]
fn real_tls_transport_rejects_redirects_encoding_truncation_and_oversized_bodies() {
    assert_eq!(
        tls_response(
            b"HTTP/1.1 200 OK\r\nContent-Encoding: identity\r\nContent-Length: 4\r\nConnection: close\r\n\r\ntrue",
            Duration::from_secs(2)
        )
        .unwrap(),
        b"true"
    );
    for (response, expected) in [
        (b"HTTP/1.1 302 Found\r\nLocation: https://other.example.org/a?token=secret\r\nContent-Length: 0\r\n\r\n".as_slice(), "redirect refused"),
        (b"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: 4\r\n\r\ntrue".as_slice(), "encoded"),
        (b"HTTP/1.1 200 OK\r\nContent-Length: 40\r\n\r\ntrue".as_slice(), "truncated"),
        (b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".as_slice(), "HTTP status 404"),
    ] {
        let error = tls_response(response, Duration::from_secs(2)).unwrap_err().to_string();
        assert!(error.contains(expected), "{error}");
        assert!(!error.contains("secret"), "{error}");
    }
}

#[test]
fn cached_reference_closure_and_loaded_bytes_drive_preparation_digest() {
    let h = Harness::new();
    let child = "https://schemas.example.org/child.json";
    h.resource(ROOT, json!({"$ref":"child.json"}));
    h.resource(child, json!({"type":"object"}));
    let before = format!("{:?}", h.prepare(CONFIG).unwrap().config);
    let offline = Harness {
        env: h.env.clone().with_var("RYL_OFFLINE", "1"),
        ..h
    };
    assert_eq!(
        format!("{:?}", offline.prepare(CONFIG).unwrap().config),
        before
    );
    assert_eq!(offline.transport.calls.lock().unwrap().len(), 2);
    let cache = offline.dir.path().join("cache");
    let child_entry = std::fs::read_dir(&cache)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            let bytes = std::fs::read(path).unwrap();
            let split = bytes.iter().position(|byte| *byte == b'\n').unwrap();
            let metadata: serde_json::Value =
                serde_json::from_slice(&bytes[..split]).unwrap();
            metadata["url"] == child
        })
        .unwrap();
    std::fs::remove_file(child_entry).unwrap();
    let error = offline.prepare(CONFIG).unwrap_err();
    assert!(error.contains("offline cache"), "{error}");
    assert!(error.contains(child), "{error}");
    assert_eq!(offline.transport.calls.lock().unwrap().len(), 2);
    std::fs::remove_dir_all(cache).unwrap();
    let online = Harness {
        env: offline.env.clone().with_var("RYL_OFFLINE", "0"),
        ..offline
    };
    online.resource(child, json!({"type":"string"}));
    assert_ne!(
        format!("{:?}", online.prepare(CONFIG).unwrap().config),
        before
    );
}

#[test]
fn fresh_download_pin_mismatch_oversize_and_proxy_notes_are_redacted() {
    let h = Harness::new();
    h.resource(ROOT, json!(true));
    let wrong =
        BTreeMap::from([(ROOT.to_owned(), format!("sha256:{}", "0".repeat(64)))]);
    assert!(
        Network::from_env(&h.env, wrong)
            .unwrap()
            .fetch(ROOT, 100)
            .unwrap_err()
            .to_string()
            .contains("pin mismatch")
    );
    assert!(!h.dir.path().join("cache").exists());
    assert!(
        h.network()
            .fetch(ROOT, 1)
            .unwrap_err()
            .to_string()
            .contains("size limit")
    );
    let env = h
        .env
        .clone()
        .with_var("HTTPS_PROXY", "https://user:proxy-secret@proxy.invalid");
    let error = Network::from_env(&env, BTreeMap::new())
        .unwrap()
        .fetch("https://schemas.example.org/missing?token=secret", 100)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("HTTPS_PROXY set; ryl does not use proxies"),
        "{error}"
    );
    assert!(!error.contains("secret"), "{error}");
}

#[test]
fn user_allowlist_unions_normalized_origins_and_reports_bad_policy() {
    let h = Harness::new();
    let policy = h.dir.path().join("ryl/allowed-schema-origins");
    let env = h
        .env
        .clone()
        .with_config_dir(h.dir.path())
        .with_file(&policy, "https://SECOND.example.org:443\n\n");
    h.resource("https://second.example.org/root", json!(true));
    Network::from_env(&env, BTreeMap::new())
        .unwrap()
        .fetch("https://second.example.org/root#fragment", 100)
        .unwrap();
    h.resource(ROOT, json!(true));
    Network::from_env(&env, BTreeMap::new())
        .unwrap()
        .fetch(ROOT, 100)
        .unwrap();
    assert!(
        Network::from_env(
            &env.with_file(policy, "https://bad.example.org/path"),
            BTreeMap::new()
        )
        .is_err()
    );
}

#[test]
fn cache_eviction_and_unwritable_destinations_fail_closed() {
    let h = Harness::new();
    let cache = h.dir.path().join("cache");
    std::fs::create_dir(&cache).unwrap();
    let old = cache.join("old-entry");
    std::fs::File::create(&old)
        .unwrap()
        .set_len(256 * 1024 * 1024)
        .unwrap();
    h.resource(ROOT, json!(true));
    h.network().fetch(ROOT, 100).unwrap();
    assert!(!old.exists());
    std::fs::remove_dir_all(&cache).unwrap();
    std::fs::write(&cache, "file blocking cache directory").unwrap();
    assert!(
        h.network()
            .fetch(ROOT, 100)
            .unwrap_err()
            .to_string()
            .contains("cache")
    );
}

#[cfg(unix)]
#[test]
fn cache_refuses_symlink_entries_and_parents() {
    use std::os::unix::fs::symlink;
    let h = Harness::new();
    let cache = h.dir.path().join("cache");
    let outside = h.dir.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    symlink(&outside, &cache).unwrap();
    h.resource(ROOT, json!(true));
    assert!(
        h.network()
            .fetch(ROOT, 100)
            .unwrap_err()
            .to_string()
            .contains("symlinks")
    );
    assert!(std::fs::read_dir(&outside).unwrap().next().is_none());
    std::fs::remove_file(&cache).unwrap();
    std::fs::create_dir(&cache).unwrap();
    symlink(&outside, cache.join("entry")).unwrap();
    assert!(
        h.network()
            .fetch(ROOT, 100)
            .unwrap_err()
            .to_string()
            .contains("symlinks")
    );
}

#[test]
fn association_exclusivity_and_pin_shape_are_config_errors() {
    let h = Harness::new();
    for config in [
        "[[lint.rules.schema.schemas]]\nfiles = ['*.yaml']\n",
        "[[lint.rules.schema.schemas]]\npath = 'a.json'\nurl = 'https://schemas.example.org/root.json'\n",
        "[lint.rules.schema.pins]\n'https://schemas.example.org/root.json' = 'sha256:bad'\n",
    ] {
        assert!(
            h.prepare(config)
                .unwrap_err()
                .contains("invalid config: schema")
        );
    }
}

#[test]
fn cache_and_repeated_associations_count_toward_preparation_limits() {
    let h = Harness::new();
    std::fs::write(
        h.dir.path().join("large.json"),
        format!("{}true", " ".repeat(8 * 1024 * 1024)),
    )
    .unwrap();
    let config = "[[lint.rules.schema.schemas]]\npath = 'large.json'\n".repeat(2);
    assert!(h.prepare(&config).unwrap_err().contains("size limit"));
    std::fs::write(h.dir.path().join("small.json"), "true").unwrap();
    let config = "[[lint.rules.schema.schemas]]\npath = 'small.json'\n".repeat(1025);
    assert!(h.prepare(&config).unwrap_err().contains("resource limit"));
}

struct StatusWriter(std::sync::mpsc::SyncSender<Vec<u8>>);
impl std::io::Write for StatusWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let _ = self.0.try_send(bytes.to_vec());
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn slow_preparation_status_appears_after_delay_and_fast_work_cancels() {
    use ryl::rules::schema::net::progress::Progress;
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let status = Progress::start(Box::new(StatusWriter(tx.clone())));
    assert_eq!(
        rx.recv_timeout(Duration::from_secs(2)).unwrap(),
        b"Preparing schemas...\n"
    );
    drop(status);
    let status = Progress::start(Box::new(StatusWriter(tx)));
    drop(status);
    assert!(rx.recv_timeout(Duration::from_millis(250)).is_err());
}

struct BlockedLookup {
    started: std::sync::mpsc::SyncSender<()>,
    release: std::sync::mpsc::Receiver<()>,
    finished: std::sync::mpsc::SyncSender<()>,
}
impl ryl::rules::schema::net::http::Lookup for BlockedLookup {
    fn lookup(&self, _: &str) -> std::io::Result<Vec<std::net::SocketAddr>> {
        self.started.send(()).unwrap();
        self.release.recv_timeout(Duration::from_secs(2)).unwrap();
        Ok(vec!["8.8.8.8:443".parse().unwrap()])
    }
}
impl Drop for BlockedLookup {
    fn drop(&mut self) {
        let _ = self.finished.try_send(());
    }
}

#[test]
fn blocked_resolver_has_one_worker_bounded_queue_and_caller_deadlines() {
    use ryl::rules::schema::net::http::{AddressSource, ResolverWorker};
    let (started, began) = std::sync::mpsc::sync_channel(1);
    let (release, gate) = std::sync::mpsc::sync_channel(1);
    let (finished, done) = std::sync::mpsc::sync_channel(1);
    let worker = ResolverWorker::new(Box::new(BlockedLookup {
        started,
        release: gate,
        finished,
    }));
    let deadline = || std::time::Instant::now() + Duration::from_millis(50);
    assert_eq!(
        worker.resolve("first", deadline()).unwrap_err().kind(),
        std::io::ErrorKind::TimedOut
    );
    began.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(
        worker.resolve("queued", deadline()).unwrap_err().kind(),
        std::io::ErrorKind::TimedOut
    );
    assert_eq!(
        worker.resolve("full", deadline()).unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    release.send(()).unwrap();
    drop(worker);
    done.recv_timeout(Duration::from_secs(2)).unwrap();
}

#[test]
fn real_tls_transport_caps_body_headers_and_slow_streams() {
    let mut oversized = b"HTTP/1.1 200 OK\r\nContent-Length: 101\r\n\r\n".to_vec();
    oversized.extend([b'x'; 101]);
    assert!(
        tls_response(&oversized, Duration::from_secs(2))
            .unwrap_err()
            .to_string()
            .contains("size limit")
    );
    let header = format!(
        "HTTP/1.1 200 OK\r\nLarge: {}\r\nContent-Length: 0\r\n\r\n",
        "x".repeat(33 * 1024)
    );
    assert!(
        tls_response(header.as_bytes(), Duration::from_secs(2))
            .unwrap_err()
            .to_string()
            .contains("request failed")
    );
    assert!(
        tls_response(
            b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\n|slow|true",
            Duration::from_millis(200)
        )
        .unwrap_err()
        .to_string()
        .contains("timed out")
    );
}

struct ChargedTime(AtomicU64);
impl Clock for ChargedTime {
    fn unix_seconds(&self) -> u64 {
        0
    }
    fn monotonic(&self) -> Duration {
        Duration::from_secs(self.0.fetch_add(30, Ordering::Relaxed))
    }
}

#[test]
fn process_time_budget_is_shared_without_sleeping_or_refreshing_cache() {
    let h = Harness::new();
    let dependencies = Dependencies {
        transport: h.transport.clone(),
        clock: Arc::new(ChargedTime(AtomicU64::new(0))),
        budget: Arc::default(),
        filesystem: Arc::new(ryl::rules::schema::net::cache_io::SystemFileSystem),
    };
    let env = h.env.clone().with_schema_dependencies(dependencies);
    for index in 0..2 {
        let url = format!("https://schemas.example.org/{index}");
        h.resource(&url, json!(true));
        Network::from_env(&env, BTreeMap::new())
            .unwrap()
            .fetch(&url, 100)
            .unwrap();
    }
    let error = Network::from_env(&env, BTreeMap::new())
        .unwrap()
        .fetch(ROOT, 100)
        .unwrap_err();
    assert!(error.to_string().contains("budget exhausted"));
}

#[test]
fn granted_schema_prepares_and_validates_through_real_loopback_tls_adapter() {
    with_tls_server(b"HTTP/1.1 200 OK\r\nContent-Length: 18\r\nConnection: close\r\n\r\n{\"type\":\"integer\"}", |transport| {
        let mut h = Harness::new();
        let dependencies = Dependencies { transport: Arc::new(transport), clock: h.clock.clone(), budget: Arc::default(), filesystem: Arc::new(ryl::rules::schema::net::cache_io::SystemFileSystem) };
        h.env = h.env.clone().with_schema_dependencies(dependencies);
        let context = h.prepare(CONFIG).unwrap();
        assert!(ryl::lint_str("42\n", std::path::Path::new("in.yaml"), &context.config, h.dir.path()).is_empty());
        let errors = ryl::lint_str("wrong\n", std::path::Path::new("in.yaml"), &context.config, h.dir.path());
        assert_eq!(errors.len(), 1);
        println!("granted TLS schema: clean input has no diagnostics; invalid input: {}:{} {}", errors[0].line, errors[0].column, errors[0].message);
    });
}

#[test]
fn toml_pins_are_applied_and_normalized_before_schema_compilation() {
    let h = Harness::new();
    h.resource(ROOT, json!(true));
    let config =
        |pin: &str| format!("{CONFIG}\n[lint.rules.schema.pins]\n'{ROOT}' = '{pin}'\n");
    assert!(
        h.prepare(&config(&format!("sha256:{}", "0".repeat(64))))
            .unwrap_err()
            .contains("pin mismatch")
    );
    h.prepare(&config(
        "sha256:B5BEA41B6C623F7C09F1BF24DCAE58EBAB3C0CDD90AD966BC43A45B44867E12B",
    ))
    .unwrap();
    assert_eq!(format!("{:?}", h.network()), "SchemaNetwork");
    assert_eq!(
        ryl::rules::schema::net::display_url("not a URL"),
        "<invalid schema URL>"
    );
}

#[test]
fn no_cache_environment_disables_schema_cache_even_offline() {
    let h = Harness::new();
    h.resource(ROOT, json!(true));
    h.network().fetch(ROOT, 100).unwrap();
    let env = h
        .env
        .clone()
        .with_var("RYL_NO_CACHE", "1")
        .with_var("RYL_OFFLINE", "1");
    assert!(
        Network::from_env(&env, BTreeMap::new())
            .unwrap()
            .fetch(ROOT, 100)
            .unwrap_err()
            .to_string()
            .contains("offline cache")
    );
}

#[test]
fn remote_resource_limits_invalid_json_and_invalid_uris_fail_preparation() {
    let h = Harness::new();
    h.resource(ROOT, json!(true));
    assert!(
        h.prepare(&CONFIG.repeat(1025))
            .unwrap_err()
            .contains("resource limit")
    );
    std::fs::remove_dir_all(h.dir.path().join("cache")).unwrap();
    h.transport
        .resources
        .lock()
        .unwrap()
        .insert(ROOT.to_owned(), b"{secret".to_vec());
    let error = h.prepare(CONFIG).unwrap_err();
    assert!(error.contains("invalid schema JSON"), "{error}");
    assert!(!error.contains("secret"), "{error}");
    let uri = "https://schemas.example.org/root%GG";
    h.resource(uri, json!(true));
    assert!(
        h.prepare(&format!("[[lint.rules.schema.schemas]]\nurl = '{uri}'\n"))
            .unwrap_err()
            .contains("invalid schema URI")
    );
}

#[test]
fn invalid_user_policy_and_pin_urls_fail_before_retrieval() {
    let mut h = Harness::new();
    let policy = h.dir.path().join("ryl/allowed-schema-origins");
    let env = h
        .env
        .clone()
        .with_config_dir(h.dir.path())
        .with_exists(policy);
    assert!(
        Network::from_env(&env, BTreeMap::new())
            .unwrap_err()
            .to_string()
            .contains("cannot read user schema")
    );
    let pins = BTreeMap::from([(
        "http://schemas.example.org/root".to_owned(),
        format!("sha256:{}", "0".repeat(64)),
    )]);
    assert!(Network::from_env(&h.env, pins).is_err());
    h.env = h
        .env
        .clone()
        .with_var("RYL_SCHEMA_ALLOWED_ORIGINS", "http://schemas.example.org");
    std::fs::write(
        h.dir.path().join("local.json"),
        format!("{{\"$ref\":\"{ROOT}\"}}"),
    )
    .unwrap();
    assert!(
        h.prepare("[[lint.rules.schema.schemas]]\npath = 'local.json'\n")
            .unwrap_err()
            .contains("require HTTPS")
    );
    assert!(h.transport.calls.lock().unwrap().is_empty());
}

#[derive(Debug)]
struct FailedAddresses;
impl ryl::rules::schema::net::http::AddressSource for FailedAddresses {
    fn resolve(
        &self,
        _: &str,
        _: std::time::Instant,
    ) -> std::io::Result<Vec<std::net::SocketAddr>> {
        Err(std::io::ErrorKind::TimedOut.into())
    }
}

#[test]
fn resolver_rejects_relative_uris_and_propagates_lookup_errors() {
    use ryl::rules::schema::net::http::CheckedResolver;
    use ureq::unversioned::{resolver::Resolver, transport::NextTimeout};
    let resolver = CheckedResolver::new(Arc::new(FailedAddresses));
    for raw in ["/relative", "https://schemas.example.org/root"] {
        assert!(
            resolver
                .resolve(
                    &raw.parse().unwrap(),
                    &ureq::config::Config::default(),
                    NextTimeout::default()
                )
                .is_err()
        );
    }
    for ip in ["192.0.1.1", "2001:4860:4860::8888"] {
        let resolver = CheckedResolver::new(Arc::new(Addresses(Mutex::new(vec![
            std::net::SocketAddr::new(ip.parse().unwrap(), 443),
        ]))));
        assert_eq!(
            resolver
                .resolve(
                    &"https://schemas.example.org/root".parse().unwrap(),
                    &ureq::config::Config::default(),
                    NextTimeout::default()
                )
                .unwrap()
                .len(),
            1
        );
    }
}

#[test]
fn redirects_without_a_location_are_still_errors() {
    assert!(
        tls_response(
            b"HTTP/1.1 302 Found\r\nContent-Length: 0\r\n\r\n",
            Duration::from_secs(2)
        )
        .unwrap_err()
        .to_string()
        .contains("invalid location")
    );
}
