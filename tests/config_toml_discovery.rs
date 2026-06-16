use std::path::{Path, PathBuf};

use ryl::config::{Overrides, RuleLevel, discover_config_with, discover_per_file};
use tempfile::tempdir;

#[path = "common/mod.rs"]
mod common;
use common::fake_env::FakeEnv;

#[test]
fn project_toml_takes_precedence_over_yaml_and_adds_notice() {
    let env = FakeEnv::new()
        .with_cwd(PathBuf::from("/repo"))
        .with_file(
            PathBuf::from("/repo/.ryl.toml"),
            "locale = 'fr_FR.UTF-8'\n[rules]\nanchors = 'disable'\n",
        )
        .with_file(
            PathBuf::from("/repo/.yamllint"),
            "locale: en_US.UTF-8\nrules: {}\n",
        )
        .with_exists(PathBuf::from("/repo/file.yaml"));
    let ctx = discover_config_with(
        &[PathBuf::from("/repo/file.yaml")],
        &Overrides::default(),
        &env,
    )
    .expect("project TOML should load");
    assert_eq!(ctx.source.as_deref(), Some(Path::new("/repo/.ryl.toml")));
    assert_eq!(ctx.config.locale(), Some("fr_FR.UTF-8"));
    assert_eq!(ctx.notices.len(), 1);
    assert!(ctx.notices[0].contains("ignoring legacy YAML config discovery"));
}

#[test]
fn explicit_pyproject_with_tool_ryl_section_loads() {
    let pyproject = PathBuf::from("/repo/pyproject.toml");
    let env = FakeEnv::new().with_cwd(PathBuf::from("/repo")).with_file(
        pyproject.clone(),
        "[project]\nname = 'demo'\nversion = '0.1.0'\n[tool.ryl]\nlocale = 'en_GB.UTF-8'\n",
    );
    let ctx = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(pyproject),
            config_data: None,
        },
        &env,
    )
    .expect("explicit pyproject [tool.ryl] should load");
    assert_eq!(ctx.config.locale(), Some("en_GB.UTF-8"));
}

#[test]
fn exact_typed_toml_preserves_runtime_matchers_and_rule_settings() {
    let cfg = PathBuf::from("/repo/.ryl.toml");
    let env = FakeEnv::new().with_cwd(PathBuf::from("/repo")).with_file(
        cfg.clone(),
        "files = { yaml = ['configs/**/*.yaml'] }\nignore = ['vendor/**']\nlocale = 'en_US.UTF-8'\n[rules]\ndocument-start = 'disable'\n[rules.comments]\nlevel = 'warning'\nrequire-starting-space = true\nignore = ['generated/**']\n",
    );
    let ctx = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(cfg),
            config_data: None,
        },
        &env,
    )
    .expect("exact typed TOML should load");

    assert_eq!(ctx.config.locale(), Some("en_US.UTF-8"));
    assert_eq!(ctx.config.rule_level("comments"), Some(RuleLevel::Warning));
    assert!(
        ctx.config
            .rule_option_bool("comments", "require-starting-space", false)
    );
    assert!(
        ctx.config
            .is_file_ignored(Path::new("/repo/vendor/data.yaml"), Path::new("/repo"),)
    );
    assert!(ctx.config.is_yaml_candidate(
        Path::new("/repo/configs/app.yaml"),
        Path::new("/repo"),
    ));
    assert!(
        !ctx.config
            .is_yaml_candidate(Path::new("/repo/docs/app.yaml"), Path::new("/repo"),)
    );
    assert!(ctx.config.is_rule_ignored(
        "comments",
        Path::new("/repo/generated/out.yaml"),
        Path::new("/repo"),
    ));
    assert!(!ctx.config.is_rule_ignored(
        "document-start",
        Path::new("/repo/generated/out.yaml"),
        Path::new("/repo"),
    ));
}

