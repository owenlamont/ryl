use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use jsonschema::{Retrieve, Uri};
use serde_json::Value;
use url::Url;

use super::{Association, ID, Prepared};
use crate::config::{Env, PathGlob, YamlLintConfig};
use crate::yaml_dom::YamlOwned;

const MAX_SCHEMA_BYTES: usize = 16 * 1024 * 1024;
const MAX_SCHEMA_RESOURCES: usize = 1024;

#[derive(Debug, thiserror::Error)]
enum ReadError {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("schema path escapes config base directory")]
    Escape,
    #[error("{}", super::net::RESOURCE_LIMIT)]
    ResourceLimit,
    #[error("{}", super::net::SIZE_LIMIT)]
    ByteLimit,
    #[error("{0}")]
    Remote(String),
    #[error(transparent)]
    Network(#[from] super::net::Error),
    #[error("{url}: invalid schema JSON")]
    RemoteJson {
        url: String,
        #[source]
        source: serde_json::Error,
    },
}

#[derive(Default)]
struct Files {
    bytes: usize,
    contents: BTreeMap<String, Vec<u8>>,
    resources: usize,
}

#[derive(Clone)]
struct LocalRetriever {
    base: PathBuf,
    files: Arc<Mutex<Files>>,
    network: Arc<OnceLock<super::net::Network>>,
    needs_network: Arc<Mutex<bool>>,
    failure: Arc<Mutex<Option<String>>>,
    allow_files: bool,
    dependencies: Option<Arc<Mutex<std::collections::BTreeSet<PathBuf>>>>,
}

impl LocalRetriever {
    fn read(&self, path: &Path) -> Result<Value, ReadError> {
        let path = path.canonicalize().map_err(|source| ReadError::Io {
            path: path.to_owned(),
            source,
        })?;
        if !path.starts_with(&self.base) {
            return Err(ReadError::Escape);
        }
        let mut files = self
            .files
            .lock()
            .expect("schema reader never panics while holding its lock");
        if files.resources >= MAX_SCHEMA_RESOURCES {
            return Err(ReadError::ResourceLimit);
        }
        let remaining = MAX_SCHEMA_BYTES - files.bytes;
        #[cfg(unix)]
        let identity = Url::from_file_path(&path)
            .expect("canonical schema path is absolute")
            .to_string();
        #[cfg(not(unix))]
        let identity = Url::from_file_path(&path)
            .map_err(|()| {
                ReadError::Remote(
                    "schema path cannot be represented as a file URI".to_owned(),
                )
            })?
            .to_string();
        if let Some(dependencies) = &self.dependencies {
            dependencies
                .lock()
                .expect("schema dependencies lock")
                .insert(
                    Url::parse(&identity)
                        .expect("canonical schema file URI")
                        .to_file_path()
                        .expect("canonical schema filesystem path"),
                );
        }
        if let Some(bytes) = files.contents.get(&identity) {
            let size = bytes.len();
            if size > remaining {
                return Err(ReadError::ByteLimit);
            }
            let value = serde_json::from_slice(bytes)
                .expect("cached schema bytes already parsed successfully");
            files.bytes += size;
            files.resources += 1;
            return Ok(value);
        }
        let mut bytes = Vec::new();
        std::fs::File::open(&path)
            .and_then(|file| file.take((remaining + 1) as u64).read_to_end(&mut bytes))
            .map_err(|source| ReadError::Io {
                path: path.clone(),
                source,
            })?;
        if bytes.len() > remaining {
            return Err(ReadError::ByteLimit);
        }
        let value =
            serde_json::from_slice(&bytes).map_err(|source| ReadError::Json {
                path: path.clone(),
                source,
            })?;
        files.bytes += bytes.len();
        files.contents.insert(identity, bytes);
        files.resources += 1;
        Ok(value)
    }

    fn remote(&self, raw: &str) -> Result<Value, ReadError> {
        let Some(network) = self.network.get() else {
            *self.needs_network.lock().expect("schema network lock") = true;
            return Err(ReadError::Remote(
                "schema network preparation required".to_owned(),
            ));
        };
        let mut files = self.files.lock().expect("schema reader lock");
        if files.resources >= MAX_SCHEMA_RESOURCES {
            return Err(ReadError::ResourceLimit);
        }
        let bytes = network
            .fetch(raw, MAX_SCHEMA_BYTES - files.bytes)
            .map_err(ReadError::from)?;
        let value =
            serde_json::from_slice(&bytes).map_err(|source| ReadError::RemoteJson {
                url: super::net::display_url(raw),
                source,
            })?;
        files.bytes += bytes.len();
        files.resources += 1;
        let mut identity = Url::parse(raw).expect("validated URL");
        identity.set_fragment(None);
        files.contents.insert(identity.to_string(), bytes);
        Ok(value)
    }

