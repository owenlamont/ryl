mod common;

use std::path::PathBuf;

use ryl::config::{
    FixRule, FixRuleSelector, Overrides, discover_config, discover_config_with,
};

#[test]
fn yaml_config_rejects_fix_table() {
    let err = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some("fix:\n  fixable: [comments]\n".to_string()),
        },
    )
    .expect_err("yaml config should reject fix table");

    assert_eq!(
        err,
        "invalid config: fix is only supported in TOML configuration"
    );
}

#[test]
fn toml_config_parses_fix_policy() {
    let cfg = PathBuf::from("/repo/.ryl.toml");
    let env = common::fake_env::FakeEnv::new().with_file(
        cfg.clone(),
        "[fix]\nfixable = ['comments']\nunfixable = ['new-lines']\n",
    );

    let ctx = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(cfg),
            config_data: None,
        },
        &env,
    )
    .expect("toml config should parse");

    assert_eq!(
        ctx.config.fix().fixable(),
        [FixRuleSelector::Rule(FixRule::Comments)]
    );
    assert_eq!(ctx.config.fix().unfixable(), [FixRule::NewLines]);
    assert!(ctx.config.fix().allows_rule("comments"));
    assert!(!ctx.config.fix().allows_rule("new-lines"));
    assert!(!ctx.config.fix().allows_rule("new-line-at-end-of-file"));
}

#[test]
fn toml_config_parses_new_safe_fix_rules() {
    let cfg = PathBuf::from("/repo/.ryl.toml");
    let env = common::fake_env::FakeEnv::new().with_file(
        cfg.clone(),
        "[fix]\nfixable = ['braces', 'brackets', 'commas', 'comments-indentation']\nunfixable = ['braces']\n",
    );

    let ctx = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(cfg),
            config_data: None,
        },
        &env,
    )
    .expect("toml config should parse new fix rules");

    assert_eq!(
        ctx.config.fix().fixable(),
        [
            FixRuleSelector::Rule(FixRule::Braces),
            FixRuleSelector::Rule(FixRule::Brackets),
            FixRuleSelector::Rule(FixRule::Commas),
            FixRuleSelector::Rule(FixRule::CommentsIndentation),
        ]
    );
    assert_eq!(ctx.config.fix().unfixable(), [FixRule::Braces]);
    assert!(!ctx.config.fix().allows_rule("braces"));
    assert!(ctx.config.fix().allows_rule("brackets"));
    assert!(ctx.config.fix().allows_rule("commas"));
    assert!(ctx.config.fix().allows_rule("comments-indentation"));
}

#[test]
fn toml_config_parses_exact_typed_fix_variants() {
    let cfg = PathBuf::from("/repo/.ryl.toml");
    let env = common::fake_env::FakeEnv::new().with_file(
        cfg.clone(),
        "[fix]\nfixable = ['new-line-at-end-of-file']\nunfixable = ['brackets', 'commas', 'comments-indentation']\n",
    );

    let ctx = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(cfg),
            config_data: None,
        },
        &env,
    )
    .expect("toml config should parse typed fix variants");

    assert_eq!(
        ctx.config.fix().fixable(),
        [FixRuleSelector::Rule(FixRule::NewLineAtEndOfFile)]
    );
    assert_eq!(
        ctx.config.fix().unfixable(),
        [
            FixRule::Brackets,
            FixRule::Commas,
            FixRule::CommentsIndentation
        ]
    );
}

#[test]
fn toml_config_with_datetime_extra_parses_fix_policy() {
    let cfg = PathBuf::from("/repo/.ryl.toml");
    let env = common::fake_env::FakeEnv::new().with_file(
        cfg.clone(),
        "stamp = 1979-05-27T07:32:00Z\n[fix]\nfixable = ['ALL']\nunfixable = ['brackets', 'commas', 'comments-indentation']\n",
    );

    let ctx = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(cfg),
            config_data: None,
        },
        &env,
    )
    .expect("TOML config with datetime extra should still parse fix policy");

    assert_eq!(ctx.config.fix().fixable(), [FixRuleSelector::All]);
    assert_eq!(
        ctx.config.fix().unfixable(),
        [
            FixRule::Brackets,
            FixRule::Commas,
            FixRule::CommentsIndentation
        ]
    );
}

#[test]
fn fallback_toml_path_still_parses_fix_before_later_rule_error() {
    let cfg = PathBuf::from("/repo/.ryl.toml");
    let env = common::fake_env::FakeEnv::new().with_file(
        cfg.clone(),
        "stamp = 1979-05-27T07:32:00Z\n[fix]\nfixable = ['ALL']\nunfixable = ['comments']\n[rules.comments]\nrequire-starting-space = 1\n",
    );

    let err = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(cfg),
            config_data: None,
        },
        &env,
    )
    .expect_err("fallback TOML path should still reach legacy fix parsing");

    assert_eq!(
        err,
        "invalid config: option \"require-starting-space\" of \"comments\" should be bool"
    );
}

