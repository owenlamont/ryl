use std::collections::HashSet;
use std::fmt::Write as _;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use ryl::config::{SourceKind, YamlLintConfig};
use ryl::rules::ALL_RULE_IDS;
use ryl::{LintProblem, Severity};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

pub(crate) struct Settings {
    pub disabled: bool,
    pub directory: Option<PathBuf>,
    checked: Mutex<HashSet<PathBuf>>,
}

impl Settings {
    pub fn new(disabled: bool, directory: Option<PathBuf>) -> Self {
        let env_disabled = std::env::var_os("RYL_NO_CACHE")
            .is_some_and(|value| !value.is_empty() && value != "0");
        Self {
            disabled: disabled || env_disabled,
            directory: directory
                .or_else(|| std::env::var_os("RYL_CACHE_DIR").map(PathBuf::from)),
            checked: Mutex::new(HashSet::new()),
        }
    }

    pub fn entry(
        &self,
        path: &Path,
        base_dir: &Path,
        cfg: &YamlLintConfig,
        kind: SourceKind,
        mode: &str,
    ) -> Result<Option<PathBuf>, String> {
        if self.disabled {
            return Ok(None);
        }
        let root = self.directory.as_deref().or_else(|| cfg.cache_dir());
        let relative_to = if self.directory.is_some() {
            std::env::current_dir().expect("cwd exists")
        } else {
            base_dir.to_path_buf()
        };
        let root = root.map_or_else(
            || {
                let project =
                    cfg.source().and_then(Path::parent).is_some_and(|parent| {
                        parent == base_dir || parent == base_dir.join(".config")
                    });
                let location = if project {
                    base_dir.to_path_buf()
                } else {
                    std::env::current_dir().expect("cwd exists")
                };
                location.join(".ryl_cache")
            },
            |root| {
                if root.is_absolute() {
                    root.to_path_buf()
                } else {
                    relative_to.join(root)
                }
            },
        );
        let dir = root.join(env!("CARGO_PKG_VERSION"));
        let mut checked = self.checked.lock().expect("cache lock not poisoned");
        if checked.insert(dir.clone()) {
            fs::create_dir_all(&dir).map_err(|err| cache_error(&dir, &err))?;
            ensure_support_file(&root.join(".gitignore"), b"*\n")?;
            ensure_support_file(
                &root.join("CACHEDIR.TAG"),
                b"Signature: 8a477f597d28d172789f06886806bc55\n",
            )?;
            let probe = dir.join(format!(".probe-{}", std::process::id()));
            fs::write(&probe, b"").map_err(|err| cache_error(&dir, &err))?;
            let _ = fs::remove_file(&probe);
        }
        drop(checked);
        let Ok(content) = fs::read(path) else {
            return Ok(None);
        };
        if mode == "format"
            && (path.as_os_str().to_str().is_none()
                || path.to_string_lossy().contains(char::is_control)
                || content.starts_with(&[0xef, 0xbb, 0xbf])
                || content.starts_with(&[0xff, 0xfe])
                || content.starts_with(&[0xfe, 0xff])
                || content.starts_with(&[0, 0, 0xfe, 0xff])
                || std::str::from_utf8(&content).is_err())
        {
            return Ok(None);
        }
        let mut hash = Sha256::new();
        hash.update(env!("CARGO_PKG_VERSION"));
        hash.update([0]);
        hash.update(mode);
        hash.update([0]);
        hash.update(format!("{kind:?}:{cfg:?}"));
        hash.update([0]);
        hash.update(base_dir.as_os_str().as_encoded_bytes());
        hash.update([0]);
        hash.update(path.as_os_str().as_encoded_bytes());
        hash.update([0]);
        hash.update(content);
        let mut name = String::with_capacity(69);
        for byte in hash.finalize() {
            write!(name, "{byte:02x}").expect("writing to a String is infallible");
        }
        name.push_str(".json");
        Ok(Some(dir.join(name)))
    }
}

fn cache_error(path: &Path, err: &std::io::Error) -> String {
    format!("error: cache {}: {err}", path.display())
}

fn ensure_support_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    match create_new(path) {
        Ok(mut file) => file.write_all(bytes),
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(err) => Err(err),
    }
    .map_err(|err| cache_error(path, &err))
}

fn create_new(path: &Path) -> std::io::Result<fs::File> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}

pub fn clean_hit(path: &Path) -> bool {
    fs::read(path).is_ok_and(|bytes| bytes == b"clean\n")
}

pub fn mark_clean(path: &Path) -> Result<(), String> {
    atomic_write(path, b"clean\n")
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let temp = TempPath(path.with_extension(format!(
        "{}.{}.tmp",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    )));
    create_new(&temp.0)
        .and_then(|mut file| file.write_all(bytes))
        .and_then(|()| fs::rename(&temp.0, path))
        .map_err(|err| cache_error(path, &err))
}

struct TempPath(PathBuf);

impl Drop for TempPath {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[derive(Serialize, Deserialize)]
struct CachedProblem {
    line: usize,
    column: usize,
    level: String,
    message: String,
    rule: Option<String>,
}

pub fn lint_hit(path: &Path) -> Option<Vec<LintProblem>> {
    let bytes = fs::read(path).ok()?;
    let cached: Vec<CachedProblem> = serde_json::from_slice(&bytes).ok()?;
    cached
        .into_iter()
        .map(|problem| {
            let level = match problem.level.as_str() {
                "error" => Severity::Error,
                "warning" => Severity::Warning,
                _ => return None,
            };
            let rule = match problem.rule {
                Some(name) => Some(*ALL_RULE_IDS.iter().find(|id| **id == name)?),
                None => None,
            };
            Some(LintProblem {
                line: problem.line,
                column: problem.column,
                level,
                message: problem.message,
                rule,
            })
        })
        .collect()
}

pub fn store_lint(path: &Path, problems: &[LintProblem]) -> Result<(), String> {
    let cached: Vec<_> = problems
        .iter()
        .map(|problem| CachedProblem {
            line: problem.line,
            column: problem.column,
            level: problem.level.as_str().to_string(),
            message: problem.message.clone(),
            rule: problem.rule.map(str::to_string),
        })
        .collect();
    let bytes = serde_json::to_vec(&cached).expect("cache entries serialize");
    atomic_write(path, &bytes)
}
