use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use jsonschema::{Retrieve, Uri};
use serde_json::Value;
use url::Url;

use super::{Association, ID, MAX_SCHEMA_BYTES, MAX_SCHEMA_RESOURCES, Prepared};
use crate::config::{Env, PathGlob, YamlLintConfig};
use crate::yaml_dom::YamlOwned;

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

impl Files {
    fn digest(&self) -> blake3::Hash {
        let mut digest = blake3::Hasher::new();
        for (name, bytes) in &self.contents {
            digest.update(&(name.len() as u64).to_le_bytes());
            digest.update(name.as_bytes());
            digest.update(&(bytes.len() as u64).to_le_bytes());
            digest.update(bytes);
        }
        digest.finalize()
    }
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
    fn track(&self, path: &Path) -> Result<(), ReadError> {
        let mut ancestor = path;
        while let Err(source) = std::fs::symlink_metadata(ancestor) {
            if source.kind() != std::io::ErrorKind::NotFound {
                return Err(ReadError::Io {
                    path: ancestor.to_owned(),
                    source,
                });
            }
            ancestor = ancestor
                .parent()
                .expect("absolute candidates reach an existing filesystem root");
        }
        let canonical = ancestor.canonicalize().map_err(|source| ReadError::Io {
            path: ancestor.to_owned(),
            source,
        })?;
        if !canonical.starts_with(&self.base) {
            return Err(ReadError::Escape);
        }
        let normalized = crate::cli_support::lexical_abspath(path);
        if !normalized.starts_with(&self.base) {
            return Err(ReadError::Escape);
        }
        self.dependencies
            .as_ref()
            .expect("only dependency observers track missing paths")
            .lock()
            .expect("schema dependencies lock")
            .insert(normalized);
        Ok(())
    }

    fn read(&self, path: &Path) -> Result<Value, ReadError> {
        let candidate = path;
        let path = match path.canonicalize() {
            Ok(path) => path,
            Err(source) => {
                if self.dependencies.is_some() {
                    self.track(candidate)?;
                }
                return Err(ReadError::Io {
                    path: candidate.to_owned(),
                    source,
                });
            }
        };
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
            let mut dependencies =
                dependencies.lock().expect("schema dependencies lock");
            let candidate = crate::cli_support::lexical_abspath(candidate);
            if candidate.starts_with(&self.base) {
                dependencies.insert(candidate);
            }
            dependencies.insert(path.clone());
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
        self.read(&path).map_err(Into::into)
    }
}

