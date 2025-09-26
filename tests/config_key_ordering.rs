use ryl::config::YamlLintConfig;

#[test]
fn ignored_keys_sequence_accepts_valid_entries() {
    let cfg = YamlLintConfig::from_yaml_str(
        "rules:\n  key-ordering:\n    ignored-keys: [\"name\", \"^b\"]\n",
    )
    .expect("config should parse");
    let resolved = ryl::rules::key_ordering::Config::resolve(&cfg);
    let hits = ryl::rules::key_ordering::check("b: 1\na: 1\n", &resolved);
    assert!(
        hits.is_empty(),
        "ignored keys should skip enforcement: {hits:?}"
    );
}

#[test]
fn ignored_keys_sequence_non_string_errors() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  key-ordering:\n    ignored-keys: [1]\n")
        .expect_err("non-string sequence entries should error");
    assert!(
        err.contains("ignored-keys"),
        "error should mention ignored-keys: {err}"
    );
}

#[test]
fn ignored_keys_sequence_invalid_regex_errors() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  key-ordering:\n    ignored-keys: [\"[\"]\n")
        .expect_err("invalid regex should error");
    assert!(err.contains("invalid regex"), "unexpected message: {err}");
}

#[test]
fn ignored_keys_scalar_invalid_regex_errors() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  key-ordering:\n    ignored-keys: \"[\"\n")
        .expect_err("invalid scalar regex should error");
    assert!(err.contains("invalid regex"), "unexpected message: {err}");
}

#[test]
fn ignored_keys_invalid_type_errors() {
    let err =
        YamlLintConfig::from_yaml_str("rules:\n  key-ordering:\n    ignored-keys: {bad: true}\n")
            .expect_err("non sequence/string should error");
    assert!(err.contains("should contain regex strings"), "{err}");
}

#[test]
fn key_ordering_unknown_option_errors() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  key-ordering:\n    unexpected: true\n")
        .expect_err("unknown option should error");
    assert!(err.contains("unknown option \"unexpected\""), "{err}");
}

#[test]
fn key_ordering_non_string_key_errors() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  key-ordering:\n    1: true\n")
        .expect_err("non-string key should error");
    assert!(err.contains("unknown option"), "{err}");
}
