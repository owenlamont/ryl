use std::collections::HashSet;
use std::path::{Path, PathBuf};

struct FakeEnv {
    exists: HashSet<PathBuf>,
    files: std::collections::HashMap<PathBuf, String>,
}

impl FakeEnv {
    fn new() -> Self {
        Self {
            exists: HashSet::new(),
            files: std::collections::HashMap::new(),
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
            .ok_or_else(|| format!("no file: {}", p.display()))
    }
    fn path_exists(&self, p: &Path) -> bool {
        self.exists.contains(p)
    }
    fn env_var(&self, _key: &str) -> Option<String> {
        None
    }
}

#[test]
fn project_config_search_dedups_dir_and_file_inputs() {
    let mut envx = FakeEnv::new();
    let dir = PathBuf::from("proj");
    let file = dir.join("file.yaml");
    let cfg = dir.join(".yamllint");
    envx.exists.insert(cfg.clone());
    envx.files.insert(cfg.clone(), "rules: {}\n".into());

    let inputs = vec![dir.clone(), file];
    let overrides = ryl::config::Overrides::default();
    let got = ryl::config::discover_config_with(&inputs, &overrides, &envx)
        .expect("config discovery should succeed");
    assert_eq!(got.base_dir, dir);
    assert_eq!(got.source.as_deref(), Some(cfg.as_path()));
}
