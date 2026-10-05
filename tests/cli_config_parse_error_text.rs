use std::fs;
use std::process::Command;

use tempfile::tempdir;

mod common;
use common::cli::run;

fn config_error(name: &str, config: &str) -> String {
    let td = tempdir().unwrap();
    let cfg = td.path().join(name);
    fs::write(&cfg, config).unwrap();
    let file = td.path().join("a.yaml");
    fs::write(&file, "a: 1\n").unwrap();
    let (code, _, stderr) = run(Command::new(env!("CARGO_BIN_EXE_ryl"))
        .arg("check")
        .arg("-c")
        .arg(&cfg)
        .arg(&file));
    assert_eq!(code, 2, "stderr={stderr}");
    assert_eq!(stderr.lines().count(), 1, "stderr={stderr}");
    stderr.trim_end().to_owned()
}

#[test]
fn toml_syntax_error_is_one_line_with_location() {
    assert_eq!(
        config_error("bad.toml", "[rules\n"),
        "failed to parse config data: TOML parse error at line 1, column 7: \
         unclosed table, expected `]`"
    );
}

#[test]
fn pyproject_syntax_error_counts_columns_in_chars() {
    assert_eq!(
        config_error("pyproject.toml", "[tool.ryl]\nx = \"é\" é\n"),
        "failed to parse config data: TOML parse error at line 2, column 9: \
         unexpected key or value, expected newline, `#`"
    );
}

#[test]
fn yaml_config_type_error_keeps_key_path_on_one_line() {
    assert_eq!(
        config_error("bad.yaml", "rules:\n  indentation:\n    spaces: x\n"),
        "failed to parse config data: data did not match any variant of untagged \
         enum RuleEntry in `rules.indentation`"
    );
}

#[test]
fn newline_in_a_config_key_stays_escaped() {
    let err = config_error("bad.toml", "[output]\n\"a\\n::error::x\" = 1\n");
    assert!(err.contains("unknown field `a\\u{a}::error::x`"), "{err}");
    assert!(err.ends_with("in `output`"), "{err}");
}