    fn resource(
        &self,
        uri: &Uri<String>,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let url = Url::parse(uri.as_str())?;
        if url.scheme() != "file" {
            return self.remote(uri.as_str()).map_err(Into::into);
        }
        if !self.allow_files {
            return Err("remote schema cannot read file references".into());
        }
        if url.host_str().is_some_and(|host| !host.is_empty()) {
            return Err("schema UNC references are not supported".into());
        }
        #[cfg(unix)]
        let path = url
            .to_file_path()
            .expect("a hostless file URL has a Unix filesystem path");
        #[cfg(not(unix))]
        let path = url
            .to_file_path()
            .map_err(|()| "invalid schema file reference")?;
        if let Some(dependencies) = &self.dependencies {
            dependencies
                .lock()
                .expect("schema dependencies lock")
                .insert(path.clone());
        }
        self.read(&path).map_err(Into::into)
    }
}

impl LocalRetriever {
    fn compile(
        &self,
        path: &str,
        remote: bool,
        files: Vec<PathGlob>,
    ) -> Result<Association, String> {
        let (schema, uri) = if remote {
            let schema = self.remote(path).map_err(|error| error.to_string())?;
            let mut url = Url::parse(path).expect("retrieved schema URL is valid");
            url.set_fragment(None);
            let uri = jsonschema::uri::from_str(url.as_str())
                .map_err(|_| "invalid schema URI".to_owned())?;
            (schema, uri)
        } else {
            let (root, uri) = root_location(&self.base, path)?;
            (self.read(&root).map_err(|error| error.to_string())?, uri)
        };
        let registry = jsonschema::Registry::new()
            .retriever(self.clone())
            .add(uri.as_str(), schema.clone())
            .and_then(jsonschema::RegistryBuilder::prepare)
            .map_err(|error| error.to_string())?;
        super::complexity::check(&registry, uri.clone())?;
        let validator = jsonschema::options()
            .with_base_uri(uri.as_str())
            .with_registry(&registry)
            .with_retriever(self.clone())
            .with_pattern_options(
                jsonschema::PatternOptions::fancy_regex()
                    .backtrack_limit(100_000)
                    .size_limit(1_048_576)
                    .dfa_size_limit(1_048_576),
            )
            .build(&schema)
            .map_err(|error| error.to_string())?;
        Ok(Association {
            files,
            validator: Arc::new(validator),
            registry: Arc::new(registry),
            root_uri: uri,
        })
    }
}

impl Retrieve for LocalRetriever {
    fn retrieve(
        &self,
        uri: &Uri<String>,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let result = self.resource(uri);
        if let Err(error) = &result {
            *self.failure.lock().expect("schema error lock") = Some(error.to_string());
        }
        result
    }
}

fn root_location(base: &Path, path: &str) -> Result<(PathBuf, Uri<String>), String> {
    if !Path::new(path).components().all(|part| {
        matches!(
            part,
            Component::Normal(_) | Component::CurDir | Component::ParentDir
        )
    }) || path.starts_with('\\')
    {
        return Err(
            "schema root path must be relative to config base directory".to_owned()
        );
    }
    let root = base.join(path);
    #[cfg(unix)]
    let uri = Url::from_file_path(&root).expect("schema path is absolute");
    #[cfg(not(unix))]
    let uri = Url::from_file_path(&root)
        .map_err(|()| "schema path cannot be represented as a file URI")?;
    // WHATWG file paths leave these characters unescaped, unlike RFC 3986 URI paths.
    let uri = uri
        .as_str()
        .replace('[', "%5B")
        .replace(']', "%5D")
        .replace('^', "%5E")
        .replace('|', "%7C");
    Ok((
        root,
        jsonschema::uri::from_str(&uri)
            .expect("percent-encoded file URL is an RFC 3986 URI"),
    ))
}

fn enable_network(
    network: &OnceLock<super::net::Network>,
    cfg: &YamlLintConfig,
    envx: &dyn Env,
) -> Result<Option<super::net::progress::Progress>, String> {
    let pins = cfg
        .rule_option(ID, "pins")
        .and_then(YamlOwned::as_mapping)
        .into_iter()
        .flatten()
        .map(|(url, pin)| {
            (
                url.as_str().expect("typed pin URL").to_owned(),
                pin.as_str().expect("typed pin digest").to_ascii_lowercase(),
            )
        })
        .collect();
    let value = super::net::Network::from_env(envx, pins).map_err(|e| e.to_string())?;
    network
        .set(value)
        .expect("network initialized only once during preparation");
    Ok(super::net::progress::terminal())
}

