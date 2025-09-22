use std::fs;
use std::process::Command;

use tempfile::tempdir;

fn run(cmd: &mut Command) -> (i32, String, String) {
    let out = cmd.output().expect("process");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (code, stdout, stderr)
}

#[test]
fn invalid_inline_config_causes_exit_2() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("file.yaml");
    fs::write(&file, "key: value\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _out, err) = run(Command::new(exe)
        .arg("-d")
        .arg("extends: missing-config")
        .arg(&file));
    assert_eq!(code, 2, "missing inline extends should exit 2: {err}");
    assert!(
        err.contains("failed to read"),
        "expected config read error: {err}"
    );
}