#[test]
fn project_pyproject_without_tool_ryl_falls_back_to_yaml() {
    let env = FakeEnv::new()
        .with_cwd(PathBuf::from("/repo"))
        .with_file(
            PathBuf::from("/repo/pyproject.toml"),
            "[project]\nname = 'demo'\nversion = '0.1.0'\n",
        )
        .with_file(
            PathBuf::from("/repo/.yamllint"),
            "locale: en_US.UTF-8\nrules: {}\n",
        )
        .with_exists(PathBuf::from("/repo/file.yaml"));
    let ctx = discover_config_with(
        &[PathBuf::from("/repo/file.yaml")],
        &Overrides::default(),
        &env,
    )
    .expect("yaml fallback should load");
    assert_eq!(ctx.source.as_deref(), Some(Path::new("/repo/.yamllint")));
    assert_eq!(ctx.config.locale(), Some("en_US.UTF-8"));
    assert!(ctx.notices.is_empty());
}

#[test]
fn discover_per_file_finds_project_toml() {
    let td = tempdir().unwrap();
    let root = td.path();
    std::fs::write(root.join(".ryl.toml"), "[rules]\nanchors = 'disable'\n").unwrap();
    std::fs::write(root.join("file.yaml"), "a: 1\n").unwrap();
    let ctx = discover_per_file(&root.join("file.yaml")).expect("per-file discovery");
    assert_eq!(
        ctx.source.as_deref(),
        Some(root.join(".ryl.toml").as_path())
    );
}

#[test]
fn project_skips_non_ryl_pyproject_and_uses_parent_toml() {
    let env = FakeEnv::new()
        .with_cwd(PathBuf::from("/repo"))
        .with_file(
            PathBuf::from("/repo/sub/pyproject.toml"),
            "[project]\nname = 'sub'\nversion = '0.1.0'\n",
        )
        .with_file(
            PathBuf::from("/repo/.ryl.toml"),
            "locale = 'fr_FR.UTF-8'\n[rules]\nanchors = 'disable'\n",
        )
        .with_exists(PathBuf::from("/repo/sub/file.yaml"));
    let ctx = discover_config_with(
        &[PathBuf::from("/repo/sub/file.yaml")],
        &Overrides::default(),
        &env,
    )
    .expect("ancestor TOML should load");
    assert_eq!(ctx.source.as_deref(), Some(Path::new("/repo/.ryl.toml")));
    assert_eq!(ctx.config.locale(), Some("fr_FR.UTF-8"));
}

#[test]
fn project_pyproject_with_tool_ryl_is_used() {
    let env = FakeEnv::new()
        .with_cwd(PathBuf::from("/repo"))
        .with_file(
            PathBuf::from("/repo/pyproject.toml"),
            "[project]\nname = 'demo'\nversion = '0.1.0'\n[tool.ryl]\nlocale = 'de_DE.UTF-8'\n",
        )
        .with_exists(PathBuf::from("/repo/file.yaml"));
    let ctx = discover_config_with(
        &[PathBuf::from("/repo/file.yaml")],
        &Overrides::default(),
        &env,
    )
    .expect("pyproject [tool.ryl] should load");
    assert_eq!(
        ctx.source.as_deref(),
        Some(Path::new("/repo/pyproject.toml"))
    );
    assert_eq!(ctx.config.locale(), Some("de_DE.UTF-8"));
}

#[test]
fn explicit_pyproject_requires_tool_ryl_section() {
    let pyproject = PathBuf::from("/repo/pyproject.toml");
    let env = FakeEnv::new().with_cwd(PathBuf::from("/repo")).with_file(
        pyproject.clone(),
        "[project]\nname = 'demo'\nversion = '0.1.0'\n",
    );
    let err = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(pyproject),
            config_data: None,
        },
        &env,
    )
    .unwrap_err();
    assert!(err.contains("missing [tool.ryl] section"));
}

#[test]
fn toml_config_rejects_extends_key() {
    let cfg = PathBuf::from("/repo/.ryl.toml");
    let env = FakeEnv::new()
        .with_cwd(PathBuf::from("/repo"))
        .with_file(cfg.clone(), "extends = 'relaxed'\n");
    let err = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(cfg),
            config_data: None,
        },
        &env,
    )
    .unwrap_err();
    assert!(err.contains("extends is not supported in TOML configuration"));
}

