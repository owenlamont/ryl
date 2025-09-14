use std::fs;
use std::process::Command;

use tempfile::tempdir;

fn run(cmd: &mut Command) -> (i32, String, String) {
    let out = cmd.output().expect("failed to run helper");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (code, stdout, stderr)
}

#[test]
fn dc_ok_prints_empty_when_no_configs() {
    let td = tempdir().unwrap();
    let exe = env!("CARGO_BIN_EXE_discover_config_bin");
    let (code, out, err) = run(Command::new(exe).arg(td.path()));
    assert_eq!(code, 0, "expected success: {err}");
    assert_eq!(out.trim(), "");
}

#[test]
fn dc_ok_prints_env_config_path() {
    let td = tempdir().unwrap();
    let cfg = td.path().join("envcfg.yml");
    fs::write(&cfg, "ignore: ['**/skip/**']\n").unwrap();
    let exe = env!("CARGO_BIN_EXE_discover_config_bin");
    let (code, out, err) = run(Command::new(exe)
        .env("YAMLLINT_CONFIG_FILE", &cfg)
        .arg(td.path()));
    assert_eq!(code, 0, "expected success: {err}");
    assert!(out.trim_end().ends_with(cfg.to_string_lossy().as_ref()));
}

#[test]
fn dc_error_on_project_config_dir() {
    let td = tempdir().unwrap();
    let root = td.path();
    fs::create_dir(root.join(".yamllint")).unwrap();
    fs::write(root.join("a.yaml"), "a: 1\n").unwrap();
    let exe = env!("CARGO_BIN_EXE_discover_config_bin");
    let (code, _out, err) = run(Command::new(exe).arg(root));
    assert_eq!(code, 2, "expected exit 2: {err}");
    assert!(err.contains("failed to read"));
}

#[test]
fn dc_ok_prints_user_global_path_when_present() {
    let td = tempdir().unwrap();
    let xdg = td.path().join("xdg").join("yamllint");
    fs::create_dir_all(&xdg).unwrap();
    let global_cfg = xdg.join("config");
    fs::write(&global_cfg, "ignore: ['**/a.yaml']\n").unwrap();
    let proj = td.path().join("proj");
    fs::create_dir_all(&proj).unwrap();
    let exe = env!("CARGO_BIN_EXE_discover_config_bin");
    let (code, out, err) = run(Command::new(exe)
        .env("XDG_CONFIG_HOME", td.path().join("xdg"))
        .arg(&proj));
    assert_eq!(code, 0, "expected success: {err}");
    assert!(
        out.trim_end()
            .ends_with(global_cfg.to_string_lossy().as_ref())
    );
}
