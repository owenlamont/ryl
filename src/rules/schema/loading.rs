use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

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
    #[error("schema resource limit exceeded (1024)")]
    ResourceLimit,
    #[error("schema size limit exceeded (16 MiB including references)")]
    ByteLimit,
}

#[derive(Default)]
struct Files {
    bytes: usize,
    contents: BTreeMap<PathBuf, Vec<u8>>,
    resources: usize,
}

#[derive(Clone)]
struct LocalRetriever {
    base: PathBuf,
    files: Arc<Mutex<Files>>,
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
        if let Some(bytes) = files.contents.get(&path) {
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
        files.contents.insert(path, bytes);
        files.resources += 1;
        Ok(value)
    }
}

impl Retrieve for LocalRetriever {
    fn retrieve(
        &self,
        uri: &Uri<String>,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        let url = Url::parse(uri.as_str())?;
        if url.scheme() != "file" {
            return Err("remote $ref needs network opt-in (not supported yet)".into());
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

pub(crate) fn prepare(
    cfg: &YamlLintConfig,
    base: &Path,
    envx: &dyn Env,
) -> Result<Option<Arc<Prepared>>, String> {
    let Some(entries) = cfg
        .rule_option(ID, "schemas")
        .and_then(YamlOwned::as_sequence)
        .filter(|entries| !entries.is_empty())
    else {
        return Ok(None);
    };
    let absolute_base = envx.current_dir().join(base);
    let canonical_base = absolute_base
        .canonicalize()
        .map_err(|error| format!("invalid config: schema base: {error}"))?;
    let mut digest = blake3::Hasher::new();
    let mut associations = Vec::new();
    for entry in entries {
        let reader = LocalRetriever {
            base: canonical_base.clone(),
            files: Arc::default(),
        };
        let path = entry
            .as_mapping_get("path")
            .and_then(YamlOwned::as_str)
            .expect("typed schema path");
        let build = || -> Result<Association, String> {
            if !Path::new(path).components().all(|part| {
                matches!(
                    part,
                    Component::Normal(_) | Component::CurDir | Component::ParentDir
                )
            }) || path.starts_with('\\')
            {
                return Err(
                    "schema root path must be relative to config base directory"
                        .to_owned(),
                );
            }
            let root = reader.base.join(path);
            let uri = Url::from_file_path(&root)
                .expect("a schema root joined to a canonical base is absolute");
            let schema = reader.read(&root).map_err(|error| error.to_string())?;
            let validator = jsonschema::options()
                .with_base_uri(uri.as_str())
                .with_retriever(reader.clone())
                .with_pattern_options(
                    jsonschema::PatternOptions::fancy_regex()
                        .backtrack_limit(100_000)
                        .size_limit(1_048_576)
                        .dfa_size_limit(1_048_576),
                )
                .build(&schema)
                .map_err(|error| error.to_string())?;
            let files = entry
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
            Ok(Association {
                files,
                validator: Arc::new(validator),
            })
        };
        associations
            .push(build().map_err(|error| {
                format!("invalid config: schema '{path}': {error}")
            })?);
        for (path, bytes) in &reader.files.lock().expect("schema reader lock").contents
        {
            let name = path.to_string_lossy();
            digest.update(&(name.len() as u64).to_le_bytes());
            digest.update(name.as_bytes());
            digest.update(&(bytes.len() as u64).to_le_bytes());
            digest.update(bytes);
        }
    }
    Ok(Some(Arc::new(Prepared {
        associations,
        digest: digest.finalize(),
    })))
}