#[test]
fn explicit_toml_parse_error_is_reported() {
    let cfg = PathBuf::from("/repo/.ryl.toml");
    let env = FakeEnv::new()
        .with_cwd(PathBuf::from("/repo"))
        .with_file(cfg.clone(), "rules = [\n");
    let err = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(cfg),
            config_data: None,
        },
        &env,
    )
    .unwrap_err();
    assert!(err.contains("failed to parse config data"));
}

#[test]
fn toml_scalar_typed_unknown_keys_are_rejected() {
    // Unknown top-level keys are rejected regardless of TOML value type; the bool, float,
    // and datetime values here also exercise non-string entries in `extra` and the
    // multi-key listing in the error.
    let cfg = PathBuf::from("/repo/.ryl.toml");
    let env = FakeEnv::new().with_cwd(PathBuf::from("/repo")).with_file(
        cfg.clone(),
        "flag = true\nratio = 1.5\nstamp = 1979-05-27T07:32:00Z\n[rules]\nanchors = 'disable'\n",
    );
    let err = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(cfg),
            config_data: None,
        },
        &env,
    )
    .expect_err("unrecognised top-level keys must be rejected");
    assert!(
        err.contains("unrecognised TOML configuration key")
            && err.contains("`flag`")
            && err.contains("`ratio`")
            && err.contains("`stamp`"),
        "error should name all unknown keys: {err}"
    );
}

#[test]
fn toml_integer_unknown_key_is_rejected() {
    let cfg = PathBuf::from("/repo/.ryl.toml");
    let env = FakeEnv::new()
        .with_cwd(PathBuf::from("/repo"))
        .with_file(cfg.clone(), "answer = 42\n[rules]\nanchors = 'disable'\n");
    let err = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(cfg),
            config_data: None,
        },
        &env,
    )
    .expect_err("an unrecognised integer-valued top-level key must be rejected");
    assert!(
        err.contains("unrecognised TOML configuration key") && err.contains("`answer`"),
        "error should name the unknown key: {err}"
    );
}

#[test]
fn scalar_rules_value_is_rejected() {
    let cfg = PathBuf::from("/repo/.ryl.toml");
    let env = FakeEnv::new()
        .with_cwd(PathBuf::from("/repo"))
        .with_file(cfg.clone(), "rules = 1\n");
    let err = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(cfg),
            config_data: None,
        },
        &env,
    )
    .expect_err("scalar rules value should be rejected");

    assert!(err.contains("failed to parse config data"));
}

#[test]
fn scalar_tool_ryl_pyproject_is_rejected() {
    let pyproject = PathBuf::from("/repo/pyproject.toml");
    let env = FakeEnv::new()
        .with_cwd(PathBuf::from("/repo"))
        .with_file(pyproject.clone(), "[tool]\nryl = 1\n");
    let err = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(pyproject),
            config_data: None,
        },
        &env,
    )
    .expect_err("scalar [tool.ryl] should be rejected");

    assert!(err.contains("failed to parse config data"));
}

#[test]
fn exact_typed_toml_supports_single_string_ignore_from_file() {
    let td = tempdir().unwrap();
    let root = td.path();
    std::fs::write(root.join(".ignore-list"), "build/**\n").unwrap();
    std::fs::write(
        root.join(".ryl.toml"),
        "ignore-from-file = '.ignore-list'\n",
    )
    .unwrap();
    std::fs::write(root.join("file.yaml"), "a: 1\n").unwrap();

    let ctx = discover_per_file(&root.join("file.yaml"))
        .expect("typed TOML ignore-from-file should load");
    let rendered = ctx.config.to_toml_string();

    assert!(rendered.contains("ignore-from-file = ["));
    assert!(rendered.contains(".ignore-list"));
}

