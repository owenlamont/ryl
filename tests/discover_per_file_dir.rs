use ryl::config::discover_per_file;
use tempfile::tempdir;

#[test]
fn discover_per_file_handles_directory_input() {
    let td = tempdir().unwrap();
    // No project or user config: resolution falls back to an empty config (the
    // explicit-enable model), not the built-in default preset.
    let ctx = discover_per_file(td.path()).expect("discover for dir");
    assert!(
        !ctx.config_found,
        "an empty temp dir has no config to discover"
    );
    assert!(
        ctx.config.rule_names().is_empty(),
        "the no-config fallback must enable no rules",
    );
}
