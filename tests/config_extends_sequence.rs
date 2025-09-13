use ryl::config::{Overrides, discover_config};

#[test]
fn extends_sequence_merges_presets_in_order() {
    let cfg = "extends: [default, relaxed]\n";
    let ctx = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(cfg.into()),
        },
    )
    .expect("parse");
    // From default
    assert!(ctx.config.rule_names().contains(&"anchors".to_string()));
    // From relaxed override
    assert!(ctx.config.rule_names().contains(&"braces".to_string()));
}
