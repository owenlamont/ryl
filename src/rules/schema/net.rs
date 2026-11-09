use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, UNIX_EPOCH};
#[cfg(feature = "schema-network")]
use std::time::{Instant, SystemTime};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use url::Url;

use crate::config::Env;

pub mod cache_io;
pub mod progress;

const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_CACHE: u64 = 256 * 1024 * 1024;
const FRESH: u64 = 24 * 60 * 60;

#[derive(Debug, Default)]
pub struct Options {
    pub allowed_origins: Vec<String>,
    pub offline: bool,
    pub no_cache: bool,
    pub no_progress: bool,
}

static OPTIONS: OnceLock<Options> = OnceLock::new();
static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn configure(options: Options) {
    let _ = OPTIONS.set(options);
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Policy(&'static str),
    #[error("{url}: {reason}")]
    Resource { url: String, reason: String },
    #[cfg(feature = "schema-network")]
    #[error(
        "{url}: schema HTTPS request failed (destination policy, DNS, TLS, timeout or transport)"
    )]
    Request {
        url: String,
        #[source]
        source: Box<ureq::Error>,
    },
    #[error("{url}: schema body truncated, timed out or unreadable")]
    Body {
        url: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{url}: cannot write schema cache")]
    CacheWrite {
        url: String,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot inspect schema cache path")]
    CacheInspect {
        #[source]
        source: std::io::Error,
    },
    #[error("{source}; {variables} set; ryl does not use proxies")]
    Proxy {
        variables: String,
        #[source]
        source: Box<Error>,
    },
}

impl Error {
    #[must_use]
    pub fn transport(url: &Url) -> Self {
        Self::at(url, "schema transport failed")
    }
    fn at(url: &Url, reason: &str) -> Self {
        Self::Resource {
            url: display_url(url.as_str()),
            reason: reason.to_owned(),
        }
    }
}

#[must_use]
pub fn display_url(raw: &str) -> String {
    let Ok(mut url) = Url::parse(raw) else {
        return "<invalid schema URL>".to_owned();
    };
    let _ = url.set_username("");
    let _ = url.set_password(None);
    if url.query().is_some() {
        let keys: Vec<_> = url.query_pairs().map(|(key, _)| key.into_owned()).collect();
        url.query_pairs_mut()
            .clear()
            .extend_pairs(keys.iter().map(|key| (key.as_str(), "***")));
    }
    url.set_fragment(None);
    crate::cli_support::sanitize_control(url.as_str()).into_owned()
}

#[must_use]
pub fn redact_message(text: &str) -> String {
    static PATH_URLS: OnceLock<regex::Regex> = OnceLock::new();
    static URLS: OnceLock<regex::Regex> = OnceLock::new();
    let text = crate::cli_support::sanitize_control(text);
    let path_urls = PATH_URLS.get_or_init(|| {
        regex::Regex::new(
            r"`(?P<prefix>[^`\r\n]*?)(?P<url>[A-Za-z][A-Za-z0-9+.-]*://[^\r\n]*)`",
        )
        .expect("URL key path pattern")
    });
    let text = path_urls.replace_all(&text, |captures: &regex::Captures<'_>| {
        format!("`{}{}`", &captures["prefix"], display_url(&captures["url"]))
    });
    let urls = URLS.get_or_init(|| {
        regex::Regex::new(r#"[A-Za-z][A-Za-z0-9+.-]*://[^\s\"'<>`]+"#)
            .expect("URL token pattern")
    });
    urls.replace_all(&text, |captures: &regex::Captures<'_>| {
        display_url(&captures[0])
    })
    .into_owned()
}

#[cfg(feature = "schema-network")]
fn env_flag(env: &dyn Env, key: &str) -> bool {
    env.env_var(key)
        .is_some_and(|value| !value.is_empty() && value != "0")
}

fn canonical(raw: &str) -> Result<Url, Error> {
    let mut url = Url::parse(raw).map_err(|_| Error::Policy("invalid schema URL"))?;
    if url.scheme() != "https"
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.host_str().is_none_or(|host| host.contains('*'))
    {
        return Err(Error::at(
            &url,
            "schemas require HTTPS on port 443 without userinfo",
        ));
    }
    url.set_fragment(None);
    Ok(url)
}

