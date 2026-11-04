use std::fs;

use ryl::config::YamlLintConfig;
use tempfile::tempdir;

#[test]
fn file_entrypoints_keep_alias_suppression_and_markdown_positions() {
    let dir = tempdir().unwrap();
    let cfg = YamlLintConfig::from_toml_str("[lint.rules.colons]").unwrap();
    let content = "a:  1  # yamllint disable-line rule:colons\nb:  2\n";
    let yaml = dir.path().join("a.yaml");
    let markdown = dir.path().join("a.md");
    fs::write(&yaml, content).unwrap();
    fs::write(&markdown, format!("heading\n```yaml\n{content}```\n")).unwrap();
    let yaml_problems = ryl::lint_file(&yaml, &cfg, dir.path()).unwrap();
    let markdown_problems =
        ryl::lint_markdown_file(&markdown, &cfg, dir.path()).unwrap();
    assert_eq!(yaml_problems.len(), 1);
    assert_eq!(markdown_problems.len(), 1);
    assert_eq!(
        (yaml_problems[0].line, yaml_problems[0].rule),
        (2, Some("colons"))
    );
    assert_eq!(
        (markdown_problems[0].line, markdown_problems[0].rule),
        (4, Some("colons"))
    );
}

#[test]
fn yaml_file_read_errors_remain_errors_without_diagnostics() {
    let dir = tempdir().unwrap();
    let missing = dir.path().join("missing.yaml");
    let error =
        ryl::lint_file(&missing, &YamlLintConfig::default(), dir.path()).unwrap_err();
    assert!(error.contains("failed to read"), "{error}");
    assert!(error.contains("missing.yaml"), "{error}");
}
