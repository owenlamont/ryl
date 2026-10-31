use std::fs;
use std::path::PathBuf;

use ryl::discover::{gather_yaml_from_dir, is_yaml_path};
use tempfile::tempdir;

#[test]
fn is_yaml_path_matches_common_extensions_case_insensitive() {
    assert!(is_yaml_path(&PathBuf::from("foo.yaml")));
    assert!(is_yaml_path(&PathBuf::from("bar.yml")));
    assert!(is_yaml_path(&PathBuf::from("BAZ.YAML")));
    assert!(!is_yaml_path(&PathBuf::from("nope.txt")));
}

#[test]
fn gather_yaml_from_dir_finds_yaml_files_only() {
    let td = tempdir().unwrap();
    let root = td.path();
    fs::write(root.join("a.yaml"), "a: 1\n").unwrap();
    fs::write(root.join("b.yml"), "b: 1\n").unwrap();
    fs::write(root.join("not.yaml.txt"), "noop\n").unwrap();

    let files = gather_yaml_from_dir(root);
    let mut names: Vec<_> = files
        .into_iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, vec!["a.yaml", "b.yml"]);
}

#[test]
fn gather_yaml_from_dir_skips_vcs_directories_but_not_dot_github() {
    let td = tempdir().unwrap();
    let root = td.path();
    for vcs in [".git", ".hg", ".svn", ".jj"] {
        fs::create_dir(root.join(vcs)).unwrap();
        fs::write(root.join(vcs).join("x.yaml"), "a: 1\n").unwrap();
    }
    fs::create_dir(root.join(".github")).unwrap();
    fs::write(root.join(".github/ci.yaml"), "a: 1\n").unwrap();

    assert_eq!(gather_yaml_from_dir(root), [root.join(".github/ci.yaml")]);
}