#[test]
fn exact_typed_toml_splits_multiline_scalar_ignore_patterns() {
    let td = tempdir().unwrap();
    let root = td.path();
    std::fs::write(
        root.join(".ryl.toml"),
        "ignore = \"\"\"\nvendor/**\ngenerated/**\n\"\"\"\n",
    )
    .unwrap();
    std::fs::write(root.join("file.yaml"), "a: 1\n").unwrap();

    let ctx = discover_per_file(&root.join("file.yaml"))
        .expect("typed TOML multiline ignore should load");

    assert_eq!(
        ctx.config
            .ignore_patterns()
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        vec!["vendor/**", "generated/**"]
    );
    assert!(
        ctx.config
            .is_file_ignored(&root.join("vendor/data.yaml"), root)
    );
    assert!(
        ctx.config
            .is_file_ignored(&root.join("generated/data.yaml"), root)
    );
}

#[test]
fn toml_ignore_and_ignore_from_file_conflict_errors() {
    let cfg = PathBuf::from("/repo/.ryl.toml");
    let env = FakeEnv::new().with_cwd(PathBuf::from("/repo")).with_file(
        cfg.clone(),
        "ignore = ['vendor/**']\nignore-from-file = ['.ignore-list']\n",
    );
    let err = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(cfg),
            config_data: None,
        },
        &env,
    )
    .expect_err("conflicting top-level ignore settings should fail");

    assert_eq!(
        err,
        "invalid config: ignore and ignore-from-file keys cannot be used together"
    );
}

#[test]
fn toml_quoted_strings_conflict_errors() {
    let cfg = PathBuf::from("/repo/.ryl.toml");
    let env = FakeEnv::new().with_cwd(PathBuf::from("/repo")).with_file(
        cfg.clone(),
        "[rules.quoted-strings]\nextra-required = ['^http']\n",
    );
    let err = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(cfg),
            config_data: None,
        },
        &env,
    )
    .expect_err("conflicting quoted-strings TOML settings should fail");

    assert_eq!(
        err,
        "invalid config: quoted-strings: cannot use both \"required: true\" and \"extra-required\""
    );
}

#[test]
fn toml_float_value_for_locale_reports_string_error() {
    let cfg = PathBuf::from("/repo/.ryl.toml");
    let env = FakeEnv::new()
        .with_cwd(PathBuf::from("/repo"))
        .with_file(cfg.clone(), "locale = 1.25\n");
    let err = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(cfg),
            config_data: None,
        },
        &env,
    )
    .unwrap_err();
    assert!(err.contains("failed to parse config data"));
}

#[test]
fn invalid_project_pyproject_toml_errors_in_discover_config() {
    let env = FakeEnv::new()
        .with_cwd(PathBuf::from("/repo"))
        .with_file(PathBuf::from("/repo/pyproject.toml"), "[tool.ryl\n")
        .with_exists(PathBuf::from("/repo/file.yaml"));
    let err = discover_config_with(
        &[PathBuf::from("/repo/file.yaml")],
        &Overrides::default(),
        &env,
    )
    .unwrap_err();
    assert!(err.contains("failed to parse config data"));
}

#[test]
fn invalid_project_pyproject_toml_errors_in_discover_per_file() {
    let td = tempdir().unwrap();
    let root = td.path();
    std::fs::write(root.join("pyproject.toml"), "[tool.ryl\n").unwrap();
    std::fs::write(root.join("file.yaml"), "a: 1\n").unwrap();
    let err = discover_per_file(&root.join("file.yaml")).unwrap_err();
    assert!(err.contains("failed to parse config data"));
}

#[test]
fn explicit_invalid_pyproject_toml_reports_parse_error() {
    let pyproject = PathBuf::from("/repo/pyproject.toml");
    let env = FakeEnv::new()
        .with_cwd(PathBuf::from("/repo"))
        .with_file(pyproject.clone(), "[tool.ryl\n");
    let err = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(pyproject),
            config_data: None,
        },
        &env,
    )
    .unwrap_err();
    assert!(err.contains("failed to parse config data"));
}

