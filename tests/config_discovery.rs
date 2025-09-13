use std::fs;
use std::path::PathBuf;

use ryl::config::{Overrides, discover_config, discover_config_with_env};
use tempfile::tempdir;

fn write(path: &PathBuf, content: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

#[test]
fn inline_config_takes_precedence_over_file() {
    let td = tempdir().unwrap();
    let proj = td.path().join("proj");
    fs::create_dir_all(&proj).unwrap();
    let proj_cfg = proj.join(".yamllint");
    write(&proj_cfg, "ignore:\n  - foo.txt\n");

    let inputs = vec![proj.clone()];
    let ctx = discover_config(
        &inputs,
        &Overrides {
            config_file: None,
            config_data: Some("ignore: ['bar.txt']".into()),
        },
    )
    .unwrap();

    assert_eq!(ctx.config.ignore_patterns(), &[String::from("bar.txt")]);
    assert!(ctx.source.is_none());
}

#[test]
fn env_config_used_when_no_project_config_via_injected_env() {
    let td = tempdir().unwrap();
    let cfg = td.path().join("yamllint.yaml");
    write(&cfg, "ignore: ['**/generated/**']\n");

    let inputs = vec![td.path().to_path_buf()];
    let ctx = discover_config_with_env(&inputs, &Overrides::default(), |k| {
        if k == "YAMLLINT_CONFIG_FILE" {
            Some(cfg.display().to_string())
        } else {
            None
        }
    })
    .unwrap();
    assert!(
        ctx.config
            .is_file_ignored(&td.path().join("a/generated/x.yaml"), &ctx.base_dir)
    );
}

#[test]
fn is_file_ignored_matches_relative_patterns() {
    let td = tempdir().unwrap();
    let proj = td.path().join("proj");
    fs::create_dir_all(&proj).unwrap();
    let proj_cfg = proj.join(".yamllint.yml");
    write(&proj_cfg, "ignore: ['**/*.skip.yaml', 'docs/**']\n");

    let inputs = vec![proj.clone()];
    let ctx = discover_config(&inputs, &Overrides::default()).unwrap();

    assert!(
        ctx.config
            .is_file_ignored(&proj.join("a/b/test.skip.yaml"), &ctx.base_dir)
    );
    assert!(
        ctx.config
            .is_file_ignored(&proj.join("docs/guide.yaml"), &ctx.base_dir)
    );
    assert!(
        !ctx.config
            .is_file_ignored(&proj.join("src/ok.yaml"), &ctx.base_dir)
    );
}
