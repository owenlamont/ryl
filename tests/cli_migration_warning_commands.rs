mod common;

use std::fs;
use std::path::Path;

use common::cli::{run, ryl};
use tempfile::tempdir;

fn run_printed_command(home: &Path, cwd: &Path, warning: &str) {
    let command = warning
        .split_once("run `ryl ")
        .expect("warning names a migration command")
        .1
        .split_once('`')
        .expect("command ends with a backtick")
        .0;
    assert!(
        warning.contains(&format!("with ryl {} or later", env!("CARGO_PKG_VERSION"))),
        "{warning}"
    );
    let (code, stdout, stderr) =
        run(ryl(home).current_dir(cwd).args(command.split_whitespace()));
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
}

#[test]
fn project_yaml_warning_command_finishes_migration() {
    let td = tempdir().unwrap();
    let root = td.path();
    fs::write(root.join(".yamllint"), "rules: {anchors: enable}\n").unwrap();
    fs::write(root.join("a.yaml"), "a: 1\n").unwrap();
    let (code, _, warning) = run(ryl(root).current_dir(root).args(["check", "a.yaml"]));
    assert_eq!(code, 0, "{warning}");
    run_printed_command(root, root, &warning);
    assert!(!root.join(".yamllint").exists());
    let (code, _, stderr) = run(ryl(root).current_dir(root).args(["check", "a.yaml"]));
    assert_eq!(code, 0, "{stderr}");
    assert!(stderr.is_empty(), "{stderr}");
}

#[test]
fn explicit_yaml_warning_command_writes_usable_toml() {
    let td = tempdir().unwrap();
    let root = td.path();
    fs::write(root.join("lint.yaml"), "rules: {anchors: enable}\n").unwrap();
    fs::write(root.join("a.yaml"), "a: 1\n").unwrap();
    let (code, _, warning) =
        run(ryl(root)
            .current_dir(root)
            .args(["check", "-c", "lint.yaml", "a.yaml"]));
    assert_eq!(code, 0, "{warning}");
    run_printed_command(root, root, &warning);
    assert!(!root.join("lint.yaml").exists());
    let (code, _, stderr) =
        run(ryl(root)
            .current_dir(root)
            .args(["check", "-c", "lint.toml", "a.yaml"]));
    assert_eq!(code, 0, "{stderr}");
    assert!(stderr.is_empty(), "{stderr}");
}

#[test]
fn deprecated_toml_warning_command_rewrites_yaml_files() {
    let td = tempdir().unwrap();
    let root = td.path();
    fs::write(
        root.join(".ryl.toml"),
        "yaml-files = ['*.yaml']\n[rules]\nanchors = 'enable'\n",
    )
    .unwrap();
    fs::write(root.join("a.yaml"), "a: 1\n").unwrap();
    let (code, _, warning) = run(ryl(root).current_dir(root).args(["check", "a.yaml"]));
    assert_eq!(code, 2, "{warning}");
    assert!(warning.contains("yaml-files"), "{warning}");
    let (code, _, stderr) = run(ryl(root).current_dir(root).args([
        "--migrate-configs",
        "--migrate-write",
        "--migrate-root",
        ".ryl.toml",
    ]));
    assert_eq!(code, 0, "{stderr}");
    let migrated = fs::read_to_string(root.join(".ryl.toml")).unwrap();
    assert!(migrated.contains("yaml = [\"*.yaml\"]"), "{migrated}");
    let (code, _, stderr) = run(ryl(root).current_dir(root).args(["check", "a.yaml"]));
    assert_eq!(code, 0, "{stderr}");
    assert!(stderr.is_empty(), "{stderr}");
}

#[test]
fn existing_files_yaml_wins_over_legacy_yaml_files() {
    let td = tempdir().unwrap();
    let root = td.path();
    fs::write(
        root.join(".ryl.toml"),
        "yaml-files = ['*.yml']\n[files]\nyaml = ['*.yaml']\n[lint.rules]\nanchors = 'enable'\n",
    )
    .unwrap();
    let (code, _, stderr) = run(ryl(root).current_dir(root).args([
        "--migrate-configs",
        "--migrate-write",
        "--migrate-root",
        ".ryl.toml",
    ]));
    assert_eq!(code, 0, "{stderr}");
    let migrated = fs::read_to_string(root.join(".ryl.toml")).unwrap();
    assert!(migrated.contains("yaml = [\"*.yaml\"]"), "{migrated}");
    assert!(!migrated.contains("yaml-files"), "{migrated}");
}

#[test]
fn deprecated_toml_key_warning_command_finishes_migration() {
    let td = tempdir().unwrap();
    let root = td.path();
    fs::write(root.join(".ryl.toml"), "[rules]\nanchors = 'enable'\n").unwrap();
    fs::write(root.join("a.yaml"), "a: 1\n").unwrap();
    let (code, _, warning) = run(ryl(root).current_dir(root).args(["check", "a.yaml"]));
    assert_eq!(code, 0, "{warning}");
    run_printed_command(root, root, &warning);
    let (code, _, stderr) = run(ryl(root).current_dir(root).args(["check", "a.yaml"]));
    assert_eq!(code, 0, "{stderr}");
    assert!(stderr.is_empty(), "{stderr}");
}

#[test]
fn env_yaml_warning_command_writes_usable_toml() {
    let td = tempdir().unwrap();
    let root = td.path();
    fs::write(root.join("lint.yaml"), "rules: {anchors: enable}\n").unwrap();
    fs::write(root.join("a.yaml"), "a: 1\n").unwrap();
    let (code, _, warning) = run(ryl(root)
        .current_dir(root)
        .env("YAMLLINT_CONFIG_FILE", root.join("lint.yaml"))
        .args(["check", "a.yaml"]));
    assert_eq!(code, 0, "{warning}");
    run_printed_command(root, root, &warning);
    assert!(!root.join("lint.yaml").exists());
    let (code, _, stderr) =
        run(ryl(root)
            .current_dir(root)
            .args(["check", "-c", "lint.toml", "a.yaml"]));
    assert_eq!(code, 0, "{stderr}");
    assert!(stderr.is_empty(), "{stderr}");
}

#[test]
fn user_global_yaml_warning_command_finishes_migration() {
    let td = tempdir().unwrap();
    let root = td.path();
    let config_dir = root.join("config");
    fs::create_dir_all(config_dir.join("yamllint")).unwrap();
    fs::write(
        config_dir.join("yamllint/config"),
        "rules: {anchors: enable}\n",
    )
    .unwrap();
    fs::write(root.join("a.yaml"), "a: 1\n").unwrap();
    let check = || {
        run(ryl(root)
            .current_dir(root)
            .env_remove("YAMLLINT_CONFIG_FILE")
            .env("XDG_CONFIG_HOME", &config_dir)
            .args(["check", "a.yaml"]))
    };
    let (code, _, warning) = check();
    assert_eq!(code, 0, "{warning}");
    let command = warning
        .split_once("run `ryl ")
        .expect("migration command")
        .1
        .split_once('`')
        .expect("command terminator")
        .0;
    assert!(
        warning.contains(&format!("with ryl {} or later", env!("CARGO_PKG_VERSION")))
    );
    let (code, _, stderr) = run(ryl(root)
        .current_dir(root)
        .env("XDG_CONFIG_HOME", &config_dir)
        .args(command.split_whitespace()));
    assert_eq!(code, 0, "{stderr}");
    let (code, _, stderr) = check();
    assert_eq!(code, 0, "{stderr}");
    assert!(stderr.is_empty(), "{stderr}");
}
