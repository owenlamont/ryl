mod common;

use std::fs;

use common::cli::{output_tuple, run, ryl, ryl_on, stdin_output};
use ryl::config::YamlLintConfig;
use ryl::config_schema::{
    normalize_toml_config, parse_toml_config_str, schema_value, yaml_schema_value,
};
use serde_json::json;
use tempfile::tempdir;

const RULES: &str = "[lint.rules]\ntruthy = { level = 'warning' }\ntrailing-spaces = { level = 'warning' }\n";

fn config(strict: Option<bool>) -> String {
    let mut config = String::from("[lint]\n");
    if let Some(strict) = strict {
        config.push_str(&format!("strict = {strict}\n"));
    }
    config.push_str(RULES);
    config
}

#[test]
fn cli_overrides_config_overrides_default_for_check_bare_fix_and_stdin() {
    for configured in [None, Some(false), Some(true)] {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(".ryl.toml"), config(configured)).unwrap();
        let file = dir.path().join("a.yaml");
        for (flags, override_value) in [
            (vec![], None),
            (vec!["--strict"], Some(true)),
            (vec!["-s"], Some(true)),
            (vec!["--no-strict"], Some(false)),
        ] {
            let expected = if override_value.or(configured).unwrap_or(false) {
                2
            } else {
                0
            };
            for mode in ["check", "bare", "fix", "stdin"] {
                fs::write(&file, "a: yes \n").unwrap();
                let mut cmd = ryl(dir.path());
                if mode != "bare" {
                    cmd.arg("check");
                }
                cmd.args(&flags);
                if mode == "fix" {
                    cmd.arg("--fix");
                }
                let (code, _, stderr) = if mode == "stdin" {
                    output_tuple(stdin_output(
                        cmd.arg("--stdin-filename").arg(&file).arg("-"),
                        b"a: yes \n",
                        |result| result.unwrap(),
                    ))
                } else {
                    run(cmd.arg(&file))
                };
                assert_eq!(
                    code, expected,
                    "{configured:?}, {flags:?}, {mode}: {stderr}"
                );
                assert!(stderr.contains("truthy"), "{stderr}");
                if mode == "fix" {
                    assert_eq!(fs::read_to_string(&file).unwrap(), "a: yes\n");
                }
            }
        }
    }
}

#[test]
fn strict_works_in_pyproject_explicit_files_and_inline_toml() {
    for strict in [false, true] {
        let dir = tempdir().unwrap();
        let cfg = config(Some(strict));
        fs::write(
            dir.path().join("pyproject.toml"),
            cfg.replace("[lint", "[tool.ryl.lint"),
        )
        .unwrap();
        let explicit = dir.path().join("config.toml");
        fs::write(&explicit, &cfg).unwrap();
        let file = dir.path().join("a.yaml");
        fs::write(&file, "a: yes\n").unwrap();
        for (flags, expected) in [
            (vec![], if strict { 2 } else { 0 }),
            (vec!["--strict"], 2),
            (vec!["--no-strict"], 0),
        ] {
            for source in ["project", "file", "inline"] {
                let mut cmd = ryl(dir.path());
                cmd.arg("check").args(&flags);
                match source {
                    "file" => {
                        cmd.arg("-c").arg(&explicit);
                    }
                    "inline" => {
                        cmd.arg("-d").arg(&cfg);
                    }
                    _ => {}
                }
                let (code, _, stderr) = run(cmd.arg(&file));
                assert_eq!(code, expected, "{strict}, {flags:?}, {source}: {stderr}");
                assert!(stderr.contains("truthy"), "{stderr}");
            }
        }
    }
}

#[test]
fn strict_preserves_clean_error_suppression_and_diff_exit_codes() {
    let cfg = config(Some(true));
    for (input, flags, expected) in [
        ("a: value\n", vec!["check"], 0),
        ("a: [\n", vec!["check"], 1),
        ("a: yes\n", vec!["check", "--no-warnings"], 0),
        ("a: yes\n", vec!["check", "--diff"], 0),
        ("a: yes \n", vec!["check", "--diff"], 1),
        ("a: value \n", vec!["check", "--fix"], 0),
        ("a: yes\n", vec!["--list-files"], 0),
        ("a: yes\n", vec!["check", "--strict", "--no-strict"], 2),
    ] {
        let (code, _, stderr, _) = ryl_on(Some(&cfg), input, &flags);
        assert_eq!(code, expected, "{flags:?}: {stderr}");
        if flags.contains(&"--no-strict") {
            assert!(stderr.contains("cannot be used with"), "{stderr}");
        }
    }
    let error_cfg = "[lint]\nstrict = true\n[lint.rules]\ntruthy = 'enable'\n";
    let (code, _, stderr, _) =
        ryl_on(Some(error_cfg), "a: yes\n", &["check", "--no-strict"]);
    assert_eq!(code, 1, "{stderr}");
}

#[test]
fn strict_uses_first_run_config_across_projects() {
    let dir = tempdir().unwrap();
    let mut files = Vec::new();
    for (name, strict) in [("strict", true), ("relaxed", false)] {
        let project = dir.path().join(name);
        fs::create_dir(&project).unwrap();
        fs::write(project.join(".ryl.toml"), config(Some(strict))).unwrap();
        let file = project.join("a.yaml");
        fs::write(&file, "a: yes\n").unwrap();
        files.push(file);
    }
    for expected in [2, 0] {
        let (code, _, stderr) = run(ryl(dir.path()).arg("check").args(&files));
        assert_eq!(code, expected, "{stderr}");
        files.reverse();
    }
}

#[test]
fn strict_is_a_toml_lint_boolean_and_survives_serialization() {
    let schema = schema_value();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let yaml_schema = yaml_schema_value();
    assert!(yaml_schema["properties"].get("strict").is_none());
    assert!(yaml_schema["properties"].get("lint").is_none());
    for strict in [false, true] {
        let text = config(Some(strict));
        let typed = parse_toml_config_str(&text, false).unwrap().unwrap();
        assert_eq!(normalize_toml_config(&typed).strict, Some(strict));
        let runtime = YamlLintConfig::from_toml_str(&text).unwrap();
        let serialized = runtime.to_toml_string();
        let value: toml::Table = serialized.parse().unwrap();
        assert_eq!(value["lint"]["strict"].as_bool(), Some(strict));
        assert_eq!(
            YamlLintConfig::from_toml_str(&serialized).unwrap().strict(),
            strict
        );
        assert!(validator.is_valid(&json!({"lint": {"strict": strict}})));
        assert!(!validator.is_valid(&json!({"strict": strict})));
    }
    for value in ["'true'", "1", "[]", "{}"] {
        assert!(
            YamlLintConfig::from_toml_str(&format!("[lint]\nstrict = {value}\n"))
                .is_err()
        );
    }
    assert!(!validator.is_valid(&json!({"lint": {"strict": "true"}})));
    for (yaml, expected) in [
        ("strict: true\nrules: {truthy: {level: warning}}\n", 0),
        (
            "lint: {strict: true}\nrules: {truthy: {level: warning}}\n",
            2,
        ),
    ] {
        let (code, _, stderr, _) = ryl_on(None, "a: yes\n", &["check", "-d", yaml]);
        assert_eq!(code, expected, "{stderr}");
    }
}