#[cfg(feature = "schema-network")]
fn origin(raw: &str) -> Result<String, Error> {
    let url = canonical(raw)?;
    if url.path() != "/"
        || url.query().is_some()
        || Url::parse(raw)
            .expect("validated origin URL")
            .fragment()
            .is_some()
    {
        return Err(Error::Policy(
            "schema grants must be exact HTTPS origins without a path, query or fragment",
        ));
    }
    Ok(url.origin().ascii_serialization())
}

pub trait Clock: Send + Sync {
    fn unix_seconds(&self) -> u64;
    fn monotonic(&self) -> Duration;
}

#[cfg(feature = "schema-network")]
struct SystemClock;
#[cfg(feature = "schema-network")]
impl Clock for SystemClock {
    fn monotonic(&self) -> Duration {
        static START: OnceLock<Instant> = OnceLock::new();
        START.get_or_init(Instant::now).elapsed()
    }
    fn unix_seconds(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }
}

pub trait Transport: Send + Sync {
    /// # Errors
    /// Returns a redacted failure for transport, framing or size errors.
    fn get(&self, url: &Url, timeout: Duration, limit: usize)
    -> Result<Vec<u8>, Error>;
}

pub struct Network {
    grants: BTreeSet<String>,
    pins: BTreeMap<String, String>,
    cache: Option<PathBuf>,
    offline: bool,
    no_progress: bool,
    proxy_vars: Vec<&'static str>,
    dependencies: Dependencies,
}

#[derive(Default)]
struct Budget {
    requests: usize,
    elapsed: Duration,
}
#[derive(Default)]
pub struct NetworkBudget(Mutex<Budget>);

#[derive(Clone)]
pub struct Dependencies {
    pub transport: Arc<dyn Transport>,
    pub clock: Arc<dyn Clock>,
    pub budget: Arc<NetworkBudget>,
    pub filesystem: Arc<dyn cache_io::FileSystem>,
}

#[cfg(feature = "schema-network")]
static BUDGET: std::sync::LazyLock<Arc<NetworkBudget>> =
    std::sync::LazyLock::new(Arc::default);

impl Network {
    /// # Errors
    /// Returns an error because this build lacks schema networking.
    #[cfg(not(feature = "schema-network"))]
    pub fn from_env(
        env: &dyn Env,
        pins: BTreeMap<String, String>,
    ) -> Result<Self, Error> {
        let _ = (env, pins);
        Err(Error::Policy(
            "this build lacks schema network support (enable schema-network)",
        ))
    }