impl LocalRetriever {
    fn compile(
        &self,
        path: &str,
        remote: bool,
        files: Vec<PathGlob>,
        resolution_base: &Path,
        fragment: Option<&str>,
    ) -> Result<Association, String> {
        let (schema, uri) = if remote {
            let schema = self.remote(path).map_err(|error| error.to_string())?;
            let mut url = Url::parse(path).expect("retrieved schema URL is valid");
            url.set_fragment(None);
            let uri = jsonschema::uri::from_str(url.as_str())
                .map_err(|_| "invalid schema URI".to_owned())?;
            (schema, uri)
        } else {
            let (root, uri) = root_location(resolution_base, path)?;
            (self.read(&root).map_err(|error| error.to_string())?, uri)
        };
        let registry = jsonschema::Registry::new()
            .retriever(self.clone())
            .add(uri.as_str(), schema.clone())
            .and_then(jsonschema::RegistryBuilder::prepare)
            .map_err(|error| error.to_string())?;
        super::complexity::check(&registry, &uri)?;
        let (schema, base) = if let Some(fragment) = fragment {
            let target = format!("{uri}#{fragment}");
            super::complexity::check(
                &registry,
                &jsonschema::uri::from_str(&target)
                    .map_err(|error| error.to_string())?,
            )?;
            // An unused wrapper URI retains the subschema's dialect and reference scope.
            let mut base = "urn:ryl:schema-selection:0".to_owned();
            while registry.contains_resource(&base) {
                base.push(':');
            }
            (
                serde_json::json!({"$ref":target}),
                std::borrow::Cow::Owned(base),
            )
        } else {
            (schema, std::borrow::Cow::Borrowed(uri.as_str()))
        };
        let validator = jsonschema::options()
            .with_base_uri(base)
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
        || path.as_bytes().get(1) == Some(&b':')
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
    let uri = Url::parse(uri.as_str()).expect("file URL normalizes dot segments");
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

struct Root<'a> {
    raw: &'a str,
    fragment: Option<&'a str>,
    remote: bool,
    files: Vec<PathGlob>,
    base: PathBuf,
}

impl LocalRetriever {
    fn prepare_root(
        &self,
        root: &Root<'_>,
        cfg: &YamlLintConfig,
        env: &dyn Env,
        progress: &mut Option<super::net::progress::Progress>,
    ) -> Result<Option<Association>, String> {
        if root.remote && !env.remote_schemas() {
            return Ok(None);
        }
        if root.remote && self.network.get().is_none() {
            *progress = enable_network(&self.network, cfg, env)?;
        }
        let path = root.raw;
        let fragment = root.fragment;
        let build = || {
            self.compile(path, root.remote, root.files.clone(), &root.base, fragment)
        };
        let charged = {
            let files = self.files.lock().expect("schema reader lock");
            (files.bytes, files.resources)
        };
        let mut result = build();
        if *self.needs_network.lock().expect("schema network lock") {
            if !env.remote_schemas() {
                return Ok(None);
            }
            let mut files = self.files.lock().expect("schema reader lock");
            (files.bytes, files.resources) = charged;
            drop(files);
            *progress = enable_network(&self.network, cfg, env)?;
            *self.needs_network.lock().expect("schema network lock") = false;
            self.failure.lock().expect("schema error lock").take();
            result = build();
        }
        if let Some(error) = self.failure.lock().expect("schema error lock").take() {
            result = Err(error);
        }
        result.map(Some)
    }
}

fn directive_base(
    raw: &str,
    remote: bool,
    path: Option<&Path>,
    env: &dyn Env,
    base: &Path,
) -> Result<PathBuf, String> {
    if remote {
        if !raw
            .get(..6)
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https:"))
        {
            return Err(
                "schema directive root must be a relative path or HTTPS URL".to_owned()
            );
        }
        return Ok(base.to_owned());
    }
    let path =
        path.ok_or("local schema directive requires a filename; use --stdin-filename")?;
    Ok(env
        .current_dir()
        .join(path)
        .parent()
        .expect("absolute input has a parent")
        .to_owned())
}

pub(crate) fn prepare(
    cfg: &YamlLintConfig,
    base: &Path,
    envx: &dyn Env,
) -> Result<super::Preparation, super::PreparationError> {
    let location = std::cell::Cell::new((1, 1));
    prepare_inner(cfg, base, envx, &location).map_err(|message| {
        super::PreparationError {
            line: location.get().0,
            column: location.get().1,
            message,
        }
    })
}

fn sources(
    cfg: &YamlLintConfig,
) -> impl Iterator<
    Item = (
        &str,
        bool,
        Option<&YamlOwned>,
        Option<&super::directives::Declaration>,
    ),
> {
    let configured = cfg
        .rule_option(ID, "schemas")
        .and_then(YamlOwned::as_sequence)
        .into_iter()
        .flatten()
        .map(|entry| {
            let remote = entry.as_mapping_get("url").and_then(YamlOwned::as_str);
            let path = remote
                .or_else(|| entry.as_mapping_get("path").and_then(YamlOwned::as_str))
                .expect("typed schema location");
            (path, remote.is_some(), Some(entry), None)
        });
    let declared = cfg
        .schema_request()
        .into_iter()
        .flat_map(|request| request.declarations.iter())
        .filter_map(|declaration| {
            declaration
                .value
                .as_deref()
                .filter(|value| !value.eq_ignore_ascii_case("none"))
                .map(|value| {
                    (value, Url::parse(value).is_ok(), None, Some(declaration))
                })
        });
    configured.chain(declared)
}

fn prepare_inner(
    cfg: &YamlLintConfig,
    base: &Path,
    envx: &dyn Env,
    location: &std::cell::Cell<(usize, usize)>,
) -> Result<super::Preparation, String> {
    let mut sources = sources(cfg).peekable();
    let request = cfg.schema_request();
    if sources.peek().is_none() && request.is_none() {
        return Ok(super::Preparation::Ready(None));
    }
    let absolute_base = envx.current_dir().join(base);
    let canonical_base = absolute_base
        .canonicalize()
        .map_err(|error| format!("invalid config: schema base: {error}"))?;
    let dependencies = envx.local_schema_dependencies();
    let mut associations = Vec::new();
    let mut directives = BTreeMap::new();
    let loaded = Arc::new(Mutex::new(Files::default()));
    let network = Arc::new(OnceLock::new());
    let mut progress = None;
    for (raw, remote, entry, declaration) in sources {
        if declaration.is_some() && directives.contains_key(raw) {
            continue;
        }
        location.set(
            declaration
                .map_or((1, 1), |declaration| (declaration.line, declaration.column)),
        );
        let label = remote.then(|| super::net::display_url(raw));
        let resolution_base = if declaration.is_some() {
            directive_base(
                raw,
                remote,
                request.and_then(|request| request.path.as_deref()),
                envx,
                &absolute_base,
            )?
        } else {
            canonical_base.clone()
        };
        let reader = LocalRetriever {
            base: canonical_base.clone(),
            files: Arc::clone(&loaded),
            network: Arc::clone(&network),
            needs_network: Arc::default(),
            failure: Arc::default(),
            allow_files: !remote,
            dependencies: dependencies.clone(),
        };
        let files: Vec<PathGlob> = entry
            .and_then(|entry| entry.as_mapping_get("files"))
            .and_then(YamlOwned::as_sequence)
            .into_iter()
            .flatten()
            .filter_map(YamlOwned::as_str)
            .map(|glob| {
                PathGlob::new(glob, &absolute_base, envx.case_insensitive_paths())
                    .expect("validated schema files glob")
            })
            .collect();
        let (path, fragment) = if remote || declaration.is_some() {
            raw.split_once('#')
                .map_or((raw, None), |(path, fragment)| (path, Some(fragment)))
        } else {
            (raw, None)
        };
        let association = reader
            .prepare_root(
                &Root {
                    raw: path,
                    fragment,
                    remote,
                    files,
                    base: resolution_base,
                },
                cfg,
                envx,
                &mut progress,
            )
            .map_err(|error| {
                format!(
                    "invalid config: schema '{}': {}",
                    label.as_deref().unwrap_or(raw),
                    super::net::redact_message(&error)
                )
            })?;
        let Some(association) = association else {
            return Ok(super::Preparation::NeedsRemotePreparation);
        };
        if declaration.is_some() {
            directives.insert(raw.to_owned(), association);
        } else {
            associations.push(association);
        }
    }
    let usage = loaded.lock().expect("schema reader lock");
    Ok(super::Preparation::Ready(Some(Arc::new(Prepared {
        associations,
        directives,
        digest: usage.digest(),
        bytes: usage.bytes,
        resources: usage.resources,
    }))))
}