fn record_roots(
    dependencies: &Mutex<std::collections::BTreeSet<PathBuf>>,
    entries: &[YamlOwned],
    base: &Path,
) -> Result<(), String> {
    let mut dependencies = dependencies.lock().expect("schema dependencies lock");
    for path in entries
        .iter()
        .filter_map(|entry| entry.as_mapping_get("path").and_then(YamlOwned::as_str))
    {
        let (_, uri) = root_location(base, path)?;
        dependencies.insert(
            Url::parse(uri.as_str())
                .expect("root file URI")
                .to_file_path()
                .expect("root filesystem path"),
        );
    }
    Ok(())
}

pub(crate) fn prepare(
    cfg: &YamlLintConfig,
    base: &Path,
    envx: &dyn Env,
) -> Result<super::Preparation, String> {
    let Some(entries) = cfg
        .rule_option(ID, "schemas")
        .and_then(YamlOwned::as_sequence)
        .filter(|entries| !entries.is_empty())
    else {
        return Ok(super::Preparation::Ready(None));
    };
    let absolute_base = envx.current_dir().join(base);
    let canonical_base = absolute_base
        .canonicalize()
        .map_err(|error| format!("invalid config: schema base: {error}"))?;
    let dependencies = envx.local_schema_dependencies();
    if let Some(dependencies) = &dependencies {
        record_roots(dependencies, entries, &canonical_base)?;
    }
    let mut digest = blake3::Hasher::new();
    let mut associations = Vec::new();
    let loaded = Arc::new(Mutex::new(Files::default()));
    let network = Arc::new(OnceLock::new());
    let mut _progress = None;
    for entry in entries {
        let remote = entry.as_mapping_get("url").and_then(YamlOwned::as_str);
        let path = remote
            .or_else(|| entry.as_mapping_get("path").and_then(YamlOwned::as_str))
            .expect("typed schema location");
        let label = remote.map_or_else(|| path.to_owned(), super::net::display_url);
        if remote.is_some() && !envx.remote_schemas() {
            return Ok(super::Preparation::NeedsRemotePreparation);
        }
        if remote.is_some() && network.get().is_none() {
            _progress = enable_network(&network, cfg, envx).map_err(|error| {
                format!(
                    "invalid config: schema '{label}': {}",
                    super::net::redact_message(&error)
                )
            })?;
        }
        let reader = LocalRetriever {
            base: canonical_base.clone(),
            files: Arc::clone(&loaded),
            network: Arc::clone(&network),
            needs_network: Arc::default(),
            failure: Arc::default(),
            allow_files: remote.is_none(),
            dependencies: dependencies.clone(),
        };
        let files: Vec<PathGlob> = entry
            .as_mapping_get("files")
            .and_then(YamlOwned::as_sequence)
            .into_iter()
            .flatten()
            .filter_map(YamlOwned::as_str)
            .map(|glob| {
                PathGlob::new(glob, &absolute_base, envx.case_insensitive_paths())
                    .expect("validated schema files glob")
            })
            .collect();
        let build = || reader.compile(path, remote.is_some(), files.clone());
        let charged = {
            let loaded = loaded.lock().expect("schema reader lock");
            (loaded.bytes, loaded.resources)
        };
        let mut result = build();
        if *reader.needs_network.lock().expect("schema network lock") {
            if !envx.remote_schemas() {
                return Ok(super::Preparation::NeedsRemotePreparation);
            }
            let mut retry = loaded.lock().expect("schema reader lock");
            (retry.bytes, retry.resources) = charged;
            drop(retry);
            _progress = enable_network(&network, cfg, envx).map_err(|error| {
                format!(
                    "invalid config: schema '{label}': {}",
                    super::net::redact_message(&error)
                )
            })?;
            *reader.needs_network.lock().expect("schema network lock") = false;
            reader.failure.lock().expect("schema error lock").take();
            result = build();
        }
        if let Some(error) = reader.failure.lock().expect("schema error lock").take() {
            result = Err(error);
        }
        associations.push(result.map_err(|error| {
            format!(
                "invalid config: schema '{label}': {}",
                super::net::redact_message(&error)
            )
        })?);
    }
    for (name, bytes) in &loaded.lock().expect("schema reader lock").contents {
        digest.update(&(name.len() as u64).to_le_bytes());
        digest.update(name.as_bytes());
        digest.update(&(bytes.len() as u64).to_le_bytes());
        digest.update(bytes);
    }
    Ok(super::Preparation::Ready(Some(Arc::new(Prepared {
        associations,
        digest: digest.finalize(),
    }))))
}
