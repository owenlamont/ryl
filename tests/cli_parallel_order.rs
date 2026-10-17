use std::fs;
use std::process::Command;

use tempfile::tempdir;

#[test]
fn format_diff_keeps_input_order_across_many_files() {
    let dir = tempdir().unwrap();
    let files: Vec<_> = (0..64)
        .map(|index| {
            let path = dir.path().join(format!("{index:03}.yaml"));
            fs::write(&path, format!("key{index}:  value\n")).unwrap();
            path
        })
        .collect();
    let exe = env!("CARGO_BIN_EXE_ryl");
    let mut expected = Vec::new();
    for path in files.iter().rev() {
        let output = Command::new(exe)
            .args(["format", "--diff", "--no-warnings"])
            .arg(path)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        expected.extend(output.stdout);
    }

    let output = Command::new(exe)
        .args(["format", "--diff", "--no-warnings"])
        .args(files.iter().rev())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(
        output.stdout == expected,
        "parallel diff order differs from serial output"
    );
}

#[cfg(unix)]
#[test]
fn format_diff_keeps_symlink_and_encoding_warnings_in_input_order() {
    use std::os::unix::fs::symlink;

    let dir = tempdir().unwrap();
    let target = dir.path().join("target.yaml");
    let link = dir.path().join("link.yaml");
    let regular = dir.path().join("regular.yaml");
    fs::write(&target, "key:  value\n").unwrap();
    fs::write(&regular, "key:  value\n").unwrap();
    symlink(&target, &link).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_ryl"))
        .args(["format", "--diff", "--no-warnings"])
        .env("YAMLLINT_FILE_ENCODING", "utf-8")
        .arg(&link)
        .arg(&regular)
        .output()
        .unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    let symlink_notice = stderr.find("refusing to follow a symlink").unwrap();
    let encoding_notice = stderr.find("YAMLLINT_FILE_ENCODING").unwrap();

    assert!(
        symlink_notice < encoding_notice,
        "warnings are out of input order: {stderr}"
    );
}
