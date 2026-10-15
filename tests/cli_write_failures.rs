use std::fs::{self, OpenOptions};
use std::path::Path;

use tempfile::tempdir;

mod common;
use common::cli::{run, ryl};

fn make_read_only(path: &Path) -> bool {
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(path, permissions).unwrap();
    OpenOptions::new().write(true).open(path).is_err()
}

#[test]
#[allow(clippy::permissions_set_readonly_false)]
fn write_failure_keeps_processing_explicit_yaml_inputs() {
    for command in ["format", "check"] {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("bad.yaml"), "a: [\n").unwrap();
        let failed = dir.path().join("failed.yaml");
        fs::write(&failed, "key: value").unwrap();
        let later = dir.path().join("later.yaml");
        fs::write(&later, "key: value").unwrap();
        if !make_read_only(&failed) {
            continue;
        }

        let mut cmd = ryl(dir.path());
        cmd.arg(command);
        if command == "check" {
            cmd.arg("--fix")
                .args(["-d", "[lint.rules]\nnew-line-at-end-of-file = 'enable'\n"]);
        }
        let (code, _stdout, stderr) =
            run(cmd.args([dir.path().join("bad.yaml"), failed.clone(), later.clone()]));

        let mut permissions = fs::metadata(&failed).unwrap().permissions();
        permissions.set_readonly(false);
        fs::set_permissions(&failed, permissions).unwrap();

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
            assert!(
                stderr.contains("1 fixed, 2 remaining"),
                "failed file must not count as fixed: {stderr}"
            );
        }
    }
}

#[test]
#[allow(clippy::permissions_set_readonly_false)]
fn write_failure_keeps_processing_directory_markdown_inputs() {
    for command in ["format", "check"] {
        let dir = tempdir().unwrap();
        let failed = dir.path().join("failed.md");
        let later = dir.path().join("later.md");
        let body = "```yaml\nnums: [1,2]\n```\n";
        fs::write(&failed, body).unwrap();
        fs::write(&later, body).unwrap();
        if !make_read_only(&failed) {
            continue;
        }

        let mut cmd = ryl(dir.path());
        cmd.arg(command).arg("--markdown");
        if command == "check" {
            cmd.arg("--fix")
                .args(["-d", "[lint.rules]\ncommas = 'enable'\n"]);
        }
        let (code, _stdout, stderr) = run(cmd.arg(dir.path()));

        let mut permissions = fs::metadata(&failed).unwrap().permissions();
        permissions.set_readonly(false);
        fs::set_permissions(&failed, permissions).unwrap();

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
