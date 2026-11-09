use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU8, Ordering},
};

use ryl::rules::schema::net::cache_io::{Entries, FileSystem, SystemFileSystem};
use ryl::rules::schema::net::{Dependencies, Error, Transport};
use url::Url;

use super::{Harness, Memory, ROOT};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fault {
    Read,
    BeforeCreate,
    Create,
    AfterCreate,
    List,
    Entry,
    Metadata,
    Open,
    Header,
    Newline,
    Body,
    Rename,
    Evict,
    Target,
}

struct FaultFs {
    fault: Fault,
    phase: Arc<AtomicU8>,
    target: Mutex<Option<PathBuf>>,
}

fn denied() -> io::Error {
    io::Error::from(io::ErrorKind::PermissionDenied)
}

struct BadReader;
impl Read for BadReader {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Err(denied())
    }
}

struct FaultWriter {
    inner: Box<dyn Write + Send>,
    calls: usize,
    fail_at: usize,
}
impl Write for FaultWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.inner.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.calls += 1;
        if self.calls == self.fail_at {
            Err(denied())
        } else {
            self.inner.write_all(bytes)
        }
    }
}

impl FileSystem for FaultFs {
    fn metadata(&self, path: &Path) -> io::Result<std::fs::Metadata> {
        let phase = self.phase.load(Ordering::Relaxed);
        if phase > 0
            && (self.fault == Fault::BeforeCreate
                || self.fault == Fault::AfterCreate && phase >= 2
                || self.fault == Fault::Target
                    && phase >= 3
                    && self.target.lock().unwrap().as_deref() == Some(path))
        {
            return Err(denied());
        }
        SystemFileSystem.metadata(path)
    }
    fn open(&self, path: &Path) -> io::Result<Box<dyn Read + Send>> {
        *self.target.lock().unwrap() = Some(path.to_owned());
        if self.fault == Fault::Read {
            Ok(Box::new(BadReader))
        } else {
            SystemFileSystem.open(path)
        }
    }
    fn create_dirs(&self, path: &Path) -> io::Result<()> {
        if self.fault == Fault::Create {
            return Err(denied());
        }
        SystemFileSystem.create_dirs(path)?;
        self.phase.store(2, Ordering::Relaxed);
        Ok(())
    }
    fn entries(&self, path: &Path) -> io::Result<Entries> {
        self.phase.store(3, Ordering::Relaxed);
        match self.fault {
            Fault::List => Err(denied()),
            Fault::Entry => Ok(Box::new(std::iter::once(Err(denied())))),
            Fault::Metadata => {
                Ok(Box::new(std::iter::once(Ok(path.join("disappeared")))))
            }
            _ => SystemFileSystem.entries(path),
        }
    }
    fn create_new(&self, path: &Path) -> io::Result<Box<dyn Write + Send>> {
        if self.fault == Fault::Open {
            return Err(denied());
        }
        let inner = SystemFileSystem.create_new(path)?;
        let fail_at = match self.fault {
            Fault::Header => 1,
            Fault::Newline => 2,
            Fault::Body => 3,
            _ => return Ok(inner),
        };
        Ok(Box::new(FaultWriter {
            inner,
            calls: 0,
            fail_at,
        }))
    }
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        if self.fault == Fault::Rename {
            Err(denied())
        } else {
            SystemFileSystem.rename(from, to)
        }
    }
    fn remove(&self, path: &Path) -> io::Result<()> {
        if self.fault == Fault::Evict {
            Err(denied())
        } else {
            SystemFileSystem.remove(path)
        }
    }
}

struct Activate {
    inner: Arc<Memory>,
    phase: Arc<AtomicU8>,
}
impl Transport for Activate {
    fn get(
        &self,
        url: &Url,
        timeout: std::time::Duration,
        limit: usize,
    ) -> Result<Vec<u8>, Error> {
        let result = self.inner.get(url, timeout, limit);
        self.phase.store(1, Ordering::Relaxed);
        result
    }
}

fn harness(fault: Fault) -> Harness {
    let mut h = Harness::new();
    h.resource(ROOT, serde_json::json!(true));
    let phase = Arc::new(AtomicU8::new(0));
    let dependencies = Dependencies {
        transport: Arc::new(Activate {
            inner: h.transport.clone(),
            phase: phase.clone(),
        }),
        clock: h.clock.clone(),
        budget: Arc::default(),
        filesystem: Arc::new(FaultFs {
            fault,
            phase,
            target: Mutex::new(None),
        }),
    };
    h.env = h.env.clone().with_schema_dependencies(dependencies);
    h
}

#[test]
fn cache_io_failures_keep_causes_and_clean_temporary_files() {
    for fault in [
        Fault::BeforeCreate,
        Fault::Create,
        Fault::AfterCreate,
        Fault::List,
        Fault::Entry,
        Fault::Metadata,
        Fault::Open,
        Fault::Header,
        Fault::Newline,
        Fault::Body,
        Fault::Rename,
        Fault::Evict,
        Fault::Target,
    ] {
        let h = harness(fault);
        let cache = h.dir.path().join("cache");
        if fault == Fault::Evict {
            std::fs::create_dir(&cache).unwrap();
            std::fs::File::create(cache.join("old"))
                .unwrap()
                .set_len(256 * 1024 * 1024)
                .unwrap();
        }
        let error = h.network().fetch(ROOT, 100).unwrap_err();
        assert!(error.to_string().contains("cache"), "{fault:?}: {error}");
        assert!(std::error::Error::source(&error).is_some(), "{fault:?}");
        if let Ok(mut entries) = std::fs::read_dir(cache) {
            assert!(
                entries.all(|entry| !entry
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|ext| ext == "tmp")),
                "{fault:?}"
            );
        }
    }
}

