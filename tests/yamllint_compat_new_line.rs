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

fn ensure_yamllint_installed() {
    let ok = Command::new("yamllint")
        .arg("--version")
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false);
    assert!(ok, "yamllint must be installed for compatibility tests");
}

fn normalize_output(stdout: String, stderr: String) -> String {
    if stderr.is_empty() { stdout } else { stderr }
}

#[test]
fn new_line_rule_matches_yamllint() {
    ensure_yamllint_installed();

    let dir = tempdir().unwrap();
    let cfg = dir.path().join("config.yml");
    fs::write(
        &cfg,
        "rules:\n  document-start: disable\n  new-line-at-end-of-file: enable\n",
    )
    .unwrap();

    let missing = dir.path().join("missing.yaml");
    fs::write(&missing, "key: value").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (ryl_code, ryl_out, ryl_err) = run(Command::new(exe).arg("-c").arg(&cfg).arg(&missing));
    let (yam_code, yam_out, yam_err) =
        run(Command::new("yamllint").arg("-c").arg(&cfg).arg(&missing));

    assert_eq!(ryl_code, 1, "ryl exit code for missing newline");
    assert_eq!(yam_code, 1, "yamllint exit code for missing newline");
    assert_eq!(
        normalize_output(ryl_out, ryl_err),
        normalize_output(yam_out, yam_err),
        "expected identical diagnostics for missing newline",
    );

    let invalid = dir.path().join("invalid.yaml");
    fs::write(&invalid, "key: [1").unwrap();

    let (ryl_bad_code, ryl_bad_out, ryl_bad_err) =
        run(Command::new(exe).arg("-c").arg(&cfg).arg(&invalid));
    let (yam_bad_code, yam_bad_out, yam_bad_err) =
        run(Command::new("yamllint").arg("-c").arg(&cfg).arg(&invalid));

    assert_eq!(ryl_bad_code, 1, "ryl exit code for invalid yaml");
    assert_eq!(yam_bad_code, 1, "yamllint exit code for invalid yaml");
    let ryl_bad = normalize_output(ryl_bad_out, ryl_bad_err);
    let yam_bad = normalize_output(yam_bad_out, yam_bad_err);
    assert!(
        ryl_bad.contains("syntax error"),
        "ryl should report a syntax error: {ryl_bad}"
    );
    assert!(
        yam_bad.contains("syntax error"),
        "yamllint should report a syntax error: {yam_bad}"
    );
    assert!(
        !ryl_bad.contains("no new line character"),
        "new line rule should be suppressed when syntax fails: {ryl_bad}"
    );
}
