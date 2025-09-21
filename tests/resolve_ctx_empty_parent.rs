use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ryl::cli_support::resolve_ctx;
use ryl::config::YamlLintConfig;

#[test]
fn resolve_ctx_handles_path_without_parent() {
    let mut cache: HashMap<PathBuf, (PathBuf, YamlLintConfig)> = HashMap::new();
    let (base_dir, cfg) = resolve_ctx(Path::new(""), None, &mut cache)
        .expect("resolve_ctx should fall back to current directory");
    assert_eq!(base_dir, PathBuf::from("."));
    assert!(cache.contains_key(&PathBuf::from(".")));
    assert!(cfg.rule_names().iter().any(|r| r == "anchors"));
}
