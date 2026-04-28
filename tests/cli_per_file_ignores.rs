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

#[test]
fn toml_per_file_ignores_combine_matching_patterns() {
    let dir = tempdir().unwrap();
    let values = dir.path().join("values.yaml");
    let manifest = dir.path().join("manifest.yaml");
    fs::write(&values, "flag: yes\n").unwrap();
    fs::write(&manifest, "flag: yes\n").unwrap();

    let config = dir.path().join(".ryl.toml");
    fs::write(
        &config,
        r#"[rules]
document-start = "enable"
truthy = "enable"

[per-file-ignores]
"**/values.yaml" = ["document-start"]
"*.yaml" = ["truthy"]
"#,
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("-c")
        .arg(&config)
        .arg(&values)
        .arg(&manifest));
    assert_eq!(
        code, 1,
        "expected one error: stdout={stdout} stderr={stderr}"
    );
    let output = if stderr.is_empty() { stdout } else { stderr };
    assert!(
        output.contains("manifest.yaml")
            && output.contains("missing document start")
            && output.contains("document-start"),
        "manifest should keep document-start diagnostic: {output}"
    );
    assert!(
        !output.contains("values.yaml") && !output.contains("truthy value"),
        "values document-start and all truthy diagnostics should be ignored: {output}"
    );
}

#[test]
fn toml_per_file_ignores_support_ruff_negated_patterns() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("src");
    fs::create_dir(&src).unwrap();
    let outside = dir.path().join("outside.yaml");
    let inside = src.join("inside.yaml");
    fs::write(&outside, "name: value\n").unwrap();
    fs::write(&inside, "name: value\n").unwrap();

    let config = dir.path().join(".ryl.toml");
    fs::write(
        &config,
        r#"[rules]
document-start = "enable"

[per-file-ignores]
"!src/**.yaml" = ["document-start"]
"#,
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) =
        run(Command::new(exe).arg("-c").arg(&config).arg(dir.path()));
    assert_eq!(
        code, 1,
        "expected one error: stdout={stdout} stderr={stderr}"
    );
    let output = if stderr.is_empty() { stdout } else { stderr };
    assert!(
        output.contains("inside.yaml") && output.contains("document-start"),
        "src file should not be ignored by negated pattern: {output}"
    );
    assert!(
        !output.contains("outside.yaml"),
        "outside file should be ignored by negated pattern: {output}"
    );
}

#[test]
fn toml_per_file_ignores_match_absolute_patterns() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("absolute.yaml");
    fs::write(&file, "name: value\n").unwrap();

    let config = dir.path().join(".ryl.toml");
    fs::write(
        &config,
        format!(
            "[rules]\ndocument-start = 'enable'\n[per-file-ignores]\n'{}' = ['document-start']\n",
            file.display()
        ),
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) =
        run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    assert_eq!(
        code, 0,
        "absolute per-file ignore should pass: stdout={stdout} stderr={stderr}"
    );
}

#[test]
fn toml_per_file_ignores_match_relative_cli_paths() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("relative.yaml");
    fs::write(&file, "name: value\n").unwrap();

    let config = dir.path().join(".ryl.toml");
    fs::write(
        &config,
        "[rules]\ndocument-start = 'enable'\n[per-file-ignores]\n'relative.yaml' = ['document-start']\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .current_dir(dir.path())
        .arg("-c")
        .arg(".ryl.toml")
        .arg("relative.yaml"));
    assert_eq!(
        code, 0,
        "relative per-file ignore should pass: stdout={stdout} stderr={stderr}"
    );
}
