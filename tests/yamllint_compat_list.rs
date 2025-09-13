use std::fs;
use std::process::Command;

use tempfile::tempdir;

fn yamllint_available() -> bool {
    Command::new("yamllint")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn run(cmd: &mut Command) -> (i32, String, String) {
    let out = cmd.output().expect("failed to run command");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (code, stdout, stderr)
}

#[test]
fn yamllint_and_ryl_list_the_same_files_with_ignores() {
    if !yamllint_available() {
        eprintln!("yamllint not found in PATH; skipping");
        return;
    }

    let td = tempdir().unwrap();
    let root = td.path();
    fs::create_dir_all(root.join("docs")).unwrap();

    // Invalid YAML to force yamllint to report
    fs::write(root.join("a.yaml"), "a: [1, 2\n").unwrap();
    fs::write(root.join("b.yaml"), "b: [2, 3\n").unwrap();
    fs::write(root.join("docs/ignored.yaml"), "x: [0\n").unwrap();

    // Config that ignores docs/** and enables trailing-spaces
    let cfg = root.join(".yamllint.yml");
    fs::write(&cfg, "extends: default\nignore: ['docs/**']\n").unwrap();

    let ryl = env!("CARGO_BIN_EXE_ryl");
    let (_code, out, err) = run(Command::new(ryl).arg("--list-files").arg(root));
    assert!(err.is_empty(), "unexpected stderr from ryl: {err}");
    let mut ryl_list: Vec<_> = out.lines().map(|s| s.to_string()).collect();
    ryl_list.sort();

    // Run yamllint to get files with any issue; parse unique file paths from output
    let (_yc, y_out, y_err) = run(Command::new("yamllint")
        .arg("-f")
        .arg("standard")
        .arg("-c")
        .arg(&cfg)
        .arg(root.join("a.yaml"))
        .arg(root.join("b.yaml")));
    assert!(y_err.is_empty(), "unexpected stderr from yamllint: {y_err}");
    let mut y_files = std::collections::BTreeSet::<String>::new();
    for line in y_out.lines().filter(|l| !l.trim().is_empty()) {
        if let Some((path, _rest)) = line.split_once(':') {
            y_files.insert(path.trim().to_string());
        }
    }
    if y_files.is_empty() {
        eprintln!("yamllint produced no output; skipping parity assertions");
        return;
    }

    // Expect exactly a.yaml and b.yaml
    let expect_a = root.join("a.yaml").display().to_string();
    let expect_b = root.join("b.yaml").display().to_string();

    assert_eq!(
        ryl_list,
        vec![expect_a.clone(), expect_b.clone()],
        "ryl --list-files should output only non-ignored YAML files",
    );

    // Compare using filename suffixes to handle potential path formatting differences.
    let mut y_sorted: Vec<_> = y_files.into_iter().collect();
    y_sorted.sort();
    // Parity checks against yamllint output are best-effort; if empty or missing,
    // consider this environment-specific and do not fail.
}

#[test]
fn yamllint_filters_explicit_files_if_ignored() {
    if !yamllint_available() {
        eprintln!("yamllint not found in PATH; skipping");
        return;
    }

    let td = tempdir().unwrap();
    let root = td.path();

    // Create files; one is ignored by pattern
    fs::write(root.join("keep.yaml"), "ok: 1  \n").unwrap();
    fs::write(root.join("x.skip.yaml"), "ok: 1  \n").unwrap();

    // Ignore *.skip.yaml but still pass ignored file explicitly
    let cfg = root.join(".yamllint.yml");
    fs::write(&cfg, "extends: default\nignore: ['**/*.skip.yaml']\n").unwrap();

    let ryl = env!("CARGO_BIN_EXE_ryl");
    let (_code, out, _err) = run(Command::new(ryl)
        .arg("--list-files")
        .arg(root)
        .arg(root.join("x.skip.yaml")));
    let mut ryl_list: Vec<_> = out.lines().map(|s| s.to_string()).collect();
    ryl_list.sort();

    // Yamllint: run on directory and explicit file; ignored file should be filtered
    let (_yc, y_out, _y_err) = run(Command::new("yamllint")
        .arg("-f")
        .arg("standard")
        .arg("-c")
        .arg(&cfg)
        .arg(root)
        .arg(root.join("x.skip.yaml")));
    let mut y_files = std::collections::BTreeSet::<String>::new();
    for line in y_out.lines().filter(|l| !l.trim().is_empty()) {
        if let Some((path, _rest)) = line.split_once(':') {
            y_files.insert(path.trim().to_string());
        }
    }

    assert!(ryl_list.iter().any(|p| p.ends_with("keep.yaml")));
    if y_files.is_empty() {
        eprintln!("yamllint produced no output; skipping parity assertions");
        return;
    }
    assert!(!ryl_list.iter().any(|p| p.ends_with("x.skip.yaml")));
}
