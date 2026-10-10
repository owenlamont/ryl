use ryl::config::{ConfigContext, Overrides, SourceKind, discover_config_with};
use ryl::lint_str;

#[path = "common/mod.rs"]
mod common;
use common::fake_env::FakeEnv;

fn inline_config(env: &FakeEnv, data: &str) -> ConfigContext {
    discover_config_with(
        &[],
        &Overrides {
            config_data: Some(data.into()),
            config_file: None,
        },
        env,
    )
    .unwrap()
}

#[test]
fn path_case_policy_covers_globs_and_config_roots() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("ProjectRoot");
    let alias = dir.path().join("PROJECTROOT");
    let config = r#"
exclude = ["skip/**", "!skip/keep.yaml"]
[files]
yaml = ["*.yaml"]
markdown = ["docs/**/*.md"]
[lint.rules.trailing-spaces]
ignore = ["rule/**"]
[lint.rules.truthy]
[lint.rules.key-ordering]
[[lint.rules.key-ordering.orders]]
files = ["ordered/**"]
path = "$"
keys = ["z", "a"]
[lint.per-file-ignores]
"ignored/**" = ["truthy"]
[[lint.per-line-ignores]]
path = "lines/**"
rules = ["truthy"]
"#;
    for insensitive in [false, true] {
        let env = FakeEnv::new()
            .with_cwd(&root)
            .with_case_insensitive_paths(insensitive)
            .with_file(root.join("patterns.ignore"), "external/**\n");
        let ctx = inline_config(&env, config);
        let cfg = &ctx.config;
        assert!(cfg.is_file_ignored(&root.join("skip/x.yaml"), &root));
        assert_eq!(
            cfg.source_kind(&root.join("x.yaml"), &root).unwrap(),
            Some(SourceKind::Yaml)
        );
        assert!(cfg.is_rule_ignored(
            "trailing-spaces",
            &root.join("rule/x.yaml"),
            &root
        ));
        assert!(cfg.is_rule_ignored("truthy", &root.join("ignored/x.yaml"), &root));
        assert_eq!(
            cfg.is_file_ignored(&alias.join("SKIP/x.yaml"), &root),
            insensitive
        );
        assert!(!cfg.is_file_ignored(&alias.join("SKIP/KEEP.yaml"), &root));
        assert_eq!(
            cfg.source_kind(&alias.join("x.YAML"), &root).unwrap(),
            insensitive.then_some(SourceKind::Yaml)
        );
        assert_eq!(
            cfg.source_kind(&alias.join("DOCS/x.MD"), &root).unwrap(),
            insensitive.then_some(SourceKind::Markdown)
        );
        assert_eq!(
            cfg.is_rule_ignored("trailing-spaces", &alias.join("RULE/x.yaml"), &root),
            insensitive
        );
        assert_eq!(
            cfg.is_rule_ignored("truthy", &alias.join("IGNORED/x.yaml"), &root),
            insensitive
        );
        let problems = lint_str("a: yes\n", &alias.join("LINES/x.yaml"), cfg, &root);
        assert_eq!(
            problems
                .iter()
                .any(|problem| problem.rule == Some("truthy")),
            !insensitive
        );
        let problems =
            lint_str("z: 1\na: 2\n", &alias.join("ORDERED/x.yaml"), cfg, &root);
        assert_eq!(
            problems
                .iter()
                .any(|problem| problem.rule == Some("key-ordering")),
            !insensitive
        );
        assert!(!cfg.is_file_ignored(&dir.path().join("Elsewhere/SKIP/x.yaml"), &root));
        assert!(!cfg.is_file_ignored(dir.path(), &root));
        let ctx = inline_config(
            &env,
            &config.replace(
                "exclude = [\"skip/**\", \"!skip/keep.yaml\"]",
                "exclude-from-file = ['patterns.ignore']",
            ),
        );
        assert_eq!(
            ctx.config
                .is_file_ignored(&alias.join("EXTERNAL/x.yaml"), &root),
            insensitive
        );
        let ctx = inline_config(
            &env,
            &config.replace(
                "\"ignored/**\" = [\"truthy\"]",
                "\"!*.keep.yaml\" = [\"truthy\"]",
            ),
        );
        assert!(!ctx.config.is_rule_ignored(
            "truthy",
            &root.join("x.keep.yaml"),
            &root
        ));
        assert!(ctx.config.is_rule_ignored(
            "truthy",
            &root.join("x.other.yaml"),
            &root
        ));
        assert_eq!(
            ctx.config
                .is_rule_ignored("truthy", &alias.join("x.KEEP.yaml"), &root),
            !insensitive
        );
    }
}

#[test]
fn case_alias_discovery_stops_at_home_and_anchors_config_container() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("ProjectRoot");
    let alias = dir.path().join("PROJECTROOT");
    let env = FakeEnv::new()
        .with_cwd(&root)
        .with_case_insensitive_paths(true)
        .with_var("HOME", root.to_string_lossy().into_owned())
        .with_file(dir.path().join(".ryl.toml"), "[lint.rules.truthy]\n");
    let inputs = [alias.join("sub/x.yaml")];
    assert!(
        !discover_config_with(&inputs, &Overrides::default(), &env)
            .unwrap()
            .config_found
    );
    let env = env.with_file(
        root.join(".config/ryl.toml"),
        "exclude = ['skip/**']\n[lint.rules.truthy]\n",
    );
    let ctx = discover_config_with(
        &inputs,
        &Overrides {
            config_file: Some(alias.join(".CONFIG/RYL.TOML")),
            config_data: None,
        },
        &env,
    )
    .unwrap();
    assert_eq!(ctx.base_dir, alias);
    assert!(
        ctx.config
            .is_file_ignored(&root.join("skip/x.yaml"), &ctx.base_dir)
    );
    let env = env.with_file(
        root.join("pyproject.toml"),
        "[tool.ryl.lint.rules.truthy]\n",
    );
    let ctx = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(root.join("PYPROJECT.TOML")),
            config_data: None,
        },
        &env,
    )
    .unwrap();
    assert!(ctx.config.rule_names().iter().any(|name| name == "truthy"));
    let env = FakeEnv::new().with_case_insensitive_paths(true);
    let ctx = inline_config(&env, "exclude = ['skip/**']\n[lint.rules.truthy]\n");
    assert!(
        ctx.config
            .is_file_ignored(std::path::Path::new("SKIP/x.yaml"), &ctx.base_dir)
    );
    let env = env.with_cwd("");
    let ctx = inline_config(&env, "exclude = ['skip/**']\n[lint.rules.truthy]\n");
    let path = std::env::current_dir().unwrap().join("SKIP/x.yaml");
    let alias = std::path::PathBuf::from(path.to_string_lossy().to_ascii_uppercase());
    assert!(ctx.config.is_file_ignored(&alias, &ctx.base_dir));
}
