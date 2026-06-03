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
fn config_file_ignores_docs_globally() {
    let td = tempdir().unwrap();
    let root = td.path();
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::write(root.join("a.yaml"), "a: 1\n").unwrap();
    fs::write(root.join("docs/ignored.yaml"), "x: 0\n").unwrap();

    // `--list-files` answers a file-selection query and is exempt from the
    // "no rules enabled" lint check, so a rule-less config is fine here.
    let cfg = root.join("cfg.yml");
    fs::write(&cfg, "ignore: ['docs/**']\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, out, err) = run(Command::new(exe)
        .arg("--list-files")
        .arg("-c")
        .arg(&cfg)
        .arg(root));
    assert_eq!(code, 0, "expected success: {err}");
    assert!(out.contains("a.yaml"));
    assert!(!out.contains("docs/ignored.yaml"));
}

#[test]
fn config_data_yaml_files_only_lists_dot_yamllint_yml() {
    let td = tempdir().unwrap();
    let root = td.path();
    fs::write(root.join("a.yaml"), "a: 1\n").unwrap();
    fs::write(root.join(".yamllint.yml"), "rules: {}\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, out, err) = run(Command::new(exe)
        .arg("--list-files")
        .arg("-d")
        .arg("yaml-files: ['**/.yamllint.yml']\n")
        .arg(root));
    assert_eq!(code, 0, "expected success: {err}");
    assert!(out.contains(".yamllint.yml"));
    assert!(!out.contains("a.yaml"));
}

#[test]
fn config_enabling_no_rules_is_rejected_loudly() {
    let td = tempdir().unwrap();
    let file = td.path().join("a.yaml");
    fs::write(&file, "a: 1\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    // A config that turns every rule off lints nothing; ryl rejects it (stricter than
    // yamllint, which silently accepts a rule-less config) rather than exit 0 silently.
    for data in ["rules: {}\n", "rules:\n  document-start: disable\n"] {
        let (code, _out, err) = run(Command::new(exe).arg("-d").arg(data).arg(&file));
        assert_eq!(code, 2, "a rule-less config must error: {err}");
        assert!(
            err.contains("enables no rules"),
            "expected the no-rules error: {err}"
        );
    }

    // `--list-files` is exempt — it answers a file query, it does not lint.
    let (code, _out, err) = run(Command::new(exe)
        .arg("--list-files")
        .arg("-d")
        .arg("rules: {}\n")
        .arg(&file));
    assert_eq!(
        code, 0,
        "--list-files must not require enabled rules: {err}"
    );
}

#[test]
fn format_strict_no_warnings_are_accepted() {
    let td = tempdir().unwrap();
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _out, err) = run(Command::new(exe)
        .arg("--list-files")
        .arg("-f")
        .arg("standard")
        .arg("-s")
        .arg("--no-warnings")
        .arg(td.path()));
    assert_eq!(code, 0, "expected success: {err}");
}
