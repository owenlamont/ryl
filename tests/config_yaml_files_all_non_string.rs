use ryl::config::{Overrides, discover_config};

#[test]
fn yaml_files_sequence_all_non_strings_are_ignored() {
    let yaml = "yaml-files: [1, 2]\nrules: {}\n";
    let _ = discover_config(
        &[],
        &Overrides {
            config_file: None,
            config_data: Some(yaml.into()),
        },
    )
    .expect("ok");
}
