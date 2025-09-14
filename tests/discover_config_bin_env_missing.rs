use std::path::PathBuf;
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
fn dc_env_points_to_missing_file_is_ignored() {
    let td = tempdir().unwrap();
    let missing: PathBuf = td.path().join("no_such_config.yml");
    let exe = env!("CARGO_BIN_EXE_discover_config_bin");
    let (code, out, err) = run(Command::new(exe)
        .env("YAMLLINT_CONFIG_FILE", &missing)
        .env("XDG_CONFIG_HOME", td.path().join("xdg"))
        .current_dir(td.path()));
    assert_eq!(code, 0, "expected success: {err}");
    assert_eq!(out.trim(), "");
}
