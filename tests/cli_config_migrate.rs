use std::fs;
use std::process::Command;

use tempfile::tempdir;

fn run(cmd: &mut Command) -> (i32, String, String) {
    let out = cmd.output().expect("failed to run ryl");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (code, stdout, stderr)
}

#[test]
fn migrate_configs_dry_run_does_not_write_files() {
    let td = tempdir().unwrap();
    let root = td.path();
    fs::write(
        root.join(".yamllint"),
        "rules: { document-start: disable }\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("--migrate-configs")
        .arg("--migrate-root")
        .arg(root));
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert!(stdout.contains(".yamllint ->"));
    assert!(!root.join(".ryl.toml").exists());
}

#[test]
fn migrate_configs_write_with_rename_flattens_extends() {
    let td = tempdir().unwrap();
    let root = td.path();
    fs::write(
        root.join("base.yaml"),
        "rules: { truthy: { level: error } }\n",
    )
    .unwrap();
    fs::write(
        root.join(".yamllint"),
        "extends: base.yaml\nrules: { document-start: disable }\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("--migrate-configs")
        .arg("--migrate-root")
        .arg(root)
        .arg("--migrate-write")
        .arg("--migrate-rename-old")
        .arg(".bak"));
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");

    let toml = fs::read_to_string(root.join(".ryl.toml")).unwrap();
    assert!(toml.contains("document-start = 'disable'"));
    assert!(toml.contains("[rules.truthy]"));
    assert!(toml.contains("level = 'error'"));
    assert!(!root.join(".yamllint").exists());
    assert!(root.join(".yamllint.bak").exists());
}

#[test]
fn migrate_configs_warns_when_multiple_yaml_configs_share_directory() {
    let td = tempdir().unwrap();
    let root = td.path();
    fs::write(root.join(".yamllint"), "rules: {}\n").unwrap();
    fs::write(
        root.join(".yamllint.yml"),
        "rules: { document-start: disable }\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) = run(Command::new(exe)
        .arg("--migrate-configs")
        .arg("--migrate-root")
        .arg(root));
    assert_eq!(code, 0, "stderr={stderr}");
    assert!(stderr.contains("warning: skipping lower-precedence config"));
}

#[test]
fn migrate_configs_write_with_delete_old_removes_skipped_lower_precedence_files() {
    let td = tempdir().unwrap();
    let root = td.path();
    let primary = root.join(".yamllint");
    let skipped = root.join(".yamllint.yml");
    fs::write(&primary, "rules: {}\n").unwrap();
    fs::write(&skipped, "rules: { document-start: disable }\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) = run(Command::new(exe)
        .arg("--migrate-configs")
        .arg("--migrate-root")
        .arg(root)
        .arg("--migrate-write")
        .arg("--migrate-delete-old"));
    assert_eq!(code, 0, "stderr={stderr}");
    assert!(!primary.exists());
    assert!(!skipped.exists());
}

#[test]
fn migrate_configs_write_with_rename_old_renames_skipped_lower_precedence_files() {
    let td = tempdir().unwrap();
    let root = td.path();
    let primary = root.join(".yamllint");
    let skipped = root.join(".yamllint.yml");
    fs::write(&primary, "rules: {}\n").unwrap();
    fs::write(&skipped, "rules: { document-start: disable }\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) = run(Command::new(exe)
        .arg("--migrate-configs")
        .arg("--migrate-root")
        .arg(root)
        .arg("--migrate-write")
        .arg("--migrate-rename-old")
        .arg(".bak"));
    assert_eq!(code, 0, "stderr={stderr}");
    assert!(!primary.exists());
    assert!(!skipped.exists());
    assert!(root.join(".yamllint.bak").exists());
    assert!(root.join(".yamllint.yml.bak").exists());
}

#[test]
fn migrate_rename_and_delete_conflict() {
    let td = tempdir().unwrap();
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) = run(Command::new(exe)
        .arg("--migrate-configs")
        .arg("--migrate-root")
        .arg(td.path())
        .arg("--migrate-write")
        .arg("--migrate-rename-old")
        .arg(".bak")
        .arg("--migrate-delete-old"));
    assert_eq!(code, 2, "stderr={stderr}");
    assert!(stderr.contains("cannot be used with"));
}

#[test]
fn migrate_delete_old_requires_write() {
    let td = tempdir().unwrap();
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) = run(Command::new(exe)
        .arg("--migrate-configs")
        .arg("--migrate-root")
        .arg(td.path())
        .arg("--migrate-delete-old"));
    assert_eq!(code, 2, "stderr={stderr}");
    assert!(stderr.contains("required"));
}

#[test]
fn migrate_options_require_migrate_configs() {
    let td = tempdir().unwrap();
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) = run(Command::new(exe)
        .arg("--migrate-root")
        .arg(td.path())
        .arg(td.path()));
    assert_eq!(code, 2, "stderr={stderr}");
    assert!(stderr.contains("--migrate-configs"));
}

#[test]
fn migrate_configs_empty_default_root_prints_no_configs_message() {
    let td = tempdir().unwrap();
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .current_dir(td.path())
        .arg("--migrate-configs"));
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert!(stdout.contains("No legacy YAML config files found under"));
}

#[test]
fn migrate_configs_stdout_prints_generated_toml() {
    let td = tempdir().unwrap();
    fs::write(
        td.path().join(".yamllint"),
        "rules: { document-start: disable }\n",
    )
    .unwrap();
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("--migrate-configs")
        .arg("--migrate-root")
        .arg(td.path())
        .arg("--migrate-stdout"));
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert!(stdout.contains("# "));
    assert!(stdout.contains("[rules]"));
}

#[test]
fn migrate_configs_write_with_delete_old_removes_source() {
    let td = tempdir().unwrap();
    let source = td.path().join(".yamllint");
    fs::write(&source, "rules: { document-start: disable }\n").unwrap();
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) = run(Command::new(exe)
        .arg("--migrate-configs")
        .arg("--migrate-root")
        .arg(td.path())
        .arg("--migrate-write")
        .arg("--migrate-delete-old"));
    assert_eq!(code, 0, "stderr={stderr}");
    assert!(!source.exists());
    assert!(td.path().join(".ryl.toml").exists());
}

#[test]
fn migrate_configs_missing_root_returns_usage_error() {
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) = run(Command::new(exe)
        .arg("--migrate-configs")
        .arg("--migrate-root")
        .arg("/definitely/no/such/ryl/path"));
    assert_eq!(code, 2, "stderr={stderr}");
    assert!(stderr.contains("migrate root does not exist"));
}