#[test]
fn config_dir_plain_toml_is_discovered() {
    let env = FakeEnv::new()
        .with_cwd(PathBuf::from("/repo"))
        .with_file(
            PathBuf::from("/repo/.config/ryl.toml"),
            "locale = 'fr_FR.UTF-8'\n[rules]\nanchors = 'disable'\n",
        )
        .with_exists(PathBuf::from("/repo/file.yaml"));
    let ctx = discover_config_with(
        &[PathBuf::from("/repo/file.yaml")],
        &Overrides::default(),
        &env,
    )
    .expect("`.config/ryl.toml` should be discovered");
    assert_eq!(
        ctx.source.as_deref(),
        Some(Path::new("/repo/.config/ryl.toml"))
    );
    assert_eq!(ctx.config.locale(), Some("fr_FR.UTF-8"));
}

#[test]
fn config_dir_dotted_toml_is_discovered() {
    let env = FakeEnv::new()
        .with_cwd(PathBuf::from("/repo"))
        .with_file(
            PathBuf::from("/repo/.config/.ryl.toml"),
            "locale = 'de_DE.UTF-8'\n[rules]\nanchors = 'disable'\n",
        )
        .with_exists(PathBuf::from("/repo/file.yaml"));
    let ctx = discover_config_with(
        &[PathBuf::from("/repo/file.yaml")],
        &Overrides::default(),
        &env,
    )
    .expect("`.config/.ryl.toml` should be discovered");
    assert_eq!(
        ctx.source.as_deref(),
        Some(Path::new("/repo/.config/.ryl.toml"))
    );
    assert_eq!(ctx.config.locale(), Some("de_DE.UTF-8"));
}

#[test]
fn config_dir_dotted_beats_plain() {
    // Within `.config/`, the hidden name wins, mirroring the root-level `.ryl.toml` >
    // `ryl.toml` ordering so the dotted/plain precedence is uniform across both dirs.
    let env = FakeEnv::new()
        .with_cwd(PathBuf::from("/repo"))
        .with_file(
            PathBuf::from("/repo/.config/.ryl.toml"),
            "locale = 'fr_FR.UTF-8'\n[rules]\nanchors = 'disable'\n",
        )
        .with_file(
            PathBuf::from("/repo/.config/ryl.toml"),
            "locale = 'de_DE.UTF-8'\n[rules]\nanchors = 'disable'\n",
        )
        .with_exists(PathBuf::from("/repo/file.yaml"));
    let ctx = discover_config_with(
        &[PathBuf::from("/repo/file.yaml")],
        &Overrides::default(),
        &env,
    )
    .expect("dotted config-dir name should win");
    assert_eq!(
        ctx.source.as_deref(),
        Some(Path::new("/repo/.config/.ryl.toml"))
    );
    assert_eq!(ctx.config.locale(), Some("fr_FR.UTF-8"));
}

#[test]
fn root_toml_beats_config_dir() {
    // A root-level `ryl.toml` outranks any `.config/` variant in the same directory.
    let env = FakeEnv::new()
        .with_cwd(PathBuf::from("/repo"))
        .with_file(
            PathBuf::from("/repo/ryl.toml"),
            "locale = 'fr_FR.UTF-8'\n[rules]\nanchors = 'disable'\n",
        )
        .with_file(
            PathBuf::from("/repo/.config/.ryl.toml"),
            "locale = 'de_DE.UTF-8'\n[rules]\nanchors = 'disable'\n",
        )
        .with_exists(PathBuf::from("/repo/file.yaml"));
    let ctx = discover_config_with(
        &[PathBuf::from("/repo/file.yaml")],
        &Overrides::default(),
        &env,
    )
    .expect("root `ryl.toml` should win over `.config/`");
    assert_eq!(ctx.source.as_deref(), Some(Path::new("/repo/ryl.toml")));
    assert_eq!(ctx.config.locale(), Some("fr_FR.UTF-8"));
}

