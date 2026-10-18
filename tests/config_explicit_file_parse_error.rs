use std::path::PathBuf;

mod common;
use common::fake_env::FakeEnv;

#[test]
fn explicit_config_file_with_invalid_yaml_errors() {
    let file = PathBuf::from("cfg.yml");
    let envx = FakeEnv::new().with_file(file.clone(), "rules: {");

    let overrides = ryl::config::Overrides {
        config_file: Some(file),
        config_data: None,
    };
    let res = ryl::config::discover_config_with(&[], &overrides, &envx);
    assert!(res.is_err());
}
