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
fn user_global_config_via_xdg_is_respected() {
    let td = tempdir().unwrap();
    let xdg = td.path().join("xdg").join("yamllint");
    fs::create_dir_all(&xdg).unwrap();
    fs::write(xdg.join("config"), "ignore: ['a.yaml']\n").unwrap();

    let proj = td.path().join("proj");
    fs::create_dir_all(&proj).unwrap();
    fs::write(proj.join("a.yaml"), "a: 1\n").unwrap();
    fs::write(proj.join("b.yaml"), "b: 1\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, out, err) = run(Command::new(exe)
        .env("XDG_CONFIG_HOME", td.path().join("xdg"))
        .arg("--list-files")
        .arg(&proj));
    assert_eq!(code, 0, "expected success: {err}");
    assert!(!out.contains("a.yaml"));
    assert!(out.contains("b.yaml"));
}
