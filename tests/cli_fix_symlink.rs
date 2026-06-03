//! `--fix` must never write through a symlink.
//!
//! `std::fs::write` follows symlinks, so without a guard an untrusted tree could
//! ship `innocent.yaml -> ~/.bashrc` and have `ryl --fix` rewrite a file outside
//! the tree. These pin that `--fix` skips symlinks (with a warning) and leaves the
//! target byte-for-byte unchanged, for the explicit-arg, directory-walk, and
//! embedded-markdown paths. Unix-only: symlink creation needs privileges on
//! Windows, and CI's coverage gate runs on Linux.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::symlink;
use std::process::Command;

use tempfile::tempdir;

fn run(cmd: &mut Command) -> (i32, String, String) {
    let out = cmd.output().expect("process");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (code, stdout, stderr)
}

const FIXABLE_YAML: &str = "secret: topsecret   \n";

#[test]
fn fix_symlink_warning_sanitizes_the_path() {
    // The skip warning embeds the (user-controlled) path; a newline in a symlink's
    // name must not break out of the warning line into a forged workflow command.
    let dir = tempdir().unwrap();
    let secret = dir.path().join("secret.yaml");
    fs::write(&secret, FIXABLE_YAML).unwrap();
    let link = dir.path().join("evil\n::error::INJECT.yaml");
    symlink(&secret, &link).unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (_code, _out, err) = run(Command::new(exe).arg("--fix").arg(&link));
    assert!(
        err.contains("refusing to apply --fix through a symlink"),
        "expected the symlink skip warning: {err:?}"
    );
    assert!(
        !err.contains("\n::error::INJECT"),
        "the warning's path must be sanitized, not injected: {err:?}"
    );
    assert_eq!(
        fs::read_to_string(&secret).unwrap(),
        FIXABLE_YAML,
        "the symlink target must be left unchanged"
    );
}

#[test]
fn fix_skips_explicit_symlink_arg_and_leaves_target_untouched() {
    let dir = tempdir().unwrap();
    let secret = dir.path().join("secret.yaml");
    fs::write(&secret, FIXABLE_YAML).unwrap();
    let link = dir.path().join("innocent.yaml");
    symlink(&secret, &link).unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (_code, _out, err) = run(Command::new(exe).arg("--fix").arg(&link));
    assert!(
        err.contains("refusing to apply --fix through a symlink"),
        "expected the symlink skip warning: {err}"
    );
    assert_eq!(
        fs::read_to_string(&secret).unwrap(),
        FIXABLE_YAML,
        "the symlink target must be left byte-for-byte unchanged"
    );
}

#[test]
fn fix_skips_symlink_pointing_outside_the_scanned_directory() {
    let dir = tempdir().unwrap();
    let secret = dir.path().join("secret.yaml");
    fs::write(&secret, FIXABLE_YAML).unwrap();
    let repo = dir.path().join("repo");
    fs::create_dir(&repo).unwrap();
    symlink(&secret, repo.join("innocent.yaml")).unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (_code, _out, err) = run(Command::new(exe).arg("--fix").arg(&repo));
    assert!(
        err.contains("refusing to apply --fix through a symlink"),
        "directory-walk --fix must skip symlinks: {err}"
    );
    assert_eq!(
        fs::read_to_string(&secret).unwrap(),
        FIXABLE_YAML,
        "dir-walk --fix must not reach a file outside the tree via a symlink"
    );
}

#[test]
fn fix_skips_symlinked_markdown_target() {
    let dir = tempdir().unwrap();
    let secret = dir.path().join("secret.md");
    let original = "```yaml\nk: v   \n```\n";
    fs::write(&secret, original).unwrap();
    let link = dir.path().join("innocent.md");
    symlink(&secret, &link).unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (_code, _out, err) =
        run(Command::new(exe).arg("--fix").arg("--markdown").arg(&link));
    assert!(
        err.contains("refusing to apply --fix through a symlink"),
        "embedded-markdown --fix must skip symlinks: {err}"
    );
    assert_eq!(
        fs::read_to_string(&secret).unwrap(),
        original,
        "the symlinked markdown target must be left byte-for-byte unchanged"
    );
}
