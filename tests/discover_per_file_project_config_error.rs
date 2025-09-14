use std::fs;

use ryl::config::discover_per_file;
use tempfile::tempdir;

#[test]
fn discover_per_file_errors_on_invalid_project_config() {
    let td = tempdir().unwrap();
    let root = td.path();
    fs::write(root.join(".yamllint"), "rules: {\n").unwrap();
    let file = root.join("a.yaml");
    fs::write(&file, "a: 1\n").unwrap();

    let res = discover_per_file(&file);
    assert!(
        res.is_err(),
        "expected parse error from invalid project config"
    );
    let err = res.err().unwrap();
    assert!(err.contains("failed to parse config data"));
}
