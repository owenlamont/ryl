use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use ryl::config::{SourceKind, YamlLintConfig};
use ryl::rules::ALL_RULE_IDS;
use ryl::{LintProblem, Severity};
use serde::{Deserialize, Serialize};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

type LintEntries = HashMap<String, Vec<LintProblem>>;

pub(crate) struct Settings {
    pub disabled: bool,
    pub directory: Option<PathBuf>,
    cwd: PathBuf,
    checked: Mutex<HashSet<PathBuf>>,
    lint_entries: Mutex<HashMap<PathBuf, Arc<LintEntries>>>,
    prefixes: Mutex<HashMap<(usize, bool, bool, PathBuf), blake3::Hasher>>,
}

impl Settings {
    pub fn new(disabled: bool, directory: Option<PathBuf>) -> Self {
        let env_disabled = std::env::var_os("RYL_NO_CACHE")
            .is_some_and(|value| !value.is_empty() && value != "0");
        Self {
            disabled: disabled || env_disabled,
            directory: directory
                .or_else(|| std::env::var_os("RYL_CACHE_DIR").map(PathBuf::from)),
            cwd: std::env::current_dir().expect("cwd exists"),
            checked: Mutex::new(HashSet::new()),
            lint_entries: Mutex::new(HashMap::new()),
            prefixes: Mutex::new(HashMap::new()),
        }
    }

    pub fn entry(
        &self,
        path: &Path,
        base_dir: &Path,
        cfg: &YamlLintConfig,
        kind: SourceKind,
        mode: &str,
    ) -> Result<Option<(PathBuf, Vec<u8>)>, String> {
        if self.disabled {
            return Ok(None);
        }
        let root = self.directory.as_deref().or_else(|| cfg.cache_dir());
        let root = root.map_or_else(
            || self.cwd.join(".ryl_cache"),
            |root| {
                if root.is_absolute() {
                    root.to_path_buf()
                } else {
                    self.directory
                        .as_ref()
                        .map_or(base_dir, |_| self.cwd.as_path())
                        .join(root)
                }
            },
        );
        let root: PathBuf = root.components().collect();
        let dir = root.join(env!("CARGO_PKG_VERSION"));
        let mut checked = self.checked.lock().expect("cache lock not poisoned");
        if checked.insert(dir.clone()) {
            for ancestor in root.ancestors().chain(std::iter::once(dir.as_path())) {
                refuse_symlink(ancestor)?;
            }
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
        let key = (
            std::ptr::from_ref::<YamlLintConfig>(cfg) as usize,
            kind == SourceKind::Markdown,
            mode == "format",
            base_dir.to_path_buf(),
        );
        let mut prefixes = self.prefixes.lock().expect("cache lock not poisoned");
        let mut hash = prefixes
            .entry(key)
            .or_insert_with(|| {
                let mut hash = blake3::Hasher::new();
                hash.update(env!("CARGO_PKG_VERSION").as_bytes());
                hash.update(&[0]);
                hash.update(mode.as_bytes());
                hash.update(&[0]);
                hash.update(format!("{kind:?}:{cfg:?}").as_bytes());
                hash.update(&[0]);
                hash.update(base_dir.as_os_str().as_encoded_bytes());
                hash.update(&[0]);
                if let Some(encoding) = std::env::var_os("YAMLLINT_FILE_ENCODING") {
                    hash.update(&[1]);
                    hash.update(encoding.as_encoded_bytes());
                } else {
                    hash.update(&[0]);
                }
                hash
            })
            .clone();
        drop(prefixes);
        hash.update(path.as_os_str().as_encoded_bytes());
        hash.update(&[0]);
        hash.update(&content);
        Ok(Some((dir.join(hash.finalize().to_hex().as_str()), content)))
    }

    pub fn lint_hit(&self, path: &Path) -> Option<Vec<LintProblem>> {
        let dir = path.parent().expect("cache entry has parent");
        let name = path
            .file_name()
            .expect("cache entry has name")
            .to_str()
            .expect("cache entry name is ASCII");
        let entries = {
            let mut loaded = self.lint_entries.lock().expect("cache lock not poisoned");
            Arc::clone(
                loaded
                    .entry(dir.to_path_buf())
                    .or_insert_with(|| Arc::new(load_lint_entries(dir))),
            )
        };
        entries.get(name).cloned()
    }
}

fn load_lint_entries(dir: &Path) -> LintEntries {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|file| file.file_name().to_string_lossy().starts_with("lint-"))
        .filter_map(|file| fs::read(file.path()).ok())
        .filter_map(|bytes| {
            postcard::from_bytes::<HashMap<String, Vec<CachedProblem>>>(&bytes).ok()
        })
        .flat_map(|batch| {
            batch.into_iter().filter_map(|(name, cached)| {
                decode_problems(cached).map(|problems| (name, problems))
            })
        })
        .collect()
}

fn refuse_symlink(path: &Path) -> Result<(), String> {
    if fs::symlink_metadata(path)
        .is_ok_and(|metadata| is_symlink_or_junction(&metadata))
    {
        return Err(format!(
            "error: cache {}: symlinked directory is refused",
            path.display()
        ));
    }
    Ok(())
}

fn is_symlink_or_junction(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;

        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
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

#[derive(Deserialize)]
struct CachedProblem(usize, usize, u8, String, Option<String>, bool);

#[derive(Serialize)]
struct CachedProblemRef<'a>(usize, usize, u8, &'a str, Option<&'a str>, bool);

fn decode_problems(cached: Vec<CachedProblem>) -> Option<Vec<LintProblem>> {
    cached
        .into_iter()
        .map(|problem| {
            let level = match problem.2 {
                0 => Severity::Error,
                1 => Severity::Warning,
                _ => return None,
            };
            let rule = match problem.4 {
                Some(name) => Some(*ALL_RULE_IDS.iter().find(|id| **id == name)?),
                None => None,
            };
            Some(LintProblem {
                line: problem.0,
                column: problem.1,
                level,
                message: problem.3,
                rule,
                hidden_unsafe_fix: problem.5,
            })
        })
        .collect()
}

fn encode_problems(problems: &[LintProblem]) -> Vec<CachedProblemRef<'_>> {
    problems
        .iter()
        .map(|problem| {
            CachedProblemRef(
                problem.line,
                problem.column,
                u8::from(problem.level != Severity::Error),
                &problem.message,
                problem.rule,
                problem.hidden_unsafe_fix,
            )
        })
        .collect()
}

pub fn store_lint_batch<'a>(
    entries: impl Iterator<Item = (&'a Path, &'a [LintProblem])>,
) -> Result<(), String> {
    let mut by_dir: HashMap<&Path, HashMap<&str, Vec<CachedProblemRef<'_>>>> =
        HashMap::new();
    for (path, problems) in entries {
        by_dir
            .entry(path.parent().expect("cache entry has parent"))
            .or_default()
            .insert(
                path.file_name()
                    .expect("cache entry has name")
                    .to_str()
                    .expect("cache entry name is ASCII"),
                encode_problems(problems),
            );
    }
    for (dir, batch) in by_dir {
        let mut names: Vec<_> = batch.keys().collect();
        names.sort();
        let mut hash = blake3::Hasher::new();
        for name in names {
            hash.update(name.as_bytes());
        }
        let path = dir.join(format!("lint-{}.postcard", hash.finalize().to_hex()));
        let bytes = postcard::to_allocvec(&batch).expect("cache entries serialize");
        atomic_write(&path, &bytes)?;
    }
    Ok(())
}
