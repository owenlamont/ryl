mod common;

use std::fs;
use std::path::Path;
#[cfg(unix)]
use std::process::Command;

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

#[cfg(unix)]
#[test]
fn migration_commands_for_shell_metacharacters_run_verbatim() {
    let td = tempdir().unwrap();
    let home = td.path();
    for directory in ["hash#dir", "dollar$dir", "apostrophe'dir"] {
        let project = home.join(directory);
        fs::create_dir(&project).unwrap();
        fs::write(project.join(".yamllint"), "rules: {anchors: enable}\n").unwrap();
        fs::write(project.join("a.yaml"), "a: 1\n").unwrap();
        let (code, _, warning) =
            run(ryl(home).current_dir(&project).args(["check", "a.yaml"]));
        assert_eq!(code, 0, "{warning}");
        let command = warning
            .split_once("run `ryl ")
            .expect("migration command")
            .1
            .split_once('`')
            .expect("command terminator")
            .0;
        let (code, stdout, stderr) = run(Command::new("sh")
            .arg("-c")
            .arg(format!("\"$RYL_BIN\" {command}"))
            .env("RYL_BIN", env!("CARGO_BIN_EXE_ryl"))
            .env("HOME", home)
            .current_dir(&project));
        assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
        let (code, _, stderr) =
            run(ryl(home).current_dir(&project).args(["check", "a.yaml"]));
        assert_eq!(code, 0, "{stderr}");
        assert!(stderr.is_empty(), "{stderr}");
    }
}

#[test]
fn unquotable_paths_give_manual_instructions_without_a_command() {
    let td = tempdir().unwrap();
    let home = td.path();
    for (name, config) in [
        (".yamllint", "rules: {anchors: enable}\n"),
        (".ryl.toml", "[rules]\nanchors = 'enable'\n"),
    ] {
        let project = home.join(format!("{name}'$dir"));
        fs::create_dir(&project).unwrap();
        fs::write(project.join(name), config).unwrap();
        fs::write(project.join("a.yaml"), "a: 1\n").unwrap();
        let (code, _, warning) =
            run(ryl(home).current_dir(&project).args(["check", "a.yaml"]));
        assert_eq!(code, 0, "{warning}");
        assert!(
            warning.contains("quote this config's path and migrate it by hand"),
            "{warning}"
        );
        assert!(!warning.contains("run `ryl --migrate"), "{warning}");
    }
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
        "# YAML patterns\nyaml-files = ['*.yaml'] # keep this comment\n[rules]\nanchors = 'enable'\n",
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
    assert!(
        migrated.contains("yaml = ['*.yaml'] # keep this comment"),
        "{migrated}"
    );
    assert!(migrated.contains("# YAML patterns"), "{migrated}");
    let (code, _, stderr) = run(ryl(root).current_dir(root).args(["check", "a.yaml"]));
    assert_eq!(code, 0, "{stderr}");
    assert!(stderr.is_empty(), "{stderr}");
}

#[test]
fn existing_files_yaml_warns_when_legacy_yaml_files_is_discarded() {
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
    assert!(
        stderr.contains(
            "discarded deprecated `yaml-files` because `files.yaml` is also set"
        ),
        "{stderr}"
    );
    let migrated = fs::read_to_string(root.join(".ryl.toml")).unwrap();
    assert!(migrated.contains("yaml = ['*.yaml']"), "{migrated}");
    assert!(!migrated.contains("yaml-files"), "{migrated}");
}

#[test]
fn yaml_files_moves_into_an_inline_files_table() {
    let config = "yaml-files = ['*.yaml']\nfiles = { markdown = ['*.md'] }\n[lint.rules]\nanchors = 'enable'\n";
    let td = tempdir().unwrap();
    let path = td.path().join(".ryl.toml");
    fs::write(&path, config).unwrap();
    let (code, _, stderr) = run(ryl(td.path())
        .args(["--migrate-configs", "--migrate-write", "--migrate-root"])
        .arg(&path));
    assert_eq!(code, 0, "{stderr}");
    let migrated = fs::read_to_string(&path).unwrap();
    assert!(migrated.contains("yaml = ['*.yaml']"), "{migrated}");
    assert!(migrated.contains("markdown = ['*.md']"), "{migrated}");
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

fn migration_notice(name: &str, config: &str) -> (String, String) {
    let td = tempdir().unwrap();
    let path = td.path().join(name);
    fs::write(&path, config).unwrap();
    let (code, _, stderr) = run(ryl(td.path())
        .args(["--migrate-configs", "--migrate-root"])
        .arg(&path));
    assert_eq!(code, 0, "{stderr}");
    (stderr, fs::read_to_string(path).unwrap())
}

#[test]
fn malformed_toml_yaml_files_is_skipped() {
    let (notice, original) = migration_notice(".ryl.toml", "yaml-files = [\n");
    assert!(notice.contains("warning: skipping"), "{notice}");
    assert_eq!(original, "yaml-files = [\n");
}

#[test]
fn yaml_files_text_in_a_comment_does_not_trigger_a_rewrite() {
    let config = "# yaml-files\n[lint.rules]\nanchors = 'enable'\n";
    let (notice, original) = migration_notice(".ryl.toml", config);
    assert!(notice.is_empty(), "{notice}");
    assert_eq!(original, config);
}

#[test]
fn non_table_files_is_skipped_without_losing_yaml_files() {
    let config =
        "yaml-files = ['*.yaml']\nfiles = 5\n[lint.rules]\nanchors = 'enable'\n";
    let (notice, original) = migration_notice(".ryl.toml", config);
    assert!(notice.contains("`files` must be a table"), "{notice}");
    assert_eq!(original, config);
}

#[test]
fn unrelated_pyproject_yaml_files_text_is_ignored() {
    let config = "[tool.other]\nlabel = 'yaml-files'\n";
    let (notice, original) = migration_notice("pyproject.toml", config);
    assert!(notice.is_empty(), "{notice}");
    assert_eq!(original, config);
}

#[test]
fn pyproject_yaml_files_reports_the_manual_move() {
    let config = "[tool.ryl]\nyaml-files = ['*.yaml']\n[tool.ryl.lint.rules]\nanchors = 'enable'\n";
    let (notice, original) = migration_notice("pyproject.toml", config);
    assert!(notice.contains("`yaml-files` to `files.yaml`"), "{notice}");
    assert_eq!(original, config);
}

#[test]
fn pyproject_existing_files_yaml_reports_manual_removal() {
    let config =
        "[tool.ryl]\nyaml-files = ['*.yml']\n[tool.ryl.files]\nyaml = ['*.yaml']\n";
    let (notice, original) = migration_notice("pyproject.toml", config);
    assert!(
        notice.contains("remove `yaml-files` because `files.yaml` is already set"),
        "{notice}"
    );
    assert_eq!(original, config);
}
