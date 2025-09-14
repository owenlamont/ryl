use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

struct FakeEnv {
    exists: HashSet<PathBuf>,
    files: HashMap<PathBuf, String>,
}

impl FakeEnv {
    fn new() -> Self {
        Self {
            exists: HashSet::new(),
            files: HashMap::new(),
        }
    }
}

impl ryl::config::Env for FakeEnv {
    fn current_dir(&self) -> PathBuf {
        PathBuf::from(".")
    }
    fn config_dir(&self) -> Option<PathBuf> {
        None
    }
    fn read_to_string(&self, p: &Path) -> Result<String, String> {
        self.files
            .get(p)
            .cloned()
            .ok_or_else(|| format!("missing {}", p.display()))
    }
    fn path_exists(&self, p: &Path) -> bool {
        self.exists.contains(p)
    }
    fn env_var(&self, _key: &str) -> Option<String> {
        None
    }
}

#[test]
fn explicit_config_file_branch_is_covered() {
    let mut envx = FakeEnv::new();
    let file = PathBuf::from("cfg.yml");
    envx.exists.insert(file.clone());
    envx.files.insert(file.clone(), "rules: {}\n".into());

    let overrides = ryl::config::Overrides {
        config_file: Some(file),
        config_data: None,
    };
    let ctx = ryl::config::discover_config_with(&[], &overrides, &envx).expect("ok");
    assert!(ctx.source.is_some());
}
