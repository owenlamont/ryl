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
fn fix_respects_new_lines_ignore_for_eof_newline() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");

    // File with LF but missing EOF newline.
    // We want to verify that if new-lines rule is set to 'dos' but ignored for this file,
    // --fix uses the original newline style (LF) for the added EOF newline.
    fs::write(&file, "key: value").unwrap();

    fs::write(
        dir.path().join(".ryl.toml"),
        r#"[rules]
new-line-at-end-of-file = "enable"
document-start = "disable"

[rules.new-lines]
type = "dos"
ignore = ["input.yaml"]
"#,
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("--fix").arg(&file));
    assert_eq!(
        code, 0,
        "fix should succeed: stdout={stdout} stderr={stderr}"
    );

    let content = fs::read_to_string(&file).unwrap();
    // It should have added '\n' (detected from file or default) NOT '\r\n' (from ignored rule).
    assert_eq!(content, "key: value\n");
}
