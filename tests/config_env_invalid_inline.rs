use ryl::config::{Overrides, discover_config_with_env};

#[test]
fn invalid_inline_data_errors_in_discover_config_with_env() {
    let inputs: Vec<std::path::PathBuf> = vec![];
    let res = discover_config_with_env(
        &inputs,
        &Overrides {
            config_file: None,
            config_data: Some("rules: {".into()),
        },
        &|_| None,
    );
    assert!(res.is_err());
}
