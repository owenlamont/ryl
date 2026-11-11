use ryl::config::{Overrides, YamlLintConfig, discover_config};

pub fn load(table: &str) -> YamlLintConfig {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join(".ryl.toml");
    std::fs::write(&file, table).unwrap();
    discover_config(
        &[],
        &Overrides {
            config_file: Some(file),
            config_data: None,
        },
    )
    .unwrap()
    .config
}
