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
fn no_arguments_returns_usage_error_code_2() {
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _out, err) = run(&mut Command::new(exe));
    assert_eq!(code, 2, "expected usage error exit code 2: {err}");
    assert!(
        err.contains("expected one or more paths"),
        "expected usage message in stderr: {err}"
    );
}

#[test]
fn empty_directory_results_in_success() {
    let dir = tempdir().unwrap();
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _out, err) = run(Command::new(exe).arg(dir.path()));
    assert_eq!(code, 0, "expected success when no YAML files: {err}");
}

#[test]
fn non_existent_file_reports_read_error() {
    let exe = env!("CARGO_BIN_EXE_ryl");
    let bogus = std::path::Path::new("this/file/does/not/exist.yaml");
    let (code, _out, err) = run(Command::new(exe).arg(bogus));
    assert_eq!(code, 1, "expected failure for unreadable file");
    assert!(
        err.contains("failed to read"),
        "expected read error in stderr: {err}"
    );
}
