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
fn user_global_config_with_invalid_yaml_errors() {
    let td = tempdir().unwrap();
    let xdg = td.path().join("xdg").join("yamllint");
    fs::create_dir_all(&xdg).unwrap();
    fs::write(xdg.join("config"), "rules: {\n").unwrap();

    let proj = td.path().join("proj");
    fs::create_dir_all(&proj).unwrap();
    fs::write(proj.join("a.yaml"), "a: 1\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _out, err) = run(Command::new(exe)
        .env("XDG_CONFIG_HOME", td.path().join("xdg"))
        .arg("--list-files")
        .arg(&proj));
    assert_eq!(code, 2, "expected exit 2: {err}");
    assert!(err.contains("failed to parse config data"));
}
