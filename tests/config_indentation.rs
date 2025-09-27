use ryl::config::YamlLintConfig;

#[test]
fn rejects_invalid_spaces_type() {
    let err =
        YamlLintConfig::from_yaml_str("rules:\n  indentation:\n    spaces: foo\n").unwrap_err();
    assert_eq!(
        err,
        "invalid config: option \"spaces\" of \"indentation\" should be in (<class 'int'>, 'consistent')"
    );
}

#[test]
fn rejects_invalid_indent_sequences_type() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  indentation:\n    indent-sequences: 1\n")
        .unwrap_err();
    assert_eq!(
        err,
        "invalid config: option \"indent-sequences\" of \"indentation\" should be in (<class 'bool'>, 'whatever', 'consistent')"
    );
}

#[test]
fn rejects_invalid_multiline_flag() {
    let err =
        YamlLintConfig::from_yaml_str("rules:\n  indentation:\n    check-multi-line-strings: []\n")
            .unwrap_err();
    assert_eq!(
        err,
        "invalid config: option \"check-multi-line-strings\" of \"indentation\" should be bool"
    );
}

#[test]
fn accepts_full_configuration() {
    let cfg = YamlLintConfig::from_yaml_str(
        "rules:\n  indentation:\n    spaces: 2\n    indent-sequences: consistent\n    check-multi-line-strings: true\n",
    )
    .expect("configuration should parse");
    assert!(cfg.rule_names().iter().any(|name| name == "indentation"));
}

#[test]
fn rejects_unknown_option_key() {
    let err = YamlLintConfig::from_yaml_str("rules:\n  indentation:\n    unexpected: true\n")
        .unwrap_err();
    assert_eq!(
        err,
        "invalid config: unknown option \"unexpected\" for rule \"indentation\""
    );
}

#[test]
fn accepts_whatever_sequence_setting() {
    let cfg =
        YamlLintConfig::from_yaml_str("rules:\n  indentation:\n    indent-sequences: whatever\n")
            .expect("configuration should parse");
    assert!(cfg.rule_names().iter().any(|name| name == "indentation"));
}
