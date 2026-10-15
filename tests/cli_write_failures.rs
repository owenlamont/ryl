use std::fs::{self, OpenOptions, Permissions};
use std::path::Path;

use tempfile::tempdir;

mod common;
use common::cli::{run, ryl};

fn make_read_only(path: &Path) -> Option<Permissions> {
    let original = fs::metadata(path).unwrap().permissions();
    let mut permissions = original.clone();
    permissions.set_readonly(true);
    fs::set_permissions(path, permissions).unwrap();
    if OpenOptions::new().write(true).open(path).is_ok() {
        fs::set_permissions(path, original).unwrap();
        None
    } else {
        Some(original)
    }
}

#[test]
fn write_failure_keeps_processing_explicit_yaml_inputs() {
    for command in ["format", "check"] {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("bad.yaml"), "a: [\n").unwrap();
        let failed = dir.path().join("failed.yaml");
        fs::write(&failed, "key: value").unwrap();
        let later = dir.path().join("later.yaml");
        fs::write(&later, "key: value").unwrap();
        let Some(original_permissions) = make_read_only(&failed) else {
            continue;
        };

        let mut cmd = ryl(dir.path());
        cmd.arg(command);
        if command == "check" {
            cmd.arg("--fix")
                .args(["-d", "[lint.rules]\nnew-line-at-end-of-file = 'enable'\n"]);
        }
        let (code, _stdout, stderr) =
            run(cmd.args([dir.path().join("bad.yaml"), failed.clone(), later.clone()]));

        fs::set_permissions(&failed, original_permissions).unwrap();

        assert_eq!(code, 2, "{command}: {stderr}");
        assert!(stderr.contains("skipped by"), "{command}: {stderr}");
        assert!(
            stderr.contains("failed to write fixed file"),
            "{command}: {stderr}"
        );
        assert!(stderr.contains("failed.yaml"), "{command}: {stderr}");
        assert_eq!(fs::read_to_string(&failed).unwrap(), "key: value");
        assert_eq!(fs::read_to_string(&later).unwrap(), "key: value\n");
        if command == "check" {
            assert_eq!(
                stderr.matches("error: failed to write fixed file").count(),
                1,
                "{stderr}"
            );
            assert!(
                stderr.contains("1 fixed, 2 remaining"),
                "failed file must not count as fixed: {stderr}"
            );
        }
    }
}

#[test]
fn write_failure_keeps_processing_directory_markdown_inputs() {
    for command in ["format", "check"] {
        let dir = tempdir().unwrap();
        let failed = dir.path().join("failed.md");
        let later = dir.path().join("later.md");
        let body = "```yaml\nnums: [1,2]\n```\n";
        fs::write(&failed, body).unwrap();
        fs::write(&later, body).unwrap();
        let Some(original_permissions) = make_read_only(&failed) else {
            continue;
        };

        let mut cmd = ryl(dir.path());
        cmd.arg(command).arg("--markdown");
        if command == "check" {
            cmd.arg("--fix")
                .args(["-d", "[lint.rules]\ncommas = 'enable'\n"]);
        }
        let (code, _stdout, stderr) = run(cmd.arg(dir.path()));

        fs::set_permissions(&failed, original_permissions).unwrap();

        assert_eq!(code, 2, "{command}: {stderr}");
        assert!(stderr.contains("failed.md"), "{command}: {stderr}");
        assert_eq!(fs::read_to_string(&failed).unwrap(), body);
        assert!(
            fs::read_to_string(&later).unwrap().contains("[1, 2]"),
            "{command} should rewrite the other Markdown file"
        );
        if command == "check" {
            assert!(stderr.contains("1 fixed, 1 remaining"), "{stderr}");
        }
    }
}

const WARNING_FIX: &str =
    "[lint.rules]\nnew-line-at-end-of-file = { level = 'warning' }\n";

#[test]
fn junit_reports_failed_fix_write_with_warnings_hidden() {
    let dir = tempdir().unwrap();
    let failed = dir.path().join("failed.yaml");
    fs::write(&failed, "key: value").unwrap();
    let Some(original_permissions) = make_read_only(&failed) else {
        return;
    };

    let (code, xml, stderr) = run(ryl(dir.path())
        .args([
            "check",
            "--fix",
            "--no-warnings",
            "-d",
            WARNING_FIX,
            "-f",
            "junit",
        ])
        .arg(&failed));
    fs::set_permissions(&failed, original_permissions).unwrap();

    assert_eq!(code, 2, "{xml}");
    assert!(xml.contains("errors=\"1\""), "{xml}");
    assert!(xml.contains("<error "), "{xml}");
    assert!(xml.contains("failed to write fixed file"), "{xml}");
    assert!(xml.contains("failed.yaml"), "{xml}");
    assert!(
        stderr.contains("error: failed to write fixed file")
            && stderr.contains("failed.yaml"),
        "{stderr}"
    );
}

#[test]
fn gitlab_reports_failed_fix_write_with_warnings_hidden() {
    for configured_target in [false, true] {
        let dir = tempdir().unwrap();
        let failed = dir.path().join("failed.yaml");
        fs::write(&failed, "key: value").unwrap();
        fs::write(
            dir.path().join(".ryl.toml"),
            format!("{WARNING_FIX}[output.gitlab]\npath = 'gl.json'\n"),
        )
        .unwrap();
        let Some(original_permissions) = make_read_only(&failed) else {
            continue;
        };

        let mut cmd = ryl(dir.path());
        cmd.current_dir(dir.path())
            .args(["check", "--fix", "--no-warnings"]);
        if !configured_target {
            cmd.args(["-f", "gitlab"]);
        }
        let (code, stdout, stderr) = run(cmd.arg(&failed));
        fs::set_permissions(&failed, original_permissions).unwrap();

        let report = if configured_target {
            fs::read_to_string(dir.path().join("gl.json")).unwrap()
        } else {
            stdout
        };
        let issues: serde_json::Value = serde_json::from_str(&report).unwrap();
        let issues = issues.as_array().unwrap();
        assert_eq!(code, 2, "{report}");
        assert!(
            stderr.contains("error: failed to write fixed file")
                && stderr.contains("failed.yaml"),
            "{stderr}"
        );
        assert_eq!(issues.len(), 1, "{report}");
        assert_eq!(issues[0]["severity"], "blocker");
        assert_eq!(issues[0]["check_name"], "error");
        assert!(
            issues[0]["description"]
                .as_str()
                .unwrap()
                .contains("failed to write fixed file")
        );
        assert!(
            issues[0]["location"]["path"]
                .as_str()
                .unwrap()
                .ends_with("failed.yaml")
        );
    }
}

#[test]
fn junit_and_gitlab_report_failed_fix_reads() {
    let dir = tempdir().unwrap();
    let missing = dir.path().join("missing.yaml");
    for format in ["junit", "gitlab"] {
        let (code, report, stderr) = run(ryl(dir.path())
            .args([
                "check",
                "--fix",
                "--no-warnings",
                "-d",
                WARNING_FIX,
                "-f",
                format,
            ])
            .arg(&missing));
        assert_eq!(code, 2, "{format}: {report}");
        assert!(report.contains("failed to read"), "{format}: {report}");
        assert!(report.contains("missing.yaml"), "{format}: {report}");
        assert!(
            stderr.contains("error: failed to read") && stderr.contains("missing.yaml"),
            "{format}: {stderr}"
        );
    }
}
