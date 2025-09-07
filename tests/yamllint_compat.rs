use std::fs;
use std::path::PathBuf;
use std::process::Command;

use tempfile::tempdir;

fn run_cmd(cmd: &mut Command) -> (i32, String, String) {
    let out = cmd.output().expect("failed to spawn process");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (code, stdout, stderr)
}

fn write_file(dir: &std::path::Path, name: &str, content: &str) -> PathBuf {
    let p = dir.join(name);
    fs::write(&p, content).expect("write file");
    p
}

fn yamllint_available() -> bool {
    Command::new("yamllint")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
#[ignore]
fn yamllint_exit_behavior_matches_for_syntax_only() {
    if !yamllint_available() {
        eprintln!("yamllint not found in PATH; skipping");
        return;
    }

    let dir = tempdir().unwrap();
    let ok = write_file(dir.path(), "ok.yaml", "a: 1\n");
    let bad = write_file(dir.path(), "bad.yaml", "a: [1, 2\n");

    // Disable yamllint rules so we only compare syntax behavior.
    let cfg = write_file(dir.path(), ".yamllint.yml", "rules: {}\n");

    let ryl = env!("CARGO_BIN_EXE_ryl");

    // Valid file: both should succeed.
    let (ryl_ok, _, _) = run_cmd(Command::new(ryl).arg(&ok));
    let (y_ok, _, y_err) = run_cmd(Command::new("yamllint").arg("-c").arg(&cfg).arg(&ok));
    assert_eq!(
        ryl_ok, 0,
        "ryl should return 0 on valid yaml: stdout/stderr from yamllint: {y_err}"
    );
    assert_eq!(y_ok, 0, "yamllint should return 0 on valid yaml");

    // Invalid file: both should be non-zero.
    let (ryl_bad, _, r_err) = run_cmd(Command::new(ryl).arg(&bad));
    let (y_bad, _, y_err) = run_cmd(Command::new("yamllint").arg("-c").arg(&cfg).arg(&bad));
    assert_ne!(ryl_bad, 0, "ryl should fail on invalid yaml: {r_err}");
    assert_ne!(y_bad, 0, "yamllint should fail on invalid yaml: {y_err}");
}
