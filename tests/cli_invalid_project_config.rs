use std::fs;

use tempfile::tempdir;

mod common;
use common::cli::{run, ryl};

#[test]
fn invalid_project_config_in_dir_causes_exit_2() {
    let td = tempdir().unwrap();
    let root = td.path();
    fs::create_dir(root.join(".yamllint")).unwrap();
    fs::write(root.join("a.yaml"), "a: 1\n").unwrap();

    // `ryl(root)` bounds discovery at the tempdir so the invalid `.yamllint` here (not a
    // stray config in the shared temp root) is what gets discovered.
    let (code, _out, err) = run(ryl(root).arg("--list-files").arg(root));
    assert_eq!(code, 2, "expected exit 2: {err}");
    assert!(err.contains("failed to read"));
}

#[test]
fn invalid_project_config_for_explicit_file_causes_exit_2() {
    let td = tempdir().unwrap();
    let root = td.path();
    fs::create_dir(root.join(".yamllint")).unwrap();
    let f = root.join("a.yaml");
    fs::write(&f, "a: 1\n").unwrap();

    let (code, _out, err) = run(ryl(root).arg("--list-files").arg(&f));
    assert_eq!(code, 2, "expected exit 2: {err}");
    assert!(err.contains("failed to read"));
}

#[test]
fn invalid_output_config_with_no_lintable_files_causes_exit_2() {
    // Linting an empty subdirectory finds no files, so per-file discovery (which descends
    // into the input) never reads the project config in the parent. The run-level
    // `[output]` read climbs to it instead, and a malformed `[output]` there must still
    // surface as an error rather than be silently ignored on an otherwise file-less run.
    let td = tempdir().unwrap();
    let root = td.path();
    fs::write(
        root.join("ryl.toml"),
        "[rules]\ncolons = \"enable\"\n[output.gitlab]\npath = \"\"\n",
    )
    .unwrap();
    let empty = root.join("empty");
    fs::create_dir(&empty).unwrap();

    // HOME = root so discovery climbs from `empty` to the parent's `ryl.toml` but no higher.
    let (code, _out, err) = run(ryl(root).arg(&empty));
    assert_eq!(
        code, 2,
        "a malformed [output] is an error even with no files to lint: {err}"
    );
    assert!(
        err.contains("output.gitlab.path must not be empty"),
        "expected the empty-path config error: {err}"
    );
}
