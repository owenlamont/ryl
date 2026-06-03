//! Billion-laughs (YAML alias-expansion) regression guards.
//!
//! `alias_bomb` is the classic ~470-byte payload whose nested anchors expand to
//! 9^10 (~3.5 billion) nodes if a loader resolves aliases by cloning. Two
//! invariants are pinned, one per code path:
//! - config parsing (which *does* build the alias-expanded DOM) must bound the
//!   expansion and reject the payload instead of exhausting memory;
//! - the lint path (which streams events to a no-op sink and never materialises
//!   the DOM) must stay immune, so a future change that starts building a DOM for
//!   linted content cannot silently reintroduce the vulnerability.
//!
//! A regression in either direction makes the offending test hang/OOM rather than
//! pass, so these stay meaningful even though they assert on a fast, bounded run.

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

fn alias_bomb() -> String {
    let mut payload = String::from("a0: &a0 \"lol\"\n");
    for level in 1..=9 {
        let refs = vec![format!("*a{}", level - 1); 9].join(",");
        payload.push_str(&format!("a{level}: &a{level} [{refs}]\n"));
    }
    let top = vec!["*a9".to_string(); 9].join(",");
    payload.push_str(&format!("boom: [{top}]\n"));
    payload
}

#[test]
fn config_alias_bomb_is_rejected_instead_of_expanded() {
    let dir = tempdir().unwrap();
    let target = dir.path().join("file.yaml");
    fs::write(&target, "key: value\n").unwrap();
    let config = dir.path().join("bomb.yaml");
    fs::write(&config, alias_bomb()).unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _out, err) = run(Command::new(exe).arg("-c").arg(&config).arg(&target));
    assert_eq!(
        code, 2,
        "billion-laughs config must be rejected with a usage exit: {err}"
    );
    assert!(
        err.contains("too many alias expansions"),
        "expected the billion-laughs guard message: {err}"
    );
}

#[test]
fn linting_alias_bomb_file_does_not_expand_aliases() {
    let dir = tempdir().unwrap();
    let target = dir.path().join("bomb.yaml");
    fs::write(&target, alias_bomb()).unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _out, err) = run(Command::new(exe).arg(&target));
    assert!(
        code == 0 || code == 1,
        "linting a billion-laughs file should finish cleanly (no DOM built), got {code}: {err}"
    );
    assert!(
        !err.contains("too many alias expansions"),
        "the lint path must never reach the config alias-expansion guard: {err}"
    );
}
