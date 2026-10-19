use std::fs;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

fn assert_completes(mut command: Command, expected_code: i32) {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn ryl");
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(90) {
        if let Some(status) = child.try_wait().expect("poll ryl") {
            assert_eq!(status.code(), Some(expected_code));
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    child.kill().expect("kill timed-out ryl");
    child.wait().expect("reap timed-out ryl");
    panic!("ryl exceeded the 90-second performance bound");
}

#[test]
fn gitlab_dense_duplicates_finish_within_bound() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("dense.yaml");
    fs::write(&input, "- yes\n".repeat(100_000)).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_ryl"));
    command
        .args([
            "check",
            "--no-cache",
            "-d",
            "[lint.rules.truthy]\nlevel = 'error'",
            "--output-format",
            "gitlab",
        ])
        .arg(input);
    assert_completes(command, 1);
}

#[test]
fn indentation_long_unicode_flow_line_finishes_within_bound() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("flow.yaml");
    let config = dir.path().join("format.toml");
    fs::write(&input, format!("é: [{}x]\n", "😀, ".repeat(600_000))).unwrap();
    fs::write(&config, "[format]\n").unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_ryl"));
    command
        .args([
            "check",
            "--no-cache",
            "-d",
            "[lint.rules]\nindentation = 'enable'",
        ])
        .arg(&input);
    assert_completes(command, 0);
    let mut command = Command::new(env!("CARGO_BIN_EXE_ryl"));
    command
        .args(["format", "--no-cache", "-c"])
        .arg(config)
        .arg(input);
    assert_completes(command, 0);
}
