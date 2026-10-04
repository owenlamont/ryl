use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ryl::cli_support::{ResolvedConfig, resolve_ctx};

#[test]
fn resolve_ctx_handles_path_without_parent() {
    let mut cache: HashMap<PathBuf, ResolvedConfig> = HashMap::new();
    let (base_dir, cfg, notices, config_found) =
        resolve_ctx(Path::new(""), None, false, &mut cache)
            .expect("resolve_ctx should resolve from the current directory");
    assert_eq!(base_dir, std::env::current_dir().unwrap());
    assert!(notices.is_empty());
    assert!(cache.contains_key(&PathBuf::from(".")));
    assert!(
        config_found,
        "the repo's own .ryl.toml is discovered for the current directory",
    );
    assert!(cfg.rule_names().iter().any(|r| r == "anchors"));
}
