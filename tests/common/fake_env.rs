use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use ryl::config::Env;

#[derive(Clone, Default)]
pub struct FakeEnv {
    cwd: PathBuf,
    files: HashMap<PathBuf, String>,
    exists: HashSet<PathBuf>,
    vars: HashMap<String, String>,
    config_dir: Option<PathBuf>,
    home: Option<PathBuf>,
    case_insensitive_paths: Option<bool>,
}

#[allow(dead_code)]
impl FakeEnv {
    pub fn new() -> Self {
        Self {
            cwd: PathBuf::from("."),
            ..Self::default()
        }
    }

    pub fn with_cwd(mut self, path: impl Into<PathBuf>) -> Self {
        self.cwd = path.into();
        self
    }

    pub fn with_config_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.config_dir = Some(path.into());
        self
    }

    pub fn with_file(
        mut self,
        path: impl Into<PathBuf>,
        content: impl Into<String>,
    ) -> Self {
        self.files.insert(path.into(), content.into());
        self
    }

    pub fn with_exists(mut self, path: impl Into<PathBuf>) -> Self {
        self.exists.insert(path.into());
        self
    }

    pub fn with_var(
        mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        self.vars.insert(key.into(), value.into());
        self
    }

    pub fn with_home(mut self, path: impl Into<PathBuf>) -> Self {
        self.home = Some(path.into());
        self
    }

    pub fn with_case_insensitive_paths(mut self, enabled: bool) -> Self {
        self.case_insensitive_paths = Some(enabled);
        self
    }

    fn paths_equal(&self, left: &Path, right: &Path) -> bool {
        if self.case_insensitive_paths() {
            left.components().count() == right.components().count()
                && left
                    .components()
                    .zip(right.components())
                    .all(|(left, right)| {
                        left.as_os_str()
                            .as_encoded_bytes()
                            .eq_ignore_ascii_case(right.as_os_str().as_encoded_bytes())
                    })
        } else {
            left == right
        }
    }
}

impl Env for FakeEnv {
    fn case_insensitive_paths(&self) -> bool {
        self.case_insensitive_paths
            .unwrap_or(cfg!(any(windows, target_os = "macos")))
    }
    fn current_dir(&self) -> PathBuf {
        self.cwd.clone()
    }

    fn config_dir(&self) -> Option<PathBuf> {
        self.config_dir.clone()
    }

    fn read_to_string(&self, p: &Path) -> Result<String, String> {
        self.files
            .iter()
            .find(|(path, _)| self.paths_equal(path, p))
            .map(|(_, data)| data.clone())
            .ok_or_else(|| {
                format!("failed to read config file {}: not found", p.display())
            })
    }

    fn path_exists(&self, p: &Path) -> bool {
        self.files
            .keys()
            .chain(&self.exists)
            .any(|path| self.paths_equal(path, p))
    }

    fn env_var(&self, key: &str) -> Option<String> {
        self.vars.get(key).cloned()
    }

    fn home_dir(&self) -> Option<PathBuf> {
        self.home
            .clone()
            .or_else(|| self.vars.get("HOME").map(PathBuf::from))
            .or_else(|| self.vars.get("USERPROFILE").map(PathBuf::from))
    }
}
