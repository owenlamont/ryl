mod common;

use std::path::PathBuf;

use ryl::config::{Overrides, discover_config_with};

fn config_from_toml(body: &str) -> ryl::config::YamlLintConfig {
    let path = PathBuf::from("/repo/.ryl.toml");
    let env = common::fake_env::FakeEnv::new().with_file(path.clone(), body);
    discover_config_with(
        &[],
        &Overrides {
            config_file: Some(path),
            config_data: None,
        },
        &env,
    )
    .expect("toml config should parse")
    .config
}

#[test]
fn markdown_sources_default_to_enabled() {
    let cfg = config_from_toml("markdown = { files = [\"*.md\"] }\n");
    assert!(cfg.markdown_front_matter());
    assert!(cfg.markdown_fenced_blocks());
}

#[test]
fn markdown_source_toggles_are_applied() {
    let cfg = config_from_toml(
        "markdown = { files = [\"*.md\"], front-matter = false, fenced-blocks = true }\n",
    );
    assert!(!cfg.markdown_front_matter());
    assert!(cfg.markdown_fenced_blocks());
}

#[test]
fn markdown_candidate_matches_only_with_files_pattern() {
    let base = PathBuf::from("/repo");
    let doc = PathBuf::from("/repo/readme.md");

    let enabled = config_from_toml("markdown = { files = [\"*.md\"] }\n");
    assert!(enabled.is_markdown_candidate(&doc, &base));
    let outside_base = PathBuf::from("/elsewhere/notes.md");
    assert!(enabled.is_markdown_candidate(&outside_base, &base));

    let disabled = config_from_toml("[rules]\ncolons = \"enable\"\n");
    assert!(!disabled.is_markdown_candidate(&doc, &base));
}

#[test]
fn to_toml_string_round_trips_markdown_settings() {
    let cfg =
        config_from_toml("markdown = { files = [\"*.md\"], front-matter = false }\n");
    let rendered = cfg.to_toml_string();

    assert!(rendered.contains("[markdown]"), "{rendered}");
    assert!(rendered.contains("files"), "{rendered}");
    assert!(rendered.contains("front-matter = false"), "{rendered}");
    assert!(rendered.contains("fenced-blocks = true"), "{rendered}");
}