#[test]
fn config_dir_beats_pyproject() {
    // `.config/ryl.toml` outranks a `pyproject.toml [tool.ryl]` in the same directory.
    let env = FakeEnv::new()
        .with_cwd(PathBuf::from("/repo"))
        .with_file(
            PathBuf::from("/repo/.config/ryl.toml"),
            "locale = 'fr_FR.UTF-8'\n[rules]\nanchors = 'disable'\n",
        )
        .with_file(
            PathBuf::from("/repo/pyproject.toml"),
            "[project]\nname = 'demo'\nversion = '0.1.0'\n[tool.ryl]\nlocale = 'de_DE.UTF-8'\n",
        )
        .with_exists(PathBuf::from("/repo/file.yaml"));
    let ctx = discover_config_with(
        &[PathBuf::from("/repo/file.yaml")],
        &Overrides::default(),
        &env,
    )
    .expect("`.config/ryl.toml` should win over pyproject");
    assert_eq!(
        ctx.source.as_deref(),
        Some(Path::new("/repo/.config/ryl.toml"))
    );
    assert_eq!(ctx.config.locale(), Some("fr_FR.UTF-8"));
}

#[test]
fn config_dir_resolves_from_ancestor() {
    // The `.config/` candidate is checked at every ancestor, so a config-dir config at
    // the repo root resolves for a file nested several directories deep.
    let env = FakeEnv::new()
        .with_cwd(PathBuf::from("/repo"))
        .with_var("HOME", "/repo")
        .with_file(
            PathBuf::from("/repo/.config/ryl.toml"),
            "locale = 'fr_FR.UTF-8'\n[rules]\nanchors = 'disable'\n",
        )
        .with_exists(PathBuf::from("/repo/sub/deep/file.yaml"));
    let ctx = discover_config_with(
        &[PathBuf::from("/repo/sub/deep/file.yaml")],
        &Overrides::default(),
        &env,
    )
    .expect("ancestor `.config/ryl.toml` should resolve");
    assert_eq!(
        ctx.source.as_deref(),
        Some(Path::new("/repo/.config/ryl.toml"))
    );
}

#[test]
fn config_dir_does_not_discover_legacy_yaml() {
    // `.config/` holds ryl-native TOML only; a `.config/.yamllint` is invisible to
    // discovery (the legacy YAML fallback only checks root-level yamllint names).
    let env = FakeEnv::new()
        .with_cwd(PathBuf::from("/repo"))
        .with_var("HOME", "/repo")
        .with_file(
            PathBuf::from("/repo/.config/.yamllint"),
            "locale: en_US.UTF-8\nrules: {}\n",
        )
        .with_exists(PathBuf::from("/repo/file.yaml"));
    let ctx = discover_config_with(
        &[PathBuf::from("/repo/file.yaml")],
        &Overrides::default(),
        &env,
    )
    .expect("discovery should succeed with no config found");
    assert_eq!(
        ctx.source, None,
        "`.config/.yamllint` must not be discovered as a config source"
    );
}

#[test]
fn config_dir_anchors_path_globs_at_project_root() {
    // A `.config/ryl.toml` must anchor path-based `[files]` globs at the project root
    // (`.config/`'s parent), not at `.config/`, so it is a true drop-in for a root
    // config; with the wrong base, `configs/**/*.yaml` would never match the repo's
    // `configs/` and discovery would silently lint nothing.
    let env = FakeEnv::new()
        .with_cwd(PathBuf::from("/repo"))
        .with_var("HOME", "/repo")
        .with_file(
            PathBuf::from("/repo/.config/ryl.toml"),
            "[files]\nyaml = ['configs/**/*.yaml']\n[rules]\ntrailing-spaces = 'enable'\n",
        )
        .with_exists(PathBuf::from("/repo/file.yaml"));
    let ctx = discover_config_with(
        &[PathBuf::from("/repo/file.yaml")],
        &Overrides::default(),
        &env,
    )
    .expect("`.config/ryl.toml` should be discovered");
    assert_eq!(ctx.base_dir, Path::new("/repo"));
    assert!(
        ctx.config
            .is_yaml_candidate(Path::new("/repo/configs/app.yaml"), &ctx.base_dir),
        "path glob must match relative to the project root, not `.config/`"
    );
}

