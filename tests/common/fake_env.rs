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

    pub fn with_file(mut self, path: impl Into<PathBuf>, content: impl Into<String>) -> Self {
        self.files.insert(path.into(), content.into());
        self
    }

    pub fn with_exists(mut self, path: impl Into<PathBuf>) -> Self {
        self.exists.insert(path.into());
        self
    }

    pub fn with_var(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.vars.insert(key.into(), value.into());
        self
    }
}

impl Env for FakeEnv {
    fn current_dir(&self) -> PathBuf {
        self.cwd.clone()
    }

    fn config_dir(&self) -> Option<PathBuf> {
        self.config_dir.clone()
    }

    fn read_to_string(&self, p: &Path) -> Result<String, String> {
        self.files
            .get(p)
            .cloned()
            .ok_or_else(|| format!("failed to read config file {}: not found", p.display()))
    }

    fn path_exists(&self, p: &Path) -> bool {
        self.files.contains_key(p) || self.exists.contains(p)
    }

    fn env_var(&self, key: &str) -> Option<String> {
        self.vars.get(key).cloned()
    }
}
