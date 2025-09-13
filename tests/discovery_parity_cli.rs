use std::fs;
use std::process::Command;

use tempfile::tempdir;

fn run(cmd: &mut Command) -> (i32, String, String) {
    let out = cmd.output().expect("failed to spawn");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (code, stdout, stderr)
}

#[test]
fn per_file_config_applies_when_no_global_config() {
    let td = tempdir().unwrap();
    let root = td.path();
    let sub = root.join("sub");
    fs::create_dir_all(&sub).unwrap();

    // Files
    fs::write(root.join("a.yaml"), "a: 1\n").unwrap();
    fs::write(sub.join("x.yaml"), "x: 1\n").unwrap();
    fs::write(sub.join("ignored.yaml"), "y: 1\n").unwrap();

    // Subdir config ignoring a specific file
    fs::write(sub.join(".yamllint"), "ignore: ['ignored.yaml']\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, out, err) = run(Command::new(exe).arg("--list-files").arg(root));
    assert_eq!(code, 0, "expected success: {err}");
    let mut lines: Vec<_> = out.lines().collect();
    lines.sort_unstable();

    let expect_a = root.join("a.yaml").display().to_string();
    let expect_x = sub.join("x.yaml").display().to_string();
    let expect_ignored = sub.join("ignored.yaml").display().to_string();

    assert!(lines.contains(&expect_a.as_str()));
    assert!(lines.contains(&expect_x.as_str()));
    assert!(!lines.contains(&expect_ignored.as_str()));
}

#[test]
fn env_global_config_overrides_project_and_explicit_file_filtered() {
    let td = tempdir().unwrap();
    let root = td.path();
    let docs = root.join("docs");
    fs::create_dir_all(&docs).unwrap();

    fs::write(root.join("a.yaml"), "a: 1\n").unwrap();
    fs::write(docs.join("b.yaml"), "b: 1\n").unwrap();

    // Project config does not ignore docs
    fs::write(root.join(".yamllint.yml"), "ignore: []\n").unwrap();

    // Global config file ignores docs/**
    let global_cfg = td.path().join("yamllint-global.yaml");
    fs::write(&global_cfg, "ignore: ['docs/**']\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, out, err) = run(Command::new(exe)
        .env("YAMLLINT_CONFIG_FILE", &global_cfg)
        .arg("--list-files")
        .arg(root)
        .arg(docs.join("b.yaml")));
    assert_eq!(code, 0, "expected success: {err}");
    let mut files: Vec<_> = out.lines().collect();
    files.sort_unstable();

    // With global ignore, directory scan excludes docs/b.yaml; explicit is filtered (parity)
    assert!(files.iter().any(|p| p.ends_with("a.yaml")));
    assert!(
        !files.iter().any(|p| p.ends_with("docs/b.yaml")),
        "explicit ignored file should be filtered"
    );
}
