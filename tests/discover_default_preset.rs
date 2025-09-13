use ryl::config::{Overrides, discover_config};

#[test]
fn default_when_no_configs_still_filters_by_extension() {
    let ctx = discover_config(&[], &Overrides::default()).expect("default loaded");
    assert!(
        ctx.config
            .is_yaml_candidate(&std::path::PathBuf::from("x.yaml"), &ctx.base_dir)
    );
    assert!(
        !ctx.config
            .is_yaml_candidate(&std::path::PathBuf::from("x.txt"), &ctx.base_dir)
    );
}
