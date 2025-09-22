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
fn list_files_outputs_expected_entries() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("sample.yaml");
    fs::write(&file, "key: value\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("--list-files").arg(dir.path()));
    assert_eq!(code, 0, "list-files should succeed: stderr={stderr}");
    assert!(stderr.trim().is_empty(), "unexpected stderr: {stderr}");
    assert!(
        stdout.contains("sample.yaml"),
        "expected stdout to include listed file: {stdout}"
    );
}
