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
fn dc_ok_env_config_path_relative_resolves_base_dir_from_cwd() {
    let td = tempdir().unwrap();
    // Write env-config at a relative path.
    fs::write(td.path().join("cfg.yml"), "rules: {}\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_discover_config_bin");
    let (code, out, err) = run(Command::new(exe)
        .env("YAMLLINT_CONFIG_FILE", "cfg.yml")
        .current_dir(td.path())
        .arg(td.path()));
    assert_eq!(code, 0, "expected success: {err}");
    assert!(out.trim_end().ends_with("cfg.yml"));
}
