use ryl::config::{Overrides, YamlLintConfig, discover_config};
use ryl::rules::anchors;

#[test]
fn anchors_forbid_all_is_opt_in_and_requires_a_boolean() {
    let content = "defaults: &defaults {retries: 3}\njob: *defaults\n";
    for option in ["", "forbid-anchors-and-aliases = false\n"] {
        let config =
            YamlLintConfig::from_toml_str(&format!("[lint.rules.anchors]\n{option}"))
                .unwrap();
        assert!(anchors::check(content, &anchors::Config::resolve(&config)).is_empty());
    }
    let error = YamlLintConfig::from_toml_str(
        "[lint.rules.anchors]\nforbid-anchors-and-aliases = 'true'\n",
    )
    .unwrap_err();
    assert!(error.contains("lint.rules.anchors"), "{error}");
}

#[test]
fn anchors_yaml_config_rejects_forbid_all_option() {
    let error = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(
                "rules:\n  anchors:\n    forbid-anchors-and-aliases: true\n".into(),
            ),
        },
    )
    .unwrap_err();
    assert!(error.contains("rules.anchors"), "{error}");
    assert!(error.contains("failed to parse config data:"), "{error}");
}

#[test]
fn anchors_allows_boolean_options() {
    let cfg = r#"
rules:
  anchors:
    forbid-undeclared-aliases: false
    forbid-duplicated-anchors: true
    forbid-unused-anchors: true
"#;

    let ctx = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(cfg.into()),
        },
    )
    .expect("parse");

    assert!(ctx.config.rule_names().iter().any(|r| r == "anchors"));
}

#[test]
fn anchors_rejects_non_bool_option() {
    let cfg = r#"
rules:
  anchors:
    forbid-duplicated-anchors: "yes"
"#;

    let err = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(cfg.into()),
        },
    )
    .expect_err("invalid value");

    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.anchors"), "{err}");
}

#[test]
fn anchors_rejects_unknown_option() {
    let cfg = r#"
rules:
  anchors:
    unknown: true
"#;

    let err = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(cfg.into()),
        },
    )
    .expect_err("unknown option");

    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.anchors"), "{err}");
}

#[test]
fn anchors_toml_enables_ambiguous_names() {
    let cfg = YamlLintConfig::from_toml_str(
        "[lint.rules.anchors]\nforbid-ambiguous-anchor-alias-names = true\n",
    )
    .expect("toml parse");
    let resolved = anchors::Config::resolve(&cfg);
    let hits = anchors::check("a: &foo: 1\n", &resolved);
    assert_eq!(hits.len(), 1, "expected one ambiguous diagnostic: {hits:?}");
    assert!(
        hits[0].message.contains("ambiguous anchor name \"foo:\""),
        "unexpected message: {hits:?}"
    );
}

#[test]
fn anchors_yaml_config_rejects_ambiguous_names_option() {
    let cfg = r#"
rules:
  anchors:
    forbid-ambiguous-anchor-alias-names: true
"#;

    let err = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(cfg.into()),
        },
    )
    .expect_err("ryl-only option");

    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("rules.anchors"), "{err}");
}

#[test]
fn anchors_rejects_non_string_option_key() {
    let cfg = r#"
rules:
  anchors:
    ? [1, 2]
    : true
"#;

    let err = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(cfg.into()),
        },
    )
    .expect_err("non-string key");

    assert!(err.contains("cannot convert non-string TOML key"), "{err}");
}