    /// # Errors
    /// Rejects invalid grants or unreadable user policy.
    #[cfg(feature = "schema-network")]
    pub fn from_env(
        env: &dyn Env,
        pins: BTreeMap<String, String>,
    ) -> Result<Self, Error> {
        let transport = Arc::new(http::Http::default());
        let options = OPTIONS.get_or_init(Options::default);
        let mut grants = BTreeSet::new();
        for raw in &options.allowed_origins {
            grants.insert(origin(raw)?);
        }
        if let Some(value) = env.env_var("RYL_SCHEMA_ALLOWED_ORIGINS") {
            for raw in value.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                grants.insert(origin(raw)?);
            }
        }
        if let Some(path) = env
            .config_dir()
            .map(|p| p.join("ryl/allowed-schema-origins"))
            && env.path_exists(&path)
        {
            let text = env.read_to_string(&path).map_err(|_| {
                Error::Policy("cannot read user schema origin allowlist")
            })?;
            for line in text.lines().map(str::trim).filter(|s| !s.is_empty()) {
                grants.insert(origin(line)?);
            }
        }
        let cache = if options.no_cache || env_flag(env, "RYL_NO_CACHE") {
            None
        } else {
            env.env_var("RYL_SCHEMA_CACHE_DIR")
                .map(PathBuf::from)
                .or_else(|| dirs_next::cache_dir().map(|p| p.join("ryl/schemas/v1")))
        };
        let mut normalized_pins = BTreeMap::new();
        for (raw, pin) in pins {
            normalized_pins.insert(canonical(&raw)?.to_string(), pin);
        }
        Ok(Self {
            grants,
            pins: normalized_pins,
            cache,
            offline: options.offline || env_flag(env, "RYL_OFFLINE"),
            no_progress: options.no_progress,
            proxy_vars: [
                "HTTPS_PROXY",
                "https_proxy",
                "HTTP_PROXY",
                "http_proxy",
                "ALL_PROXY",
                "all_proxy",
            ]
            .into_iter()
            .filter(|key| env.env_var(key).is_some())
            .collect(),
            dependencies: env.schema_dependencies().unwrap_or_else(|| Dependencies {
                transport,
                clock: Arc::new(SystemClock),
                budget: Arc::clone(&BUDGET),
                filesystem: Arc::new(cache_io::SystemFileSystem),
            }),
        })
    }

    /// # Errors
    /// Rejects denied origins, resource limits, cache failures and transport failures.
    pub fn fetch(&self, raw: &str, limit: usize) -> Result<Vec<u8>, Error> {
        let limit = limit.min(MAX_BYTES);
        let url = canonical(raw)?;
        let origin = url.origin().ascii_serialization();
        if !self.grants.contains(&origin) {
            return Err(Error::at(
                &url,
                &format!(
                    "origin not granted; pass --schema-allow-origin {origin} or add it to RYL_SCHEMA_ALLOWED_ORIGINS"
                ),
            ));
        }
        let pin = self.pins.get(url.as_str());
        if let Some((meta, bytes)) = self.read_cache(&url, limit)? {
            let pinned = Self::verify_pin(&url, pin, &bytes);
            if self.offline {
                return pinned.map(|()| bytes);
            }
            let fresh = self
                .dependencies
                .clock
                .unix_seconds()
                .checked_sub(meta.fetched)
                .is_some_and(|age| age < FRESH);
            if pinned.is_ok() && (pin.is_some() || fresh) {
                return Ok(bytes);
            }
        }
        if self.offline {
            return Err(Error::at(&url, "schema not available in offline cache"));
        }
        let mut budget = self
            .dependencies
            .budget
            .0
            .lock()
            .expect("network budget lock");
        if budget.requests >= 64 || budget.elapsed >= Duration::from_secs(60) {
            return Err(Error::at(
                &url,
                "process schema network budget exhausted (64 requests / 60 seconds)",
            ));
        }
        budget.requests += 1;
        let timeout = Duration::from_secs(60)
            .checked_sub(budget.elapsed)
            .expect("budget elapsed is under 60 seconds after the exhaustion check")
            .min(Duration::from_secs(30));
        if !self.no_progress {
            eprintln!("Downloading schema {}", display_url(url.as_str()));
        }
        let started = self.dependencies.clock.monotonic();
        let result = self.dependencies.transport.get(&url, timeout, limit);
        budget.elapsed += self.dependencies.clock.monotonic().saturating_sub(started);
        drop(budget);
        let bytes = result.map_err(|error| {
            if self.proxy_vars.is_empty() {
                error
            } else {
                Error::Proxy {
                    variables: self.proxy_vars.join(", "),
                    source: Box::new(error),
                }
            }
        })?;
        if bytes.len() > limit {
            return Err(Error::at(
                &url,
                "schema size limit exceeded (16 MiB including references)",
            ));
        }
        Self::verify_pin(&url, pin, &bytes)?;
        self.write_cache(&url, &bytes)?;
        Ok(bytes)
    }

    fn verify_pin(url: &Url, pin: Option<&String>, bytes: &[u8]) -> Result<(), Error> {
        if pin.is_some_and(|pin| pin != &format!("sha256:{}", hash(bytes))) {
            return Err(Error::at(url, "schema sha256 pin mismatch"));
        }
        Ok(())
    }

    fn read_cache(
        &self,
        url: &Url,
        limit: usize,
    ) -> Result<Option<(Metadata, Vec<u8>)>, Error> {
        let Some(root) = &self.cache else {
            return Ok(None);
        };
        safe_path(self.dependencies.filesystem.as_ref(), root)?;
        let path = root.join(cache_key(url));
        safe_path(self.dependencies.filesystem.as_ref(), &path)?;
        let Ok(file) = self.dependencies.filesystem.open(&path) else {
            return Ok(None);
        };
        let mut stored = Vec::new();
        if file
            .take((MAX_BYTES + 8193) as u64)
            .read_to_end(&mut stored)
            .is_err()
        {
            return Ok(None);
        }
        let Some(split) = stored
            .iter()
            .position(|b| *b == b'\n')
            .filter(|n| *n < 8192)
        else {
            return Ok(None);
        };
        let Ok(meta) = serde_json::from_slice::<Metadata>(&stored[..split]) else {
            return Ok(None);
        };
        let bytes = &stored[split + 1..];
        if meta.url != url.as_str()
            || meta.sha256 != hash(bytes)
            || bytes.len() > MAX_BYTES
        {
            return Ok(None);
        }
        if bytes.len() > limit {
            return Err(Error::at(
                url,
                "schema size limit exceeded (16 MiB including references)",
            ));
        }
        Ok(Some((meta, bytes.to_vec())))
    }

    fn write_cache(&self, url: &Url, bytes: &[u8]) -> Result<(), Error> {
        let Some(root) = &self.cache else {
            return Ok(());
        };
        let io_error = |source| Error::CacheWrite {
            url: display_url(url.as_str()),
            source,
        };
        safe_path(self.dependencies.filesystem.as_ref(), root)?;
        self.dependencies
            .filesystem
            .create_dirs(root)
            .map_err(io_error)?;
        safe_path(self.dependencies.filesystem.as_ref(), root)?;
        let meta = Metadata {
            url: url.to_string(),
            sha256: hash(bytes),
            fetched: self.dependencies.clock.unix_seconds(),
        };
        let header = serde_json::to_vec(&meta).expect("schema metadata serializes");
        let size =
            u64::try_from(header.len() + 1 + bytes.len()).expect("bounded cache entry");
        let mut entries = Vec::new();
        let mut total = 0;
        for entry in self
            .dependencies
            .filesystem
            .entries(root)
            .map_err(io_error)?
        {
            let entry = entry.map_err(io_error)?;
            let metadata = self
                .dependencies
                .filesystem
                .metadata(&entry)
                .map_err(io_error)?;
            if crate::cli_support::is_symlink_or_junction(&metadata) {
                return Err(Error::Policy("schema cache refuses symlinks"));
            }
            if metadata.is_file() {
                total += metadata.len();
                entries.push((
                    metadata.modified().unwrap_or(UNIX_EPOCH),
                    entry,
                    metadata.len(),
                ));
            }
        }
        entries.sort();
        for (_, path, length) in entries {
            if total + size <= MAX_CACHE {
                break;
            }
            self.dependencies
                .filesystem
                .remove(&path)
                .map_err(io_error)?;
            total -= length;
        }
        let temp = root.join(format!(
            ".{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let path = root.join(cache_key(url));
        safe_path(self.dependencies.filesystem.as_ref(), &path)?;
        let result = (|| {
            let mut file = self.dependencies.filesystem.create_new(&temp)?;
            file.write_all(&header)?;
            file.write_all(b"\n")?;
            file.write_all(bytes)?;
            drop(file);
            self.dependencies.filesystem.rename(&temp, &path)
        })();
        if result.is_err() {
            let _ = self.dependencies.filesystem.remove(&temp);
        }
        result.map_err(io_error)
    }
}

#[derive(Deserialize, Serialize)]
struct Metadata {
    url: String,
    sha256: String,
    fetched: u64,
}

fn cache_key(url: &Url) -> String {
    hash(url.as_str().as_bytes())
}

fn hash(bytes: &[u8]) -> String {
    crate::report::hex_digest(&Sha256::digest(bytes))
}

fn safe_path(filesystem: &dyn cache_io::FileSystem, path: &Path) -> Result<(), Error> {
    for parent in path.ancestors() {
        match filesystem.metadata(parent) {
            Ok(meta) if crate::cli_support::is_symlink_or_junction(&meta) => {
                return Err(Error::Policy("schema cache refuses symlinks"));
            }
            Ok(_) => (),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(source) => return Err(Error::CacheInspect { source }),
        }
    }
    Ok(())
}

#[cfg(feature = "schema-network")]
pub mod http {
    use super::{Duration, Error, Transport, Url};
    use std::io::Read;
    use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
    use std::sync::{OnceLock, mpsc};
    use ureq::unversioned::resolver::{ResolvedSocketAddrs, Resolver};
    use ureq::unversioned::transport::{DefaultConnector, NextTimeout};

    type Reply = mpsc::SyncSender<std::io::Result<Vec<SocketAddr>>>;
    struct Job {
        host: String,
        reply: Reply,
        deadline: std::time::Instant,
    }
    static WORKER: OnceLock<std::sync::Arc<ResolverWorker>> = OnceLock::new();

    pub trait AddressSource: std::fmt::Debug + Send + Sync {
        /// # Errors
        /// Returns lookup, deadline or resolver saturation failures.
        fn resolve(
            &self,
            host: &str,
            deadline: std::time::Instant,
        ) -> std::io::Result<Vec<SocketAddr>>;
    }

    pub trait Lookup: Send {
        /// # Errors
        /// Returns operating-system name resolution failures.
        fn lookup(&self, host: &str) -> std::io::Result<Vec<SocketAddr>>;
    }
    struct OsLookup;
    impl Lookup for OsLookup {
        fn lookup(&self, host: &str) -> std::io::Result<Vec<SocketAddr>> {
            (host, 443).to_socket_addrs().map(Iterator::collect)
        }
    }

    #[derive(Debug)]
    pub struct ResolverWorker {
        sender: mpsc::SyncSender<Job>,
    }
    impl ResolverWorker {
        #[must_use]
        pub fn new(lookup: Box<dyn Lookup>) -> Self {
            let (sender, receiver) = mpsc::sync_channel::<Job>(1);
            std::thread::spawn(move || run_lookups(receiver, lookup.as_ref()));
            Self { sender }
        }
    }
    fn run_lookups(receiver: mpsc::Receiver<Job>, lookup: &dyn Lookup) {
        for job in receiver {
            if std::time::Instant::now() >= job.deadline {
                continue;
            }
            let result = lookup.lookup(&job.host);
            let _ = job.reply.try_send(result);
        }
    }

    impl AddressSource for ResolverWorker {
        fn resolve(
            &self,
            host: &str,
            deadline: std::time::Instant,
        ) -> std::io::Result<Vec<SocketAddr>> {
            let (reply, rx) = mpsc::sync_channel(1);
            self.sender
                .try_send(Job {
                    host: host.to_owned(),
                    reply,
                    deadline,
                })
                .map_err(|_| std::io::Error::from(std::io::ErrorKind::WouldBlock))?;
            rx.recv_timeout(
                deadline.saturating_duration_since(std::time::Instant::now()),
            )
            .map_err(|_| std::io::Error::from(std::io::ErrorKind::TimedOut))?
        }
    }

    #[derive(Debug)]
    pub struct CheckedResolver {
        source: std::sync::Arc<dyn AddressSource>,
    }
    impl CheckedResolver {
        pub fn new(source: std::sync::Arc<dyn AddressSource>) -> Self {
            Self { source }
        }
    }
    impl Resolver for CheckedResolver {
        fn resolve(
            &self,
            uri: &ureq::http::Uri,
            _: &ureq::config::Config,
            timeout: NextTimeout,
        ) -> Result<ResolvedSocketAddrs, ureq::Error> {
            let duration = timeout
                .not_zero()
                .map_or(Duration::from_secs(5), |d| *d)
                .min(Duration::from_secs(5));
            let all = self.source.resolve(
                uri.host()
                    .ok_or(ureq::Error::HostNotFound)?
                    .trim_matches(['[', ']']),
                std::time::Instant::now() + duration,
            )?;
            if all.is_empty() || all.iter().any(|addr| !public(addr.ip())) {
                return Err(ureq::Error::HostNotFound);
            }
            let mut result = self.empty();
            for addr in all.into_iter().take(16) {
                result.push(addr);
            }
            Ok(result)
        }
    }

    fn public(ip: IpAddr) -> bool {
        match ip {
            IpAddr::V4(ip) => {
                let [a, b, c, _] = ip.octets();
                !(matches!(a, 0 | 10 | 127 | 224..=255)
                    || a == 100 && (64..=127).contains(&b)
                    || a == 169 && b == 254
                    || a == 172 && (16..=31).contains(&b)
                    || a == 192
                        && (b == 0 && matches!(c, 0 | 2)
                            || b == 168
                            || b == 88 && c == 99)
                    || a == 198 && (matches!(b, 18 | 19) || b == 51 && c == 100)
                    || a == 203 && b == 0 && c == 113)
            }
            IpAddr::V6(ip) => {
                let s = ip.segments();
                (0x2000..0x4000).contains(&s[0])
                    && s[0] != 0x2002
                    && !(s[0] == 0x2001 && (s[1] < 0x200 || matches!(s[1], 0xdb8)))
                    && !(s[0] == 0x3fff && s[1] < 0x1000)
            }
        }
    }

    pub trait AgentFactory: Send + Sync {
        fn tls_config(&self) -> ureq::tls::TlsConfig {
            ureq::tls::TlsConfig::default()
        }
        fn build(&self, config: ureq::config::Config) -> ureq::Agent;
    }
    struct StandardAgent;
    impl AgentFactory for StandardAgent {
        fn build(&self, config: ureq::config::Config) -> ureq::Agent {
            ureq::Agent::with_parts(
                config,
                DefaultConnector::default(),
                CheckedResolver::new(
                    WORKER
                        .get_or_init(|| {
                            std::sync::Arc::new(ResolverWorker::new(Box::new(OsLookup)))
                        })
                        .clone(),
                ),
            )
        }
    }
    pub struct Http {
        factory: std::sync::Arc<dyn AgentFactory>,
    }
    impl Default for Http {
        fn default() -> Self {
            Self {
                factory: std::sync::Arc::new(StandardAgent),
            }
        }
    }
    impl Http {
        pub fn new(factory: std::sync::Arc<dyn AgentFactory>) -> Self {
            Self { factory }
        }
    }
    impl Transport for Http {
        fn get(
            &self,
            url: &Url,
            timeout: Duration,
            limit: usize,
        ) -> Result<Vec<u8>, Error> {
            let config = ureq::Agent::config_builder()
                .tls_config(self.factory.tls_config())
                .proxy(None)
                .max_redirects(0)
                .max_redirects_will_error(false)
                .http_status_as_error(false)
                .max_idle_connections(0)
                .max_idle_connections_per_host(0)
                .max_response_header_size(32 * 1024)
                .timeout_global(Some(timeout))
                .timeout_connect(Some(Duration::from_secs(5)))
                .timeout_resolve(Some(Duration::from_secs(5)))
                .build();
            let agent = self.factory.build(config);
            let mut response = agent
                .get(url.as_str())
                .header("Accept-Encoding", "identity")
                .call()
                .map_err(|source| Error::Request {
                    url: super::display_url(url.as_str()),
                    source: Box::new(source),
                })?;
            if response.status().is_redirection() {
                let target = response
                    .headers()
                    .get("Location")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|raw| url.join(raw).ok());
                return Err(Error::at(
                    url,
                    &format!(
                        "schema redirect refused to {}",
                        target.map_or_else(
                            || "<invalid location>".to_owned(),
                            |url| super::display_url(url.as_str())
                        )
                    ),
                ));
            }
            if !response.status().is_success() {
                return Err(Error::at(
                    url,
                    &format!("schema HTTP status {}", response.status().as_u16()),
                ));
            }
            if response
                .headers()
                .get_all("Content-Encoding")
                .iter()
                .any(|v| v != "identity")
            {
                return Err(Error::at(url, "encoded schema response refused"));
            }
            let mut bytes = Vec::new();
            response
                .body_mut()
                .as_reader()
                .take((limit + 1) as u64)
                .read_to_end(&mut bytes)
                .map_err(|source| Error::Body {
                    url: super::display_url(url.as_str()),
                    source,
                })?;
            if bytes.len() > limit {
                return Err(Error::at(
                    url,
                    "schema size limit exceeded (16 MiB including references)",
                ));
            }
            Ok(bytes)
        }
    }
}

impl fmt::Debug for Network {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SchemaNetwork")
    }
}
