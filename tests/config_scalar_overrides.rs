use ryl::config::{Overrides, discover_config};

#[test]
fn scalar_ignore_and_yaml_files_string_are_parsed() {
    let cfg = "ignore: 'docs/**'\nyaml-files: ['.yamllint.yml']\n";
    let ctx = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(cfg.into()),
        },
    )
    .expect("parse");
    let base = ctx.base_dir.clone();
    assert!(
        ctx.config
            .is_file_ignored(&base.join("docs/x.yaml"), &ctx.base_dir)
    );
    assert!(
        ctx.config
            .is_yaml_candidate(&base.join(".yamllint.yml"), &ctx.base_dir)
    );
}

// Note: saphyr treats some malformed YAML as BadValue without returning an error,
// so invalid inline config data may not cause a hard parse error. We avoid
// asserting on exit codes for inline parse failures here.
