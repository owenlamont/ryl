use std::fs;

use ryl::config::YamlLintConfig;
use ryl::config_schema::{
    parse_toml_config_str, schema_value, toml_config_to_value, yaml_schema_value,
};
use ryl::format;
use ryl::rules::indentation::{self, IndentSequencesSetting, SpacesSetting};
use ryl::rules::line_length;
use tempfile::tempdir;

mod common;
use common::cli::{command_output, run, ryl};

fn toml(text: &str) -> YamlLintConfig {
    YamlLintConfig::from_toml_str(text).expect("valid TOML config")
}

fn spaces(spaces: SpacesSetting) -> indentation::Config {
    indentation::Config::new(spaces, IndentSequencesSetting::True, false)
}

fn line_length_hits(cfg: &YamlLintConfig) -> Vec<String> {
    let line = format!("key: {}\n", "x".repeat(85));
    line_length::check(&line, &line_length::Config::resolve(cfg))
        .into_iter()
        .map(|hit| hit.message)
        .collect()
}

#[test]
fn indentation_spaces_follows_explicit_then_shared_then_consistent() {
    for (config, expected) in [
        (
            "[lint.rules]\nindentation = \"enable\"\n",
            SpacesSetting::Consistent,
        ),
        (
            "indent-width = 4\n[lint.rules]\nindentation = \"enable\"\n",
            SpacesSetting::Fixed(4),
        ),
        (
            "indent-width = 4\n[lint.rules.indentation]\nspaces = 2\n",
            SpacesSetting::Fixed(2),
        ),
        (
            "indent-width = 4\n[lint.rules.indentation]\nspaces = \"consistent\"\n",
            SpacesSetting::Consistent,
        ),
    ] {
        assert_eq!(
            indentation::Config::resolve(&toml(config)),
            spaces(expected),
            "{config}"
        );
    }
    let yaml =
        YamlLintConfig::from_yaml_str("rules:\n  indentation: enable\n").unwrap();
    assert_eq!(yaml.indent_width(), None);
    assert_eq!(
        indentation::Config::resolve(&yaml),
        spaces(SpacesSetting::Consistent)
    );
}

#[test]
fn line_length_max_follows_explicit_then_shared_then_80() {
    for (config, expected) in [
        (
            "[lint.rules]\nline-length = \"enable\"\n",
            vec!["(90 > 80 characters)"],
        ),
        (
            "line-length = 100\n[lint.rules]\nline-length = \"enable\"\n",
            vec![],
        ),
        (
            "line-length = 100\n[lint.rules.line-length]\nmax = 60\n",
            vec!["(90 > 60 characters)"],
        ),
    ] {
        let hits = line_length_hits(&toml(config));
        assert_eq!(hits.len(), expected.len(), "{config}: {hits:?}");
        for (hit, want) in hits.iter().zip(expected) {
            assert!(hit.ends_with(want), "{config}: {hit}");
        }
    }
}

#[test]
fn admits_width_accepts_consistent_and_the_matching_fixed_width() {
    assert!(spaces(SpacesSetting::Consistent).admits_width(2));
    assert!(spaces(SpacesSetting::Fixed(4)).admits_width(4));
    assert!(!spaces(SpacesSetting::Fixed(4)).admits_width(2));
}

#[test]
fn formatter_targets_detect_and_default_to_80_and_follow_the_shared_keys() {
    let unset = toml("[lint.rules]\nindentation = \"enable\"\n");
    assert_eq!(
        (
            format::file_indent_width(&unset, "a:\n    b: 1\n"),
            format::line_length(&unset)
        ),
        (4, 80)
    );
    let set = toml(
        "indent-width = 4\nline-length = 100\n[lint.rules.indentation]\nspaces = 2\n",
    );
    assert_eq!(
        (
            format::file_indent_width(&set, "a:\n  b: 1\n"),
            format::line_length(&set)
        ),
        (4, 100)
    );
}

