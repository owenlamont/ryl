//! No configuration found is a loud error (#248).
//!
//! ryl has no default-on rules, so a run that resolves no configuration at all
//! enables nothing and must exit 2 with a message that names the `extends: default`
//! escape hatch — rather than silently linting with a preset (yamllint's behaviour)
//! or silently passing. This is distinct from the "configuration enables no rules"
//! error, which fires when a config *is* present but turns everything off.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use tempfile::tempdir;

/// Build a command whose `HOME`/`XDG_CONFIG_HOME` point at an isolated empty dir and
/// whose env carries no `YAMLLINT_CONFIG_FILE`, so neither a user-global nor an
/// env-var config can be discovered and the run genuinely sees no configuration.
fn isolated(home: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ryl"));
    cmd.current_dir(home)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join("xdg"))
        .env_remove("YAMLLINT_CONFIG_FILE");
    cmd
}

#[test]
fn no_config_found_is_rejected_with_escape_hatch() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("f.yaml");
    std::fs::write(&file, "key: value\n").unwrap();

    let out = isolated(dir.path()).arg(&file).output().expect("process");
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(2),
        "a run with no configuration must be a usage error: {err}"
    );
    assert!(
        err.contains("no configuration found") && err.contains("extends: default"),
        "expected the no-config error naming the escape hatch: {err}"
    );
}

#[test]
fn no_config_found_via_stdin_is_rejected() {
    let dir = tempdir().unwrap();
    let mut child = isolated(dir.path())
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"key: value\n")
        .expect("write stdin");
    let out = child.wait_with_output().expect("wait");
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(2),
        "stdin with no configuration must be a usage error: {err}"
    );
    assert!(
        err.contains("no configuration found"),
        "stdin must report the same no-config error: {err}"
    );
}

#[test]
fn mixed_run_reports_no_config_for_the_unconfigured_file() {
    // One file resolves a config that enables a rule; another resolves no config at
    // all. The run can't lint the unconfigured file, and the error must reflect THAT
    // file ("no configuration found") rather than the other file's config — the
    // message is chosen per offending file, not per run.
    let configured = tempdir().unwrap();
    std::fs::write(
        configured.path().join(".ryl.toml"),
        "[rules]\nanchors = \"enable\"\n",
    )
    .unwrap();
    let with_config = configured.path().join("a.yaml");
    std::fs::write(&with_config, "key: value\n").unwrap();

    let bare = tempdir().unwrap();
    let without_config = bare.path().join("b.yaml");
    std::fs::write(&without_config, "key: value\n").unwrap();

    let out = isolated(bare.path())
        .arg(&with_config)
        .arg(&without_config)
        .output()
        .expect("process");
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(2),
        "a file that resolves no config must fail the run: {err}"
    );
    assert!(
        err.contains("no configuration found"),
        "the unconfigured file's message must win over the configured file: {err}"
    );
}
