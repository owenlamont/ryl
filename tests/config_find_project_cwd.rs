use std::collections::HashSet;
use std::path::PathBuf;

struct FakeEnv {
    exists: HashSet<PathBuf>,
}

impl ryl::config::Env for FakeEnv {
    fn current_dir(&self) -> PathBuf {
        PathBuf::from(".")
    }
    fn config_dir(&self) -> Option<PathBuf> {
        None
    }
    fn read_to_string(&self, _p: &std::path::Path) -> Result<String, String> {
        Ok(String::from("rules: {}\n"))
    }
    fn path_exists(&self, p: &std::path::Path) -> bool {
        self.exists.contains(p)
    }
    fn env_var(&self, _key: &str) -> Option<String> {
        None
    }
}

#[test]
fn project_config_search_uses_cwd_when_no_inputs() {
    let mut envx = FakeEnv {
        exists: HashSet::new(),
    };
    let cfg = PathBuf::from(".yamllint");
    envx.exists.insert(cfg.clone());
    let ctx = ryl::config::discover_config_with(&[], &ryl::config::Overrides::default(), &envx)
        .expect("ok");
    assert_eq!(ctx.source.as_deref(), Some(cfg.as_path()));
}
