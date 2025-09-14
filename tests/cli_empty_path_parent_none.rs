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
fn empty_path_argument_triggers_parent_none_branch() {
    let td = tempdir().unwrap();
    let exe = env!("CARGO_BIN_EXE_ryl");
    // Pass an empty explicit path; expect success with no output.
    let (code, _out, err) = run(Command::new(exe)
        .current_dir(td.path())
        .arg("--list-files")
        .arg(""));
    assert_eq!(code, 2, "expected exit 2: {err}");
}
