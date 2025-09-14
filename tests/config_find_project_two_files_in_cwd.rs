use std::collections::HashSet;
use std::path::{Path, PathBuf};

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
    fn read_to_string(&self, p: &Path) -> Result<String, String> {
        if p == Path::new(".yamllint") {
            Ok("rules: {}\n".into())
        } else {
            Err("no".into())
        }
    }
    fn path_exists(&self, p: &Path) -> bool {
        self.exists.contains(p)
    }
    fn env_var(&self, _key: &str) -> Option<String> {
        None
    }
}

#[test]
fn project_config_search_dedups_multiple_files_without_parent() {
    let mut envx = FakeEnv {
        exists: HashSet::new(),
    };
    envx.exists.insert(PathBuf::from(".yamllint"));
    let inputs = vec![PathBuf::from("a.yml"), PathBuf::from("b.yml")];
    let ctx = ryl::config::discover_config_with(&inputs, &ryl::config::Overrides::default(), &envx)
        .expect("ok");
    assert_eq!(ctx.source.as_deref(), Some(Path::new(".yamllint")));
}
