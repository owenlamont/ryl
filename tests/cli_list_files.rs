use std::fs;

use tempfile::tempdir;

use crate::common;
use common::cli::{run, ryl};

#[test]
fn list_files_outputs_expected_entries() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.yaml");
    fs::write(&file, "key: value\n").unwrap();

    // Bound discovery at the tempdir so a stray config in the shared temp root cannot
    // change which files `--list-files` selects.
    let (code, stdout, stderr) = run(ryl(dir.path())
        .arg("check")
        .arg("--list-files")
        .arg(dir.path()));
    assert_eq!(code, 0, "list-files should succeed: stderr={stderr}");
    assert!(stderr.trim().is_empty(), "unexpected stderr: {stderr}");
    assert!(
        stdout.contains("sample.yaml"),
        "expected stdout to include listed file: {stdout}"
    );
}

#[test]
fn list_files_applies_configured_ignore_to_explicit_files() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join(".yamllint"),
        "ignore: ['*.skip.yaml']\nrules: {}\n",
    )
    .unwrap();
    let keep = dir.path().join("keep.yaml");
    let skip = dir.path().join("x.skip.yaml");
    fs::write(&keep, "ok: 1\n").unwrap();
    fs::write(&skip, "ok: 1\n").unwrap();

    let (code, stdout, stderr) = run(ryl(dir.path())
        .args(["check", "--list-files"])
        .arg(dir.path())
        .arg(&skip));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let listed: Vec<_> = stdout
        .lines()
        .filter(|line| line.ends_with(".yaml"))
        .collect();
    assert_eq!(listed, [keep.to_str().unwrap()], "{stdout}");
}

#[test]
fn list_files_uses_project_yaml_before_env_yaml() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join(".yamllint"),
        "ignore: ['project.yaml']\nrules: {}\n",
    )
    .unwrap();
    let env_config = dir.path().join("env-config.yml");
    fs::write(&env_config, "ignore: ['env.yaml']\nrules: {}\n").unwrap();
    fs::write(dir.path().join("project.yaml"), "ok: 1\n").unwrap();
    let env_file = dir.path().join("env.yaml");
    fs::write(&env_file, "ok: 1\n").unwrap();
    let keep = dir.path().join("keep.yaml");
    fs::write(&keep, "ok: 1\n").unwrap();

    let (code, stdout, stderr) = run(ryl(dir.path())
        .current_dir(dir.path())
        .env("YAMLLINT_CONFIG_FILE", &env_config)
        .args(["check", "--list-files"])
        .arg(dir.path()));
    assert_eq!(code, 0, "{stdout}{stderr}");
    let mut listed: Vec<_> = stdout
        .lines()
        .filter(|line| line.ends_with(".yaml"))
        .collect();
    listed.sort_unstable();
    let mut expected = [env_file.to_str().unwrap(), keep.to_str().unwrap()];
    expected.sort_unstable();
    assert_eq!(listed, expected, "{stdout}");
}
