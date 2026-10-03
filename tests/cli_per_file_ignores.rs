use std::fs;
use std::process::Command;

use tempfile::tempdir;

mod common;
use common::cli::{command_output, run, ryl};

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

const WORKFLOW_IGNORE_CONFIG: &str = "[rules]\ndocument-start = 'enable'\n\
     [per-file-ignores]\n'.github/workflows/*' = ['document-start']\n";

fn workflow_tree() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let workflows = dir.path().join(".github/workflows");
    fs::create_dir_all(&workflows).unwrap();
    fs::write(workflows.join("action.yml"), "on: push\n").unwrap();
    dir
}

#[test]
fn toml_per_file_ignores_match_walked_and_dot_prefixed_paths() {
    let dir = workflow_tree();
    fs::write(dir.path().join(".ryl.toml"), WORKFLOW_IGNORE_CONFIG).unwrap();

    for input in [
        ".",
        ".github",
        ".github/workflows/action.yml",
        "./.github/workflows/action.yml",
    ] {
        let (code, stdout, stderr) =
            run(ryl(dir.path()).current_dir(dir.path()).arg(input));
        assert_eq!(
            code,
            0,
            "`ryl {input}` should honour the per-file ignore: {}",
            command_output(&stdout, &stderr)
        );
    }
}

#[test]
fn toml_per_file_ignores_anchor_at_a_nested_discovered_config() {
    let dir = tempdir().unwrap();
    let svc = dir.path().join("svc");
    fs::create_dir_all(svc.join("workflows")).unwrap();
    fs::write(svc.join("workflows/action.yml"), "on: push\n").unwrap();
    fs::write(
        svc.join(".ryl.toml"),
        "[rules]\ndocument-start = 'enable'\n\
         [per-file-ignores]\n'workflows/*' = ['document-start']\n",
    )
    .unwrap();

    for input in [".", "svc", "./svc/workflows/action.yml"] {
        let (code, stdout, stderr) =
            run(ryl(dir.path()).current_dir(dir.path()).arg(input));
        assert_eq!(
            code,
            0,
            "`ryl {input}` should honour svc/.ryl.toml's per-file ignore: {}",
            command_output(&stdout, &stderr)
        );
    }
}

#[test]
fn toml_per_file_ignores_anchor_at_the_config_dir_from_a_subdirectory() {
    let dir = workflow_tree();
    fs::write(dir.path().join(".ryl.toml"), WORKFLOW_IGNORE_CONFIG).unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .current_dir(dir.path().join(".github"))
        .arg("-c")
        .arg("../.ryl.toml")
        .arg("workflows/action.yml"));
    assert_eq!(
        code,
        0,
        "a cwd below the config dir should still match: {}",
        command_output(&stdout, &stderr)
    );
}

#[test]
fn toml_per_file_ignores_match_a_path_outside_the_config_dir_as_given() {
    // Like `ignore`, a file outside the config dir is matched by the path as given.
    let dir = workflow_tree();
    let conf = dir.path().join("conf");
    fs::create_dir(&conf).unwrap();
    fs::write(conf.join("ryl.toml"), WORKFLOW_IGNORE_CONFIG).unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .current_dir(dir.path())
        .arg("-c")
        .arg("conf/ryl.toml")
        .arg("./.github/workflows/action.yml"));
    assert_eq!(
        code,
        0,
        "an out-of-tree path should match as given: {}",
        command_output(&stdout, &stderr)
    );
}

#[test]
fn toml_per_file_ignores_normalize_dot_segments_in_patterns() {
    let dir = workflow_tree();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ndocument-start = 'enable'\n\
         [per-file-ignores]\n'./.github/../.github/workflows/*' = ['document-start']\n",
    )
    .unwrap();

    for input in [".", ".github/workflows/action.yml"] {
        let (code, stdout, stderr) =
            run(ryl(dir.path()).current_dir(dir.path()).arg(input));
        assert_eq!(
            code,
            0,
            "`ryl {input}` should match a pattern with `.`/`..` segments: {}",
            command_output(&stdout, &stderr)
        );
    }
}