#[test]
fn config_dir_explicit_c_anchors_at_project_root() {
    // The `.config/` anchoring applies to an explicit `-c` too (the "all routes"
    // choice), so `-c .config/ryl.toml` is not a silent dead end for path-based globs.
    let cfg = PathBuf::from("/repo/.config/ryl.toml");
    let env = FakeEnv::new().with_cwd(PathBuf::from("/repo")).with_file(
        cfg.clone(),
        "[files]\nyaml = ['configs/**/*.yaml']\n[rules]\ntrailing-spaces = 'enable'\n",
    );
    let ctx = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(cfg),
            config_data: None,
        },
        &env,
    )
    .expect("explicit `-c .config/ryl.toml` should load");
    assert_eq!(ctx.base_dir, Path::new("/repo"));
    assert!(
        ctx.config
            .is_yaml_candidate(Path::new("/repo/configs/app.yaml"), &ctx.base_dir),
        "explicit `-c` config in `.config/` must anchor globs at the project root"
    );
}

#[test]
fn config_dir_relative_explicit_c_anchors_at_cwd() {
    // A relative `-c .config/ryl.toml` has an empty grandparent, so the project root
    // resolves to the cwd (the directory the relative `.config/` sits in).
    let env = FakeEnv::new().with_cwd(PathBuf::from("/repo")).with_file(
        PathBuf::from(".config/ryl.toml"),
        "[files]\nyaml = ['configs/**/*.yaml']\n[rules]\ntrailing-spaces = 'enable'\n",
    );
    let ctx = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(PathBuf::from(".config/ryl.toml")),
            config_data: None,
        },
        &env,
    )
    .expect("relative explicit `-c .config/ryl.toml` should load");
    assert_eq!(ctx.base_dir, Path::new("/repo"));
}

#[test]
fn config_dir_non_candidate_configs_keep_parent_base() {
    // Only the `.config/` discovery candidate names (`.ryl.toml`/`ryl.toml`) re-anchor.
    // An arbitrarily-named TOML or a yamllint-compat YAML config explicitly pointed at
    // inside a `.config/` folder keeps its parent base, like any other `-c <path>` (so
    // explicit mode never re-bases names discovery can't find, and yamllint-compat YAML
    // semantics, including relative extends/ignore-from-file, stay intact).
    for (name, body) in [
        (
            "custom.toml",
            "[files]\nyaml = ['configs/**/*.yaml']\n[rules]\ntrailing-spaces = 'enable'\n",
        ),
        ("custom.yaml", "locale: en_US.UTF-8\nrules: {}\n"),
    ] {
        let cfg = PathBuf::from("/repo/.config").join(name);
        let env = FakeEnv::new()
            .with_cwd(PathBuf::from("/repo"))
            .with_file(cfg.clone(), body);
        let ctx = discover_config_with(
            &[],
            &Overrides {
                config_file: Some(cfg),
                config_data: None,
            },
            &env,
        )
        .expect("explicit non-candidate config in `.config/` should load");
        assert_eq!(
            ctx.base_dir,
            Path::new("/repo/.config"),
            "non-candidate `{name}` in `.config/` must keep its parent base"
        );
    }
}

#[test]
fn yaml_extends_toml_is_rejected() {
    let cfg = PathBuf::from("/repo/.yamllint");
    let env = FakeEnv::new()
        .with_cwd(PathBuf::from("/repo"))
        .with_file(cfg.clone(), "extends: .ryl.toml\n")
        .with_file(PathBuf::from("/repo/.ryl.toml"), "locale = 'en_US.UTF-8'\n");
    let err = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(cfg),
            config_data: None,
        },
        &env,
    )
    .unwrap_err();
    assert!(err.contains("extends cannot reference TOML configuration"));
}
