use ryl::config::discover_per_file;
use tempfile::tempdir;

#[test]
fn discover_per_file_handles_directory_input() {
    let td = tempdir().unwrap();
    // No project or user config; falls back to built-in default.
    let ctx = discover_per_file(td.path()).expect("discover default for dir");
    assert!(ctx.config.rule_names().iter().any(|r| r == "anchors"));
}
