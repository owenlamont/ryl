use std::fs;
use std::process::Command;

use tempfile::tempdir;

fn ensure_yamllint_installed() {
    let ok = Command::new("yamllint")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    assert!(
        ok,
        "yamllint must be installed and in PATH for parity tests"
    );
}

fn run(cmd: &mut Command) -> (i32, String, String) {
    let out = cmd.output().expect("failed to run command");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (code, stdout, stderr)
}

#[test]
fn yamllint_and_ryl_honor_ignore_from_file() {
    ensure_yamllint_installed();

    let td = tempdir().unwrap();
    let root = td.path();
    fs::write(
        root.join(".yamllint"),
        "ignore-from-file: patterns.ignore\nrules: {}\n",
    )
    .unwrap();
    fs::write(root.join("patterns.ignore"), "*.skip.yaml\n").unwrap();
    fs::write(root.join("keep.yaml"), "ok: 1\n").unwrap();
    fs::write(root.join("skip.yaml"), "bad: [1\n").unwrap();

    let ryl = env!("CARGO_BIN_EXE_ryl");
    let (_code, ryl_out, ryl_err) = run(Command::new(ryl)
        .current_dir(root)
        .arg("--list-files")
        .arg("."));
    assert!(ryl_err.is_empty(), "unexpected stderr from ryl: {ryl_err}");
    let mut ryl_list: Vec<_> = ryl_out.lines().map(|s| s.to_string()).collect();
    ryl_list.sort();

    let (_yc, y_out, y_err) = run(Command::new("yamllint")
        .current_dir(root)
        .arg("--list-files")
        .arg("."));
    assert!(y_err.is_empty(), "unexpected stderr from yamllint: {y_err}");
    let mut y_list: Vec<_> = y_out.lines().map(|s| s.to_string()).collect();
    y_list.sort();

    assert_eq!(ryl_list, y_list, "file lists should match");
}

#[test]
fn project_config_precedence_over_env_matches_yamllint() {
    ensure_yamllint_installed();

    let td = tempdir().unwrap();
    let root = td.path();
    fs::write(
        root.join(".yamllint"),
        "ignore: ['ignored.yaml']\nrules: {}\n",
    )
    .unwrap();
    let env_cfg = root.join("env.yaml");
    fs::write(&env_cfg, "rules: {}\n").unwrap();
    fs::write(root.join("ignored.yaml"), "ok: 1\n").unwrap();
    fs::write(root.join("keep.yaml"), "ok: 1\n").unwrap();

    let ryl = env!("CARGO_BIN_EXE_ryl");
    let (_code, ryl_out, ryl_err) = run(Command::new(ryl)
        .current_dir(root)
        .env("YAMLLINT_CONFIG_FILE", env_cfg.display().to_string())
        .arg("--list-files")
        .arg("."));
    assert!(ryl_err.is_empty(), "unexpected stderr from ryl: {ryl_err}");
    let mut ryl_list: Vec<_> = ryl_out.lines().map(|s| s.to_string()).collect();
    ryl_list.sort();

    let (_yc, y_out, y_err) = run(Command::new("yamllint")
        .current_dir(root)
        .env("YAMLLINT_CONFIG_FILE", env_cfg.display().to_string())
        .arg("--list-files")
        .arg("."));
    assert!(y_err.is_empty(), "unexpected stderr from yamllint: {y_err}");
    let mut y_list: Vec<_> = y_out.lines().map(|s| s.to_string()).collect();
    y_list.sort();

    assert_eq!(ryl_list, y_list, "project config should take precedence");
}

#[test]
fn ignore_can_reinclude_file_from_excluded_directory_matches_yamllint() {
    ensure_yamllint_installed();

    let td = tempdir().unwrap();
    let root = td.path();
    fs::write(
        root.join(".yamllint"),
        "ignore: |\n  build/\n  !build/keep.yaml\nrules: {}\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("build")).unwrap();
    fs::write(root.join("build/keep.yaml"), "ok: 1\n").unwrap();
    fs::write(root.join("build/drop.yaml"), "ok: 1\n").unwrap();
    fs::write(root.join("top.yaml"), "ok: 1\n").unwrap();

    let ryl = env!("CARGO_BIN_EXE_ryl");
    let (_code, ryl_out, ryl_err) = run(Command::new(ryl)
        .current_dir(root)
        .arg("--list-files")
        .arg("."));
    assert!(ryl_err.is_empty(), "unexpected stderr from ryl: {ryl_err}");
    let mut ryl_list: Vec<_> = ryl_out.lines().map(|s| s.to_string()).collect();
    ryl_list.sort();

    let (_yc, y_out, y_err) = run(Command::new("yamllint")
        .current_dir(root)
        .arg("--list-files")
        .arg("."));
    assert!(y_err.is_empty(), "unexpected stderr from yamllint: {y_err}");
    let mut y_list: Vec<_> = y_out.lines().map(|s| s.to_string()).collect();
    y_list.sort();

    assert_eq!(ryl_list, y_list, "file lists should match");
}

#[test]
fn ignore_from_file_can_reinclude_file_from_excluded_directory_matches_yamllint() {
    ensure_yamllint_installed();

    let td = tempdir().unwrap();
    let root = td.path();
    fs::write(
        root.join(".yamllint"),
        "ignore-from-file: patterns.ignore\nrules: {}\n",
    )
    .unwrap();
    fs::write(root.join("patterns.ignore"), "build/\n!build/keep.yaml\n").unwrap();
    fs::create_dir_all(root.join("build")).unwrap();
    fs::write(root.join("build/keep.yaml"), "ok: 1\n").unwrap();
    fs::write(root.join("build/drop.yaml"), "ok: 1\n").unwrap();
    fs::write(root.join("top.yaml"), "ok: 1\n").unwrap();

    let ryl = env!("CARGO_BIN_EXE_ryl");
    let (_code, ryl_out, ryl_err) = run(Command::new(ryl)
        .current_dir(root)
        .arg("--list-files")
        .arg("."));
    assert!(ryl_err.is_empty(), "unexpected stderr from ryl: {ryl_err}");
    let mut ryl_list: Vec<_> = ryl_out.lines().map(|s| s.to_string()).collect();
    ryl_list.sort();

    let (_yc, y_out, y_err) = run(Command::new("yamllint")
        .current_dir(root)
        .arg("--list-files")
        .arg("."));
    assert!(y_err.is_empty(), "unexpected stderr from yamllint: {y_err}");
    let mut y_list: Vec<_> = y_out.lines().map(|s| s.to_string()).collect();
    y_list.sort();

    assert_eq!(ryl_list, y_list, "file lists should match");
}
