use std::path::PathBuf;

use ryl::config::{Overrides, discover_config, discover_config_with};

#[path = "common/mod.rs"]
mod common;
use common::fake_env::FakeEnv;

#[test]
fn ignore_and_ignore_from_file_conflict_errors() {
    let yaml = "ignore: ['a']\nignore-from-file: ['b']\n";
    let err = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(yaml.into()),
        },
    )
    .expect_err("conflicting ignore keys should error");
    assert!(err.contains("cannot be used together"));
}

#[test]
fn ignore_from_file_non_string_errors() {
    let yaml = "ignore-from-file: [1]\n";
    let err = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(yaml.into()),
        },
    )
    .expect_err("non-string ignore-from-file entries should fail");
    assert!(err.contains("ignore-from-file"));
}

#[test]
fn ignore_from_file_invalid_mapping_errors() {
    let yaml = "ignore-from-file: { bad: 1 }\n";
    let err = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(yaml.into()),
        },
    )
    .expect_err("mapping value should error");
    assert!(err.contains("ignore-from-file should contain"));
}

#[test]
fn ignore_patterns_non_string_errors() {
    let yaml = "ignore: [1]\n";
    let err = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(yaml.into()),
        },
    )
    .expect_err("non-string ignore pattern should error");
    assert!(err.contains("ignore should contain"));
}

#[test]
fn ignore_from_file_patterns_are_loaded() {
    let root = PathBuf::from("/workspace");
    let env = FakeEnv::new()
        .with_cwd(root.clone())
        .with_file(root.join("cfg.yaml"), "ignore-from-file: .gitignore\n")
        .with_exists(root.join("cfg.yaml"))
        .with_file(root.join(".gitignore"), "vendor/**\n")
        .with_exists(root.join(".gitignore"));

    let ctx = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(root.join("cfg.yaml")),
            config_data: None,
        },
        &env,
    )
    .expect("ignore-from-file should hydrate patterns");
    assert!(
        ctx.config
            .ignore_patterns()
            .iter()
            .any(|p| p == "vendor/**")
    );
}

#[test]
fn missing_ignore_from_file_errors() {
    let root = PathBuf::from("/workspace");
    let env = FakeEnv::new()
        .with_cwd(root.clone())
        .with_file(root.join("cfg.yaml"), "ignore-from-file: missing.txt\n")
        .with_exists(root.join("cfg.yaml"));

    let err = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(root.join("cfg.yaml")),
            config_data: None,
        },
        &env,
    )
    .expect_err("missing ignore file should error");
    assert!(err.contains("failed to read ignore-from-file"));
}

#[test]
fn invalid_ignore_from_file_pattern_errors() {
    let root = PathBuf::from("/workspace");
    let ignore = root.join("rules.ignore");
    let env = FakeEnv::new()
        .with_cwd(root.clone())
        .with_file(root.join("cfg.yaml"), "ignore-from-file: rules.ignore\n")
        .with_exists(root.join("cfg.yaml"))
        .with_file(ignore.clone(), "[\n")
        .with_exists(ignore);

    let err = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(root.join("cfg.yaml")),
            config_data: None,
        },
        &env,
    )
    .expect_err("invalid glob should bubble up");
    assert!(err.contains("invalid config: ignore-from-file pattern"));
}

#[test]
fn ignore_block_scalar_skips_blank_lines() {
    let yaml = "ignore: |\n  docs/**\n\n  build/\nrules: {}\n";
    let ctx = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(yaml.into()),
        },
    )
    .expect("block scalar ignore should parse");
    assert!(
        !ctx.config
            .ignore_patterns()
            .iter()
            .any(|p| p.trim().is_empty())
    );
}

#[test]
fn ignore_blank_entries_are_removed() {
    let yaml = "ignore: ['   ', 'tmp/**']\nrules: {}\n";
    let ctx = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(yaml.into()),
        },
    )
    .expect("blank ignore entries should be skipped");
    assert_eq!(ctx.config.ignore_patterns().len(), 1);
    assert_eq!(ctx.config.ignore_patterns()[0], "tmp/**");
}

#[test]
fn ignore_from_file_blank_lines_are_skipped() {
    let root = PathBuf::from("/workspace");
    let env = FakeEnv::new()
        .with_cwd(root.clone())
        .with_file(root.join("cfg.yaml"), "ignore-from-file: rules.ignore\n")
        .with_exists(root.join("cfg.yaml"))
        .with_file(root.join("rules.ignore"), "\nlogs/**\n\n")
        .with_exists(root.join("rules.ignore"));

    let ctx = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(root.join("cfg.yaml")),
            config_data: None,
        },
        &env,
    )
    .expect("blank lines in ignore-from-file should be ignored");
    assert!(ctx.config.ignore_patterns().iter().any(|p| p == "logs/**"));
}

#[test]
fn ignore_from_file_sequence_is_supported() {
    let root = PathBuf::from("/workspace");
    let env = FakeEnv::new()
        .with_cwd(root.clone())
        .with_file(
            root.join("cfg.yaml"),
            "ignore-from-file: ['a.ignore', 'b.ignore']\n",
        )
        .with_exists(root.join("cfg.yaml"))
        .with_file(root.join("a.ignore"), "a/**\n")
        .with_exists(root.join("a.ignore"))
        .with_file(root.join("b.ignore"), "b/**\n")
        .with_exists(root.join("b.ignore"));

    let ctx = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(root.join("cfg.yaml")),
            config_data: None,
        },
        &env,
    )
    .expect("sequence of ignore-from-file entries should parse");
    assert!(ctx.config.ignore_patterns().iter().any(|p| p == "a/**"));
    assert!(ctx.config.ignore_patterns().iter().any(|p| p == "b/**"));
}

#[test]
fn ignore_from_file_absolute_paths_are_supported() {
    let root = PathBuf::from("/workspace");
    let abs = PathBuf::from("/workspace/.abs-ignore");
    let env = FakeEnv::new()
        .with_cwd(root.clone())
        .with_file(
            root.join("cfg.yaml"),
            format!("ignore-from-file: {}\n", abs.display()),
        )
        .with_exists(root.join("cfg.yaml"))
        .with_file(abs.clone(), "abs/**\n")
        .with_exists(abs);

    let ctx = discover_config_with(
        &[],
        &Overrides {
            config_file: Some(root.join("cfg.yaml")),
            config_data: None,
        },
        &env,
    )
    .expect("absolute ignore-from-file paths should resolve");
    assert!(ctx.config.ignore_patterns().iter().any(|p| p == "abs/**"));
}
