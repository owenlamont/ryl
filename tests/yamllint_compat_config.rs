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
        "ignore-from-file: .ignore\nrules: {}\n",
    )
    .unwrap();
    fs::write(root.join(".ignore"), "*.skip.yaml\n").unwrap();
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
fn yamllint_config_yml_discovery_matches_yamllint() {
    ensure_yamllint_installed();

    let td = tempdir().unwrap();
    let root = td.path();

    fs::write(
        root.join("yamllint-config.yml"),
        "extends: default\nrules:\n  line-length:\n    max: 110\n",
    )
    .unwrap();
    fs::write(
        root.join("airflow.yaml"),
        "line: \"This string intentionally exceeds the default eighty character limit for yamllint\"\n",
    )
    .unwrap();

    let ryl = env!("CARGO_BIN_EXE_ryl");
    let (ryl_code, ryl_out, ryl_err) = run(Command::new(ryl).current_dir(root).arg("airflow.yaml"));
    let (yam_code, yam_out, yam_err) = run(Command::new("yamllint")
        .current_dir(root)
        .arg("airflow.yaml"));

    assert_eq!(
        ryl_code, yam_code,
        "exit mismatch\nryl: {ryl_out}{ryl_err}\nyamllint: {yam_out}{yam_err}"
    );
    assert_eq!(
        ryl_out, yam_out,
        "stdout mismatch\nryl: {ryl_out}\nyamllint: {yam_out}"
    );
    assert_eq!(
        ryl_err, yam_err,
        "stderr mismatch\nryl: {ryl_err}\nyamllint: {yam_err}"
    );
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
