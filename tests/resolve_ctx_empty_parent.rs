use std::path::Path;

use ryl::cli_support::{CliConfigFlags, ConfigCache, resolve_ctx};

#[test]
fn resolve_ctx_handles_path_without_parent() {
    let mut cache = ConfigCache::default();
    let (context, _notices) =
        resolve_ctx(Path::new(""), None, &CliConfigFlags::default(), &mut cache)
            .expect("resolve_ctx should resolve from the current directory");
    assert_eq!(context.base_dir, std::env::current_dir().unwrap());
    assert!(
        context.found,
        "the repo's own .ryl.toml is discovered for the current directory",
    );
    assert!(context.config.rule_names().iter().any(|r| r == "anchors"));
}