#[test]
fn default_fix_policy_allows_all_rules() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("input.yaml");
    std::fs::write(&file, "key: value\n").unwrap();

    let ctx = discover_config(&[file], &Overrides::default()).expect("default config");
    assert!(ctx.config.fix().allows_rule("braces"));
    assert!(ctx.config.fix().allows_rule("brackets"));
    assert!(ctx.config.fix().allows_rule("commas"));
    assert!(ctx.config.fix().allows_rule("comments"));
    assert!(ctx.config.fix().allows_rule("comments-indentation"));
    assert!(ctx.config.fix().allows_rule("new-lines"));
    assert!(!ctx.config.fix().allows_rule("indentation"));
    assert_eq!(ctx.config.fix().fixable(), [FixRuleSelector::All]);
}

#[test]
fn toml_config_rejects_non_mapping_fix_table() {
    let cfg = PathBuf::from("/repo/.ryl.toml");
    let env =
        common::fake_env::FakeEnv::new().with_file(cfg.clone(), "fix = 'comments'\n");

    let err = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(cfg),
            config_data: None,
        },
        &env,
    )
    .expect_err("fix should require a table");

    assert_eq!(err, "invalid config: fix should be a mapping");
}

#[test]
fn toml_config_rejects_unknown_fix_option() {
    let cfg = PathBuf::from("/repo/.ryl.toml");
    let env = common::fake_env::FakeEnv::new()
        .with_file(cfg.clone(), "[fix]\nunknown = ['comments']\n");

    let err = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(cfg),
            config_data: None,
        },
        &env,
    )
    .expect_err("unknown fix option should fail");

    assert_eq!(
        err,
        "invalid config: unknown option \"unknown\" for table \"fix\""
    );
}

#[test]
fn toml_config_rejects_non_list_fixable() {
    let cfg = PathBuf::from("/repo/.ryl.toml");
    let env = common::fake_env::FakeEnv::new()
        .with_file(cfg.clone(), "[fix]\nfixable = 'comments'\n");

    let err = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(cfg),
            config_data: None,
        },
        &env,
    )
    .expect_err("fixable should require a list");

    assert_eq!(
        err,
        "invalid config: option \"fixable\" of \"fix\" should be a list of strings"
    );
}

#[test]
fn toml_config_rejects_non_string_fix_rule_entries() {
    let cfg = PathBuf::from("/repo/.ryl.toml");
    let env = common::fake_env::FakeEnv::new()
        .with_file(cfg.clone(), "[fix]\nunfixable = [1]\n");

    let err = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(cfg),
            config_data: None,
        },
        &env,
    )
    .expect_err("fix rule entries should require strings");

    assert_eq!(
        err,
        "invalid config: option \"unfixable\" of \"fix\" should be a list of strings"
    );
}

#[test]
fn toml_config_rejects_unknown_fixable_rule_name() {
    let cfg = PathBuf::from("/repo/.ryl.toml");
    let env = common::fake_env::FakeEnv::new()
        .with_file(cfg.clone(), "[fix]\nfixable = ['indentation']\n");

    let err = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(cfg),
            config_data: None,
        },
        &env,
    )
    .expect_err("unknown fixable rule should fail");

    assert_eq!(
        err,
        "invalid config: option \"fixable\" of \"fix\" contains unknown fix rule \"indentation\""
    );
}

#[test]
fn toml_config_rejects_all_in_unfixable() {
    let cfg = PathBuf::from("/repo/.ryl.toml");
    let env = common::fake_env::FakeEnv::new()
        .with_file(cfg.clone(), "[fix]\nunfixable = ['ALL']\n");

    let err = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(cfg),
            config_data: None,
        },
        &env,
    )
    .expect_err("ALL should not be valid in unfixable");

    assert_eq!(
        err,
        "invalid config: option \"unfixable\" of \"fix\" contains unknown fix rule \"ALL\""
    );
}

#[test]
fn toml_config_rejects_non_list_unfixable() {
    let cfg = PathBuf::from("/repo/.ryl.toml");
    let env = common::fake_env::FakeEnv::new()
        .with_file(cfg.clone(), "[fix]\nunfixable = 'comments'\n");

    let err = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(cfg),
            config_data: None,
        },
        &env,
    )
    .expect_err("unfixable should require a list");

    assert_eq!(
        err,
        "invalid config: option \"unfixable\" of \"fix\" should be a list of strings"
    );
}

#[test]
fn toml_config_rejects_non_string_fixable_entries() {
    let cfg = PathBuf::from("/repo/.ryl.toml");
    let env = common::fake_env::FakeEnv::new()
        .with_file(cfg.clone(), "[fix]\nfixable = [1]\n");

    let err = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(cfg),
            config_data: None,
        },
        &env,
    )
    .expect_err("fixable entries should require strings");

    assert_eq!(
        err,
        "invalid config: option \"fixable\" of \"fix\" should be a list of strings"
    );
}

#[test]
fn yaml_extends_default_keeps_default_fix_policy() {
    let cfg = ryl::config::YamlLintConfig::from_yaml_str("extends: default\n")
        .expect("extends should parse");

    assert_eq!(cfg.fix().fixable(), [FixRuleSelector::All]);
    assert!(cfg.fix().unfixable().is_empty());
}
