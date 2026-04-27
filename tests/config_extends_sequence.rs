use ryl::config::{Overrides, discover_config};

#[test]
fn extends_sequence_errors() {
    let cfg = "extends: [default, relaxed]\n";
    let err = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(cfg.into()),
        },
    )
    .expect_err("extends sequence should error");
    assert!(err.contains("failed to parse config data:"), "{err}");
    assert!(err.contains("extends"), "{err}");
}
