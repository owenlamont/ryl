use ryl::config::{Overrides, discover_config};

#[test]
fn default_preset_applies_when_no_configs() {
    let ctx = discover_config(&[], &Overrides::default()).expect("default preset loaded");
    assert!(!ctx.config.rule_names().is_empty());
    assert!(
        ctx.config
            .is_yaml_candidate(&std::path::PathBuf::from("x.yaml"), &ctx.base_dir)
    );
    assert!(
        !ctx.config
            .is_yaml_candidate(&std::path::PathBuf::from("x.txt"), &ctx.base_dir)
    );
}
