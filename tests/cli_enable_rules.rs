use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};

use ryl::config::YamlLintConfig;
use ryl::rules::ALL_RULE_IDS;
use tempfile::tempdir;

mod common;
use common::cli::{command_output, run, ryl};

const SAMPLE: &str = "on: yes\nb: 1\na: 2   \n";

#[test]
fn all_enables_every_rule_at_error_level() {
    let cfg = YamlLintConfig::from_toml_str("[rules]\nALL = \"enable\"\n").unwrap();
    let mut names: Vec<&str> = cfg.rule_names().iter().map(String::as_str).collect();
    names.sort_unstable();
    let mut expected = ALL_RULE_IDS.to_vec();
    expected.sort_unstable();
    assert_eq!(names, expected);
    assert!(
        ALL_RULE_IDS
            .iter()
            .all(|id| cfg.rule_level(id) == Some(ryl::config_schema::RuleLevel::Error))
    );
}

#[test]
fn all_disable_is_stripped_and_enables_nothing() {
    let cfg = YamlLintConfig::from_toml_str("[rules]\nALL = \"disable\"\n").unwrap();
    assert!(!cfg.enables_any_rule());
    let cfg = YamlLintConfig::from_toml_str(
        "[rules]\nALL = \"disable\"\ntruthy = \"enable\"\n",
    )
    .unwrap();
    assert_eq!(cfg.rule_names(), ["truthy"]);
}

#[test]
fn explicit_rule_entries_win_over_all_in_either_order() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("a.yaml");
    fs::write(&file, SAMPLE).unwrap();
    let config = dir.path().join(".ryl.toml");
    fs::write(
        &config,
        "[rules]\ntruthy = \"disable\"\nALL = \"enable\"\n[rules.line-length]\nmax = 5\n",
    )
    .unwrap();
    let (code, stdout, stderr) = run(ryl(dir.path()).arg("check").arg(&file));
    let out = command_output(&stdout, &stderr);
    assert_eq!(code, 1, "{out}");
    assert!(
        out.contains("key-ordering") && out.contains("document-start"),
        "{out}"
    );
    assert!(
        out.contains("3:6") && out.contains("line-length"),
        "max = 5 kept: {out}"
    );
    assert!(!out.contains("truthy"), "explicit disable wins: {out}");
}

#[test]
fn enable_flag_lints_without_a_config() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("a.yaml");
    fs::write(&file, SAMPLE).unwrap();

    let (code, _, stderr) = run(ryl(dir.path()).arg("check").arg(&file));
    assert_eq!(code, 2, "no config and no flag still exits 2: {stderr}");

    let (code, stdout, stderr) = run(ryl(dir.path())
        .args(["check", "--enable", "ALL"])
        .arg(&file));
    let out = command_output(&stdout, &stderr);
    assert_eq!(code, 1, "{out}");
    assert!(
        out.contains("truthy") && out.contains("trailing-spaces"),
        "{out}"
    );

    let (code, stdout, stderr) = run(ryl(dir.path())
        .args(["check", "--enable", "truthy"])
        .arg(&file));
    let out = command_output(&stdout, &stderr);
    assert_eq!(code, 1, "{out}");
    assert!(
        out.contains("truthy") && !out.contains("trailing-spaces"),
        "{out}"
    );
}

#[test]
fn enable_flag_replaces_a_discovered_configs_selection() {
    let dir = tempdir().unwrap();
    let sub = dir.path().join("sub");
    fs::create_dir(&sub).unwrap();
    let file = sub.join("a.yaml");
    fs::write(&file, SAMPLE).unwrap();
    fs::write(
        sub.join(".ryl.toml"),
        "[rules]\ncolons = \"enable\"\ntruthy = \"disable\"\nkey-ordering = \"enable\"\n\
         [rules.line-length]\nmax = 5\n",
    )
    .unwrap();
    let (code, stdout, stderr) = run(ryl(dir.path())
        .args(["check", "--enable", "line-length", "--enable", "truthy"])
        .arg(dir.path()));
    let out = command_output(&stdout, &stderr);
    assert_eq!(code, 1, "{out}");
    assert!(
        out.contains("3:6") && out.contains("line-length"),
        "options kept: {out}"
    );
    assert!(
        out.contains("truthy"),
        "flag overrides a config disable: {out}"
    );
    assert!(
        !out.contains("key-ordering"),
        "unlisted rules are dropped: {out}"
    );
}

#[test]
fn enable_flag_restricts_inline_config_data() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("a.yaml");
    fs::write(&file, SAMPLE).unwrap();
    let (code, stdout, stderr) = run(ryl(dir.path())
        .args([
            "check",
            "-d",
            "{rules: {truthy: enable}}",
            "--enable",
            "trailing-spaces",
        ])
        .arg(&file));
    let out = command_output(&stdout, &stderr);
    assert_eq!(code, 1, "{out}");
    assert!(
        out.contains("trailing-spaces") && !out.contains("truthy"),
        "{out}"
    );
}

#[test]
fn enable_flag_applies_to_stdin() {
    let dir = tempdir().unwrap();
    let mut child = ryl(dir.path())
        .current_dir(dir.path())
        .args(["check", "--enable", "truthy", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(SAMPLE.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let text = String::from_utf8_lossy(&out.stderr).into_owned()
        + &String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(
        text.contains("truthy") && !text.contains("trailing-spaces"),
        "{text}"
    );
}

#[test]
fn enable_flag_keeps_per_file_ignores() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("a.yaml");
    fs::write(&file, SAMPLE).unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ntruthy = \"enable\"\n[per-file-ignores]\n\"a.yaml\" = [\"ALL\"]\n",
    )
    .unwrap();
    let (code, stdout, stderr) = run(ryl(dir.path())
        .args(["check", "--enable", "ALL"])
        .arg(&file));
    assert_eq!(code, 0, "{}", command_output(&stdout, &stderr));
}

#[test]
fn enable_flag_limits_fix_to_the_selected_rules() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("a.yaml");
    fs::write(&file, "a: 1   \n").unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ndocument-start = \"enable\"\n",
    )
    .unwrap();
    let (code, stdout, stderr) = run(ryl(dir.path())
        .args(["check", "--fix", "--enable", "trailing-spaces"])
        .arg(&file));
    assert_eq!(code, 0, "{}", command_output(&stdout, &stderr));
    assert_eq!(fs::read_to_string(&file).unwrap(), "a: 1\n");
}

#[test]
fn enable_flag_rejects_unknown_ids_and_lowercase_all() {
    for bad in ["nope", "all", "truthy,nope"] {
        let (code, _, stderr) = run(Command::new(env!("CARGO_BIN_EXE_ryl"))
            .args(["check", "--enable", bad, "x.yaml"]));
        assert_eq!(code, 2, "{bad}: {stderr}");
        assert!(stderr.contains("no such rule"), "{bad}: {stderr}");
    }
    let (code, _, stderr) = run(Command::new(env!("CARGO_BIN_EXE_ryl")).args([
        "check",
        "--list-files",
        "--enable",
        "truthy",
        ".",
    ]));
    assert_eq!(code, 2, "{stderr}");
}
