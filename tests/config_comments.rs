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