#[test]
fn effective_config_round_trips_the_shared_keys_only_when_set() {
    let original = toml(
        "indent-width = 4\nline-length = 100\n[lint.rules]\nindentation = \"enable\"\n",
    );
    let rendered = original.to_toml_string();
    let reloaded = toml(&rendered);
    assert_eq!(reloaded.indent_width(), original.indent_width());
    assert_eq!(reloaded.line_length(), original.line_length());
    assert_eq!(
        indentation::Config::resolve(&reloaded),
        spaces(SpacesSetting::Fixed(4))
    );
    assert!(line_length_hits(&reloaded).is_empty());

    let unset = toml("[lint.rules]\nindentation = \"enable\"\n").to_toml_string();
    assert!(!unset.contains("indent-width") && !unset.contains("line-length ="));
}

#[test]
fn migration_keeps_the_shared_keys_without_deprecating_them() {
    let legacy = parse_toml_config_str(
        "line-length = 100\nindent-width = 4\n[rules]\ntruthy = \"enable\"\n",
        false,
    )
    .unwrap()
    .unwrap();
    let deprecated: Vec<_> = legacy
        .deprecated_keys()
        .into_iter()
        .map(|k| k.key.key)
        .collect();
    assert_eq!(deprecated, ["rules"]);
    let migrated = toml_config_to_value(&legacy.to_nested());
    assert_eq!(migrated["line-length"].as_integer(), Some(100));
    assert_eq!(migrated["indent-width"].as_integer(), Some(4));
    assert!(migrated["lint"]["rules"].get("truthy").is_some());
}

#[test]
fn out_of_range_or_mistyped_shared_keys_are_rejected() {
    for config in [
        "indent-width = 0",
        "indent-width = -1",
        "indent-width = 256",
        "indent-width = \"2\"",
        "line-length = 0",
        "line-length = 70000",
        "line-len = 100",
    ] {
        let err = YamlLintConfig::from_toml_str(&format!(
            "{config}\n[lint.rules]\nindentation = \"enable\"\n"
        ))
        .unwrap_err();
        let key = config.split(' ').next().unwrap();
        assert!(err.contains(key), "{config}: {err}");
    }
}

#[test]
fn only_the_toml_schema_declares_the_shared_keys() {
    let toml_schema = schema_value();
    for (key, max) in [("line-length", 65535), ("indent-width", 255)] {
        let property = &toml_schema["properties"][key];
        assert_eq!(property["minimum"], 1, "{key}");
        assert_eq!(property["maximum"], max, "{key}");
        assert!(
            yaml_schema_value()["properties"].get(key).is_none(),
            "{key}"
        );
    }
}

#[test]
fn check_applies_shared_keys_from_a_project_config() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "indent-width = 4\nline-length = 20\n\
         [lint.rules]\nindentation = \"enable\"\nline-length = \"enable\"\n",
    )
    .unwrap();
    let file = dir.path().join("a.yaml");
    fs::write(&file, "key:\n  sub: a long value past twenty\n").unwrap();
    let (code, stdout, stderr) = run(ryl(dir.path()).arg("check").arg(&file));
    let output = command_output(&stdout, &stderr);
    assert_eq!(code, 1, "{output}");
    assert!(
        output.contains("2:3") && output.contains("indentation"),
        "{output}"
    );
    assert!(
        output.contains("2:21") && output.contains("line-length"),
        "{output}"
    );
}

#[test]
fn migration_keeps_the_format_keys() {
    let legacy = parse_toml_config_str(
        "[format]\ncomment-spacing = 3\ncomment-starting-space = 'preserve'\n\
         max-blank-lines = 0\n[rules]\ntruthy = \"enable\"\n",
        false,
    )
    .unwrap()
    .unwrap();
    let migrated = toml_config_to_value(&legacy.to_nested());
    let format = &migrated["format"];
    assert_eq!(format["comment-spacing"].as_integer(), Some(3));
    assert_eq!(format["comment-starting-space"].as_str(), Some("preserve"));
    assert_eq!(format["max-blank-lines"].as_integer(), Some(0));
}