#[test]
fn unreadable_cache_body_is_a_miss_and_subdirectories_are_ignored() {
    let h = harness(Fault::Read);
    let cache = h.dir.path().join("cache");
    std::fs::create_dir_all(cache.join("unrelated-directory")).unwrap();
    assert_eq!(h.network().fetch(ROOT, 100).unwrap(), b"true");
    assert_eq!(h.transport.calls.lock().unwrap().len(), 1);
}

#[test]
fn no_cache_downloads_without_creating_cache_entries() {
    let mut h = Harness::new();
    h.resource(ROOT, serde_json::json!(true));
    h.env = h.env.clone().with_var("RYL_NO_CACHE", "1");
    assert_eq!(h.network().fetch(ROOT, 100).unwrap(), b"true");
    assert!(!h.dir.path().join("cache").exists());
}

#[test]
fn invalid_cache_metadata_and_content_are_misses() {
    use sha2::{Digest, Sha256};
    for fault in ["metadata", "url", "hash", "oversized"] {
        let h = Harness::new();
        h.resource(ROOT, serde_json::json!(true));
        h.network().fetch(ROOT, 100).unwrap();
        let path = std::fs::read_dir(h.dir.path().join("cache"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let data = std::fs::read(&path).unwrap();
        let split = data.iter().position(|byte| *byte == b'\n').unwrap();
        let mut metadata: serde_json::Value =
            serde_json::from_slice(&data[..split]).unwrap();
        let mut bytes = data[split + 1..].to_vec();
        match fault {
            "metadata" => std::fs::write(&path, b"invalid\ntrue").unwrap(),
            other => {
                match other {
                    "url" => {
                        metadata["url"] =
                            serde_json::json!("https://other.example.org/root")
                    }
                    "hash" => metadata["sha256"] = serde_json::json!("0".repeat(64)),
                    "oversized" => {
                        bytes = vec![b' '; 16 * 1024 * 1024 + 1];
                        let hex: String = Sha256::digest(&bytes)
                            .iter()
                            .map(|byte| format!("{byte:02x}"))
                            .collect();
                        metadata["sha256"] = serde_json::json!(hex);
                    }
                    _ => unreachable!(),
                }
                let mut stored = format!("{metadata}\n").into_bytes();
                stored.extend(bytes);
                std::fs::write(&path, stored).unwrap();
            }
        }
        assert_eq!(h.network().fetch(ROOT, 100).unwrap(), b"true", "{fault}");
        assert_eq!(h.transport.calls.lock().unwrap().len(), 2, "{fault}");
    }
}

#[test]
fn user_allowlist_write_failures_preserve_existing_policy() {
    for fault in [
        Fault::BeforeCreate,
        Fault::Read,
        Fault::Create,
        Fault::Open,
        Fault::Header,
        Fault::Rename,
    ] {
        let h = Harness::new();
        let env = h
            .env
            .clone()
            .with_config_dir(h.dir.path().join("user"))
            .with_schema_dependencies(Dependencies {
                transport: h.transport.clone(),
                clock: h.clock.clone(),
                budget: Arc::default(),
                filesystem: Arc::new(FaultFs {
                    fault,
                    phase: Arc::new(AtomicU8::new(1)),
                    target: Mutex::new(None),
                }),
            });
        assert!(
            ryl::rules::schema::net::persist_origin(
                &env,
                "https://schemas.example.org"
            )
            .is_err(),
            "{fault:?}"
        );
        assert!(
            !h.dir
                .path()
                .join("user/ryl/allowed-schema-origins")
                .exists()
        );
        let parent = h.dir.path().join("user/ryl");
        if parent.exists() {
            assert_eq!(
                std::fs::read_dir(parent).unwrap().count(),
                0,
                "temporary files cleaned for {fault:?}"
            );
        }
    }
}

#[test]
fn user_allowlist_appends_canonical_origins_once_and_preserves_line_boundaries() {
    use crate::common::fake_env::FakeEnv;
    use ryl::rules::schema::net::persist_origin;
    assert!(
        persist_origin(&FakeEnv::new(), "https://schemas.example.org")
            .unwrap_err()
            .contains("directory unavailable")
    );
    for initial in [
        "",
        "https://first.example.org",
        "https://first.example.org\n",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let parent = dir.path().join("ryl");
        std::fs::create_dir(&parent).unwrap();
        let path = parent.join("allowed-schema-origins");
        std::fs::write(&path, initial).unwrap();
        let env = FakeEnv::new().with_config_dir(dir.path());
        persist_origin(&env, "https://schemas.example.org").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text.lines().last(), Some("https://schemas.example.org"));
        assert_eq!(text.lines().count(), usize::from(!initial.is_empty()) + 1);
        persist_origin(&env, "https://schemas.example.org").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        for invalid in [
            "http://schemas.example.org",
            "https://schemas.example.org/",
            "https://schemas.example.org/path",
        ] {
            assert!(persist_origin(&env, invalid).is_err());
        }
    }
}
