use std::fs;

use tempfile::tempdir;

use crate::common;
use common::cli::{command_output, run, ryl};

const ENABLE: &str = "[lint.rules]\nschema-dependent-values = \"enable\"\n";

fn check(config: &str, content: &str) -> (i32, String) {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".ryl.toml"), config).unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, content).unwrap();
    let (code, stdout, stderr) = run(ryl(dir.path()).arg("check").arg(&file));
    (code, command_output(&stdout, &stderr).to_string())
}

const KINDS: &str = "a: 2026-10-10\nb: 22:22\nc: 1_000\nd: 0b1010\ne: 08\nf: y\nn: 1\ng: 0_8\nh: 1_0e5\n";

#[test]
fn reports_each_kind_with_both_readings() {
    let (code, output) = check(ENABLE, KINDS);
    assert_eq!(code, 1, "{output}");
    for expected in [
        "1:4 ",
        "plain \"2026-10-10\" is a string in YAML 1.2 but a timestamp in YAML 1.1",
        "plain \"22:22\" is a string in YAML 1.2 but a sexagesimal number in YAML 1.1",
        "plain \"1_000\" is a string in YAML 1.2 but a number in YAML 1.1",
        "plain \"0b1010\" is a string in YAML 1.2 but a number in YAML 1.1",
        "plain \"08\" is an integer in YAML 1.2 but a string in YAML 1.1",
        "plain \"y\" is a string in YAML 1.2 but a boolean in YAML 1.1",
        "7:1 ",
        "plain \"0_8\" is a string in YAML 1.2 but a number in YAML 1.1",
        "plain \"1_0e5\" is a string in YAML 1.2 but a number in YAML 1.1",
    ] {
        assert!(output.contains(expected), "missing {expected:?}: {output}");
    }
}

#[test]
fn each_option_turns_its_kind_off() {
    let config = "[lint.rules.schema-dependent-values]\nforbid-timestamps = false\n\
                  forbid-sexagesimal = false\nforbid-underscored-numbers = false\n\
                  forbid-leading-zero-decimals = false\n\
                  forbid-single-letter-booleans = false\n";
    let (code, output) = check(config, KINDS);
    assert_eq!(code, 0, "{output}");
}

#[test]
fn stays_quiet_where_one_reader_set_is_declared_or_the_value_is_not_plain() {
    for (config, content) in [
        (
            format!("target-version = \"1.2\"\n{ENABLE}"),
            KINDS.to_string(),
        ),
        (ENABLE.to_string(), format!("%YAML 1.2\n---\n{KINDS}")),
        (ENABLE.to_string(), format!("%YAML 1.1\n---\n{KINDS}")),
        (
            ENABLE.to_string(),
            "a: '22:22'\nb: !!str 08\nc: 0777\nd: 1e5\ne: yes\n".to_string(),
        ),
    ] {
        let (code, output) = check(&config, &content);
        assert_eq!(code, 0, "{config} {content:?}: {output}");
    }
}
