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
fn list_files_respects_ignores_in_project_config() {
    let td = tempdir().unwrap();
    let proj = td.path();

    // Files
    fs::create_dir_all(proj.join("docs")).unwrap();
    fs::write(proj.join("a.yaml"), "a: 1\n").unwrap();
    fs::write(proj.join("b.yaml"), "b: 2\n").unwrap();
    fs::write(proj.join("a.skip.yaml"), "x: 0\n").unwrap();
    fs::write(proj.join("docs/ignored.yaml"), "y: 0\n").unwrap();

    // Project config
    fs::write(
        proj.join(".yamllint"),
        "ignore: ['**/*.skip.yaml', 'docs/**']\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, out, err) = run(Command::new(exe).arg("--list-files").arg(proj));
    assert_eq!(code, 0, "expected success: {err}");

    let mut lines: Vec<_> = out.lines().collect();
    lines.sort_unstable();

    let a = proj.join("a.yaml").display().to_string();
    let b = proj.join("b.yaml").display().to_string();
    assert_eq!(lines, vec![a.as_str(), b.as_str()]);
}

#[test]
fn list_files_empty_dir_prints_nothing() {
    let td = tempdir().unwrap();
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, out, err) = run(Command::new(exe).arg("--list-files").arg(td.path()));
    assert_eq!(code, 0, "expected success: {err}");
    assert!(out.trim().is_empty(), "expected no output: {out}");
}

#[test]
fn explicit_files_are_filtered_by_ignores() {
    let td = tempdir().unwrap();
    let proj = td.path();

    fs::write(proj.join(".yamllint.yml"), "ignore: ['**/*.skip.yaml']\n").unwrap();
    fs::write(proj.join("keep.yaml"), "ok: 1\n").unwrap();
    fs::write(proj.join("x.skip.yaml"), "ok: 1\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, out, err) = run(Command::new(exe)
        .arg("--list-files")
        .arg(proj)
        .arg(proj.join("x.skip.yaml")));
    assert_eq!(code, 0, "expected success: {err}");

    let lines: Vec<_> = out.lines().collect();
    assert!(lines.iter().any(|l| l.ends_with("keep.yaml")));
    assert!(
        !lines.iter().any(|l| l.ends_with("x.skip.yaml")),
        "explicit file matching ignore should be filtered (parity)"
    );
}
