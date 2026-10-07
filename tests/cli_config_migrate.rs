use std::fs;
use std::path::PathBuf;
use std::process::Command;

use tempfile::tempdir;

fn run(cmd: &mut Command) -> (i32, String, String) {
    let out = cmd.output().expect("failed to run ryl");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy_owned(out.stdout);
    let stderr = String::from_utf8_lossy_owned(out.stderr);
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
    assert!(toml.contains("document-start = \"disable\""));
    assert!(toml.contains("[lint.rules.truthy]"));
    assert!(toml.contains("level = \"error\""));
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
    assert!(stdout.contains("No legacy config files migrated under"));
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
    assert!(stdout.contains("[lint.rules]"));
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

// The user-global tests point XDG_CONFIG_HOME at a tempdir so they resolve the yamllint
// source and ryl target inside it, never touching the real config directory.
#[test]
fn migrate_user_config_write_creates_ryl_toml() {
    let td = tempdir().unwrap();
    let xdg = td.path();
    fs::create_dir_all(xdg.join("yamllint")).unwrap();
    fs::write(
        xdg.join("yamllint").join("config"),
        "rules: { key-duplicates: enable }\n",
    )
    .unwrap();
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .env("XDG_CONFIG_HOME", xdg)
        .arg("--migrate-user-config")
        .arg("--migrate-write"));
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    let toml = fs::read_to_string(xdg.join("ryl").join("ryl.toml")).unwrap();
    assert!(toml.contains("key-duplicates = \"enable\""), "got: {toml}");
}

#[test]
fn migrate_user_config_dry_run_previews_without_writing() {
    let td = tempdir().unwrap();
    let xdg = td.path();
    fs::create_dir_all(xdg.join("yamllint")).unwrap();
    fs::write(
        xdg.join("yamllint").join("config"),
        "rules: { key-duplicates: enable }\n",
    )
    .unwrap();
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .env("XDG_CONFIG_HOME", xdg)
        .arg("--migrate-user-config"));
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert!(
        stdout.contains("config ->"),
        "preview line expected: {stdout}"
    );
    assert!(!xdg.join("ryl").join("ryl.toml").exists());
}

#[test]
fn migrate_user_config_absent_source_reports_message() {
    let td = tempdir().unwrap();
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .env("XDG_CONFIG_HOME", td.path())
        .arg("--migrate-user-config"));
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert!(
        stdout.contains("No user-global config migrated"),
        "got: {stdout}"
    );
}

#[test]
fn migrate_combined_project_and_user_config_in_one_run() {
    let td = tempdir().unwrap();
    let proj = td.path().join("proj");
    fs::create_dir_all(&proj).unwrap();
    fs::write(
        proj.join(".yamllint"),
        "rules: { key-duplicates: enable }\n",
    )
    .unwrap();
    let xdg = td.path().join("xdg");
    fs::create_dir_all(xdg.join("yamllint")).unwrap();
    fs::write(
        xdg.join("yamllint").join("config"),
        "rules: { key-duplicates: enable }\n",
    )
    .unwrap();
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .env("XDG_CONFIG_HOME", &xdg)
        .arg("--migrate-configs")
        .arg("--migrate-root")
        .arg(&proj)
        .arg("--migrate-user-config")
        .arg("--migrate-write"));
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert!(proj.join(".ryl.toml").exists(), "project config migrated");
    assert!(
        xdg.join("ryl").join("ryl.toml").exists(),
        "user-global migrated"
    );
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

#[test]
fn migrate_combined_reports_absent_user_config_even_when_project_migrates() {
    let td = tempdir().unwrap();
    let proj = td.path().join("proj");
    fs::create_dir_all(&proj).unwrap();
    fs::write(
        proj.join(".yamllint"),
        "rules: { key-duplicates: enable }\n",
    )
    .unwrap();
    // XDG dir exists but holds no yamllint/config, so the user-global source is absent.
    let xdg = td.path().join("xdg");
    fs::create_dir_all(&xdg).unwrap();
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .env("XDG_CONFIG_HOME", &xdg)
        .arg("--migrate-configs")
        .arg("--migrate-root")
        .arg(&proj)
        .arg("--migrate-user-config"));
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert!(
        stdout.contains(".yamllint ->"),
        "project migrated: {stdout}"
    );
    assert!(
        stdout.contains("No user-global config migrated"),
        "absent user-global reported even though the project migrated: {stdout}"
    );
}

#[test]
fn migrate_rejects_positional_path() {
    let td = tempdir().unwrap();
    fs::create_dir(td.path().join("sub")).unwrap();
    fs::write(td.path().join("sub/.yamllint"), "rules: {}\n").unwrap();
    let exe = env!("CARGO_BIN_EXE_ryl");
    for trigger in ["--migrate-configs", "--migrate-user-config"] {
        let (code, _stdout, stderr) = run(Command::new(exe)
            .current_dir(td.path())
            .env("HOME", td.path())
            .env("XDG_CONFIG_HOME", td.path())
            .args([trigger, "--migrate-write", "sub"]));
        assert_eq!(code, 2, "{trigger}: stderr={stderr}");
        assert!(
            stderr.contains("--migrate-root") && !stderr.contains("deprecated"),
            "{trigger}: {stderr}"
        );
    }
    assert!(!td.path().join("sub/.ryl.toml").exists());
}

fn user_config_with_ignore_files(rules_yaml: &str) -> (tempfile::TempDir, PathBuf) {
    let td = tempdir().unwrap();
    let yamllint_dir = td.path().join("xdg").join("yamllint");
    let project = td.path().join("project");
    fs::create_dir_all(&yamllint_dir).unwrap();
    fs::create_dir_all(&project).unwrap();
    fs::write(yamllint_dir.join("config"), rules_yaml).unwrap();
    fs::write(yamllint_dir.join("ignores.txt"), "beside-source/\n").unwrap();
    fs::write(project.join("ignores.txt"), "from-cwd/\n").unwrap();
    (td, project)
}

#[test]
fn migrate_user_config_refuses_relative_rule_level_ignore_from_file() {
    let (td, project) = user_config_with_ignore_files(
        "rules:\n  key-duplicates:\n    ignore-from-file: ignores.txt\n",
    );
    let source = td.path().join("xdg").join("yamllint").join("config");
    let (code, stdout, stderr) = run(Command::new(env!("CARGO_BIN_EXE_ryl"))
        .current_dir(&project)
        .env("XDG_CONFIG_HOME", td.path().join("xdg"))
        .args([
            "--migrate-user-config",
            "--migrate-write",
            "--migrate-delete-old",
        ]));
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    assert!(
        stderr.contains("relative ignore-from-file `ignores.txt`"),
        "got: {stderr}"
    );
    assert!(
        stdout.contains("No user-global config migrated."),
        "got: {stdout}"
    );
    assert!(
        source.exists(),
        "a refused source is kept, even with --delete-old"
    );
}
