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
