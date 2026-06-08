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

mod common;
use common::cli::run;

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
    let (code, _out, err) = run(Command::new(exe)
        .arg("-d")
        .arg("rules:\n  key-duplicates: enable\n")
        .arg(&target));
    assert!(
        code == 0 || code == 1,
        "linting a billion-laughs file should finish cleanly (no DOM built), got {code}: {err}"
    );
    assert!(
        !err.contains("too many alias expansions"),
        "the lint path must never reach the config alias-expansion guard: {err}"
    );
}

#[test]
fn merging_a_wide_base_into_many_hosts_stays_bounded() {
    // Many distinct anchored hosts each merging a wide base would grow
    // contributions as hosts×keys (issue #252 review); the per-file budget caps
    // the materialisation so this stays bounded instead of exhausting memory.
    let dir = tempdir().unwrap();
    let target = dir.path().join("hk.yaml");
    let mut doc = String::from("base: &base\n");
    for i in 0..1100 {
        doc.push_str(&format!("  k{i}: {i}\n"));
    }
    for h in 0..1000 {
        doc.push_str(&format!("h{h}: &h{h} {{<<: *base}}\n"));
    }
    // A trailing sequence merge once the budget is spent exercises the
    // sequence-merge path under exhaustion too.
    doc.push_str("tail:\n  <<: [*base]\n");
    fs::write(&target, doc).unwrap();
    let config = dir.path().join("ryl.toml");
    fs::write(&config, "[rules.key-duplicates]\ncheck-canonical = true\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _out, err) = run(Command::new(exe).arg("-c").arg(&config).arg(&target));
    assert!(
        code == 0 || code == 1,
        "hosts×keys materialisation must stay bounded, got {code}: {err}"
    );
}

#[test]
fn deeply_nested_anchored_merges_stay_bounded() {
    // Nested anchored inline merge wrappers (`<<: &w1 {<<: &w2 {... <<: *base}}`)
    // each re-materialise the base's keys; without charging that per-level
    // materialisation this is quadratic in depth×keys (issue #252 review).
    let dir = tempdir().unwrap();
    let target = dir.path().join("nest.yaml");
    let mut doc = String::from("base: &base\n");
    for i in 0..400 {
        doc.push_str(&format!("  k{i}: {i}\n"));
    }
    doc.push_str("host:\n");
    let mut indent = String::from("  ");
    for d in 0..200 {
        doc.push_str(&format!("{indent}<<: &w{d}\n"));
        indent.push_str("  ");
    }
    doc.push_str(&format!("{indent}<<: *base\n"));
    fs::write(&target, doc).unwrap();
    let config = dir.path().join("ryl.toml");
    fs::write(&config, "[rules.key-duplicates]\ncheck-canonical = true\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _out, err) = run(Command::new(exe).arg("-c").arg(&config).arg(&target));
    assert!(
        code == 0 || code == 1,
        "nested anchored merges must stay bounded, got {code}: {err}"
    );
}

#[test]
fn merging_a_wide_anchor_many_times_stays_bounded() {
    // `<<: [*base, *base, ...]` with a wide base must not materialise
    // keys×aliases contributions (issue #252 review): each anchor merges into a
    // host at most once. A blow-up would make this quadratic in keys×aliases.
    let dir = tempdir().unwrap();
    let target = dir.path().join("wide.yaml");
    let mut doc = String::from("base: &base\n");
    for i in 0..200 {
        doc.push_str(&format!("  k{i}: {i}\n"));
    }
    let refs = vec!["*base".to_string(); 64].join(", ");
    doc.push_str(&format!("host:\n  <<: [{refs}]\n"));
    fs::write(&target, doc).unwrap();
    let config = dir.path().join("ryl.toml");
    fs::write(&config, "[rules.key-duplicates]\ncheck-canonical = true\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _out, err) = run(Command::new(exe).arg("-c").arg(&config).arg(&target));
    assert!(
        code == 0 || code == 1,
        "repeated wide merges must finish cleanly, got {code}: {err}"
    );
}

#[test]
fn linting_alias_bomb_with_check_canonical_stays_bounded() {
    // `key-duplicates: check-canonical` resolves alias values to compare merged
    // keys; it must fold each node to a bounded hash rather than materialise the
    // 9^10 alias expansion (issue #252 review). A blow-up would hang/OOM here.
    let dir = tempdir().unwrap();
    let target = dir.path().join("bomb.yaml");
    fs::write(&target, alias_bomb()).unwrap();
    let config = dir.path().join("ryl.toml");
    fs::write(&config, "[rules.key-duplicates]\ncheck-canonical = true\n").unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _out, err) = run(Command::new(exe).arg("-c").arg(&config).arg(&target));
    assert!(
        code == 0 || code == 1,
        "the value-aware lint path must finish cleanly on a billion-laughs file, got {code}: {err}"
    );
}
