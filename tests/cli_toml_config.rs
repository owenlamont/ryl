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
fn project_toml_overrides_yaml_and_emits_single_warning() {
    let td = tempdir().unwrap();
    let root = td.path();
    fs::create_dir_all(root.join("dir")).unwrap();
    fs::write(root.join("dir/a.yaml"), "a: 1\n").unwrap();
    fs::write(root.join("dir/b.yaml"), "b: 2\n").unwrap();
    fs::write(
        root.join(".ryl.toml"),
        "files = { yaml = ['**/a.yaml', '**/b.yaml'] }\n[rules]\nanchors = 'disable'\n",
    )
    .unwrap();
    fs::write(root.join(".yamllint"), "yaml-files: ['**/a.yaml']\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("--list-files").arg(root));
    assert_eq!(code, 0, "expected success: stdout={stdout} stderr={stderr}");
    assert!(stdout.contains("a.yaml"));
    assert!(stdout.contains("b.yaml"));
    let warning_count = stderr
        .matches("warning: ignoring legacy YAML config discovery because TOML config")
        .count();
    assert_eq!(warning_count, 1, "stderr={stderr}");
}

#[test]
fn explicit_pyproject_without_tool_ryl_errors() {
    let td = tempdir().unwrap();
    let root = td.path();
    let pyproject = root.join("pyproject.toml");
    fs::write(
        &pyproject,
        "[project]\nname = 'demo'\nversion = '0.1.0'\nrequires-python = '>=3.10'\n",
    )
    .unwrap();
    fs::write(root.join("a.yaml"), "a: 1\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) = run(Command::new(exe)
        .arg("--list-files")
        .arg("-c")
        .arg(&pyproject)
        .arg(root.join("a.yaml")));
    assert_eq!(code, 2);
    assert!(stderr.contains("missing [tool.ryl] section"));
}

#[test]
fn unrecognised_top_level_toml_key_is_rejected() {
    // A valid rule is present so the failure is specifically the unknown key, not the
    // "no rules enabled" guard. ryl's TOML config is the single explicit source of a
    // file's rules, so a stray/typo'd top-level key (here `preset`) must error rather
    // than be silently ignored.
    let td = tempdir().unwrap();
    let root = td.path();
    let cfg = root.join("ryl.toml");
    fs::write(
        &cfg,
        "preset = 'relaxed'\n[rules]\ndocument-start = 'enable'\n",
    )
    .unwrap();
    fs::write(root.join("a.yaml"), "a: 1\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) = run(Command::new(exe)
        .arg("-c")
        .arg(&cfg)
        .arg(root.join("a.yaml")));
    assert_eq!(
        code, 2,
        "an unrecognised top-level TOML key must error: {stderr}"
    );
    assert!(
        stderr.contains("unrecognised TOML configuration key")
            && stderr.contains("`preset`"),
        "error should name the unknown key: {stderr}"
    );
}

#[test]
fn discovered_empty_toml_config_is_rejected() {
    let td = tempdir().unwrap();
    let root = td.path();
    fs::write(root.join(".ryl.toml"), "").unwrap();
    fs::write(root.join("a.yaml"), "a: 1\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) = run(Command::new(exe).arg(root.join("a.yaml")));
    assert_eq!(
        code, 2,
        "an empty config must error, not silently lint nothing: {stderr}"
    );
    assert!(stderr.contains("configuration is empty"), "{stderr}");
}

#[test]
fn discovered_pyproject_with_empty_tool_ryl_is_rejected() {
    // A present-but-empty `[tool.ryl]` enables no rules; ryl rejects it rather than
    // silently handing back the default rule set (rules must be explicitly enabled).
    // An *absent* `[tool.ryl]` is different: discovery keeps looking.
    let td = tempdir().unwrap();
    let root = td.path();
    fs::write(root.join("pyproject.toml"), "[tool.ryl]\n").unwrap();
    fs::write(root.join("a.yaml"), "a: 1\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) = run(Command::new(exe).arg(root.join("a.yaml")));
    assert_eq!(code, 2, "an empty [tool.ryl] must be rejected: {stderr}");
    assert!(stderr.contains("configuration is empty"), "{stderr}");
}

#[test]
fn global_config_notice_is_emitted_when_env_var_triggers_global_discovery() {
    let td = tempdir().unwrap();
    let root = td.path();
    fs::write(root.join("a.yaml"), "a: 1\n").unwrap();
    fs::write(root.join(".ryl.toml"), "[rules]\nanchors = 'disable'\n").unwrap();
    fs::write(root.join(".yamllint"), "rules: {}\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) = run(Command::new(exe)
        .env("YAMLLINT_CONFIG_FILE", "/does/not/exist.yml")
        .arg("--list-files")
        .arg(root.join("a.yaml")));
    assert_eq!(code, 0, "stderr={stderr}");
    assert!(stderr.contains(
        "warning: ignoring legacy YAML config discovery because TOML config"
    ));
}

#[test]
fn explicit_file_discovery_emits_notice() {
    let td = tempdir().unwrap();
    let root = td.path();
    fs::write(root.join("a.yaml"), "a: 1\n").unwrap();
    fs::write(root.join(".ryl.toml"), "[rules]\nanchors = 'disable'\n").unwrap();
    fs::write(root.join(".yamllint"), "rules: {}\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) = run(Command::new(exe)
        .arg("--list-files")
        .arg(root.join("a.yaml")));
    assert_eq!(code, 0, "stderr={stderr}");
    assert!(stderr.contains(
        "warning: ignoring legacy YAML config discovery because TOML config"
    ));
}

#[test]
fn explicit_notice_is_deduplicated_after_directory_notice() {
    let td = tempdir().unwrap();
    let root = td.path();
    fs::create_dir_all(root.join("one")).unwrap();
    fs::create_dir_all(root.join("two")).unwrap();
    fs::write(root.join("one/from_dir.yaml"), "a: 1\n").unwrap();
    fs::write(root.join("two/from_file.yaml"), "b: 2\n").unwrap();
    fs::write(root.join(".ryl.toml"), "[rules]\nanchors = 'disable'\n").unwrap();
    fs::write(root.join(".yamllint"), "rules: {}\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) = run(Command::new(exe)
        .arg("--list-files")
        .arg(root.join("one"))
        .arg(root.join("two/from_file.yaml")));
    assert_eq!(code, 0, "stderr={stderr}");
    let warning_count = stderr
        .matches("warning: ignoring legacy YAML config discovery because TOML config")
        .count();
    assert_eq!(warning_count, 1, "stderr={stderr}");
}
