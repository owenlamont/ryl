use std::path::PathBuf;

use ryl::discover::is_yaml_path;

#[test]
fn is_yaml_path_matches_common_extensions_case_insensitive() {
    assert!(is_yaml_path(&PathBuf::from("foo.yaml")));
    assert!(is_yaml_path(&PathBuf::from("bar.yml")));
    assert!(is_yaml_path(&PathBuf::from("BAZ.YAML")));
    assert!(!is_yaml_path(&PathBuf::from("nope.txt")));
}
