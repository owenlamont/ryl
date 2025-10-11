use ryl::config::{Overrides, discover_config};

#[test]
fn invalid_yaml_data_errors() {
    let err = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some("[1]".into()),
        },
    )
    .expect_err("top-level sequence should error");
    assert!(err.contains("not a mapping"));
}

#[test]
fn yaml_files_non_sequence_errors() {
    let err = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some("yaml-files: 5\n".into()),
        },
    )
    .expect_err("non-list yaml-files should error");
    assert!(err.contains("yaml-files should be a list"));
}

#[test]
fn yaml_files_invalid_pattern_is_skipped() {
    let ctx = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some("yaml-files: ['[']\n".into()),
        },
    )
    .expect("invalid glob should be ignored");
    let base = ctx.base_dir.clone();
    assert!(!ctx.config.is_yaml_candidate(&base.join("file.yaml"), &base));
}

#[test]
fn yaml_files_negation_excludes_matches() {
    let ctx = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some("yaml-files: ['*.yaml', '!skip.yaml']\n".into()),
        },
    )
    .expect("yaml-files with negation should parse");
    let base = ctx.base_dir.clone();
    assert!(
        ctx.config.is_yaml_candidate(&base.join("keep.yaml"), &base),
        "positive pattern should include keep.yaml"
    );
    assert!(
        !ctx.config.is_yaml_candidate(&base.join("skip.yaml"), &base),
        "negated pattern should exclude skip.yaml"
    );
}
