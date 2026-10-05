use ryl::config::YamlLintConfig;

#[test]
fn rejects_unknown_option() {
    let err =
        YamlLintConfig::from_yaml_str("rules:\n  comments:\n    unexpected: true\n")
            .unwrap_err();
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.comments"), "{err}");
}

#[test]
fn rejects_non_bool_require_starting_space() {
    let err = YamlLintConfig::from_yaml_str(
        "rules:\n  comments:\n    require-starting-space: 1\n",
    )
    .unwrap_err();
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.comments"), "{err}");
}

#[test]
fn rejects_non_bool_ignore_shebangs() {
    let err =
        YamlLintConfig::from_yaml_str("rules:\n  comments:\n    ignore-shebangs: []\n")
            .unwrap_err();
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.comments"), "{err}");
}

#[test]
fn rejects_non_integer_min_spaces() {
    let err = YamlLintConfig::from_yaml_str(
        "rules:\n  comments:\n    min-spaces-from-content: true\n",
    )
    .unwrap_err();
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.comments"), "{err}");
}

#[test]
fn accepts_valid_configuration() {
    let cfg = YamlLintConfig::from_yaml_str(
        "rules:\n  comments:\n    require-starting-space: false\n    ignore-shebangs: false\n    min-spaces-from-content: 4\n",
    )
    .expect("configuration should parse");
    assert!(cfg.rule_names().iter().any(|name| name == "comments"));
}

#[test]
fn accepts_negative_min_spaces() {
    let cfg = YamlLintConfig::from_yaml_str(
        "rules:\n  comments:\n    min-spaces-from-content: -1\n",
    )
    .expect("configuration should parse");
    assert!(cfg.rule_names().iter().any(|name| name == "comments"));
}

#[test]
fn max_spaces_rejected_in_yaml_config() {
    let err = YamlLintConfig::from_yaml_str(
        "rules:\n  comments:\n    max-spaces-from-content: 2\n",
    )
    .unwrap_err();
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.comments"), "{err}");
}

#[test]
fn max_spaces_zero_is_rejected() {
    let err = YamlLintConfig::from_toml_str(
        "[lint.rules.comments]\nmin-spaces-from-content = -1\nmax-spaces-from-content = 0\n",
    )
    .unwrap_err();
    assert!(err.contains("must be at least 1"), "{err}");
}

#[test]
fn max_spaces_below_default_min_is_rejected() {
    let err = YamlLintConfig::from_toml_str(
        "[lint.rules.comments]\nmax-spaces-from-content = 1\n",
    )
    .unwrap_err();
    assert!(err.contains("\"max-spaces-from-content\" (1)"), "{err}");
    assert!(err.contains("\"min-spaces-from-content\" (2;"), "{err}");
}

#[test]
fn max_spaces_accepts_valid_pairings() {
    for (min, max) in [(1, 1), (-1, 1), (5, -1)] {
        let toml = format!(
            "[lint.rules.comments]\nmin-spaces-from-content = {min}\nmax-spaces-from-content = {max}\n"
        );
        YamlLintConfig::from_toml_str(&toml)
            .unwrap_or_else(|err| panic!("min {min} max {max} should load: {err}"));
    }
    YamlLintConfig::from_toml_str("[lint.rules]\ncomments = 'enable'\n")
        .expect("comments without options should load");
}
