use std::fs;
use std::process::Command;

use tempfile::tempdir;

fn run(cmd: &mut Command) -> (i32, String, String) {
    let out = cmd.output().expect("process");
    let code = out.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (code, stdout, stderr)
}

fn project(
    config: &str,
    name: &str,
    body: &str,
) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".ryl.toml"), config).unwrap();
    let file = dir.path().join(name);
    fs::write(&file, body).unwrap();
    (dir, file)
}

const COLONS_AND_DUPES: &str = "files = { markdown = [\"*.md\"] }\n[rules]\ncolons = \"enable\"\nkey-duplicates = \"enable\"\n";
const COLONS_ONLY: &str =
    "files = { markdown = [\"*.md\"] }\n[rules]\ncolons = \"enable\"\n";

#[test]
fn front_matter_and_fenced_blocks_map_to_host_positions() {
    let body = "---\ntitle:  hello\nduplicate: 1\nduplicate: 2\n---\n\n```yaml\nfoo:  bar\n```\n";
    let (_dir, file) = project(COLONS_AND_DUPES, "doc.md", body);

    let (code, _out, err) = run(Command::new(env!("CARGO_BIN_EXE_ryl")).arg(&file));

    assert_eq!(code, 1, "stderr={err}");
    assert!(err.contains("2:8") && err.contains("colons"), "{err}");
    assert!(
        err.contains("4:1") && err.contains("key-duplicates"),
        "{err}"
    );
    assert!(err.contains("8:6"), "fenced block colon position: {err}");
}

#[test]
fn indented_fenced_block_adds_indent_to_column() {
    let body = "-  item\n\n   ```yaml\n   key:  value\n   ```\n";
    let (_dir, file) = project(COLONS_ONLY, "doc.md", body);

    let (code, _out, err) = run(Command::new(env!("CARGO_BIN_EXE_ryl")).arg(&file));

    assert_eq!(code, 1, "stderr={err}");
    assert!(
        err.contains("4:9"),
        "indent should shift column to 9: {err}"
    );
}

#[test]
fn front_matter_only_source_skips_fenced_blocks() {
    let config = "files = { markdown = [\"*.md\"] }\nmarkdown = { fenced-blocks = false }\n[rules]\ncolons = \"enable\"\n";
    let body = "---\na:  1\n---\n\n```yaml\nb:  2\n```\n";
    let (_dir, file) = project(config, "doc.md", body);

    let (code, _out, err) = run(Command::new(env!("CARGO_BIN_EXE_ryl")).arg(&file));

    assert_eq!(code, 1, "stderr={err}");
    assert!(err.contains("2:4"), "front matter linted: {err}");
    assert!(!err.contains("6:4"), "fenced block must be skipped: {err}");
}

#[test]
fn fenced_blocks_only_source_skips_front_matter() {
    let config = "files = { markdown = [\"*.md\"] }\nmarkdown = { front-matter = false }\n[rules]\ncolons = \"enable\"\n";
    let body = "---\na:  1\n---\n\n```yaml\nb:  2\n```\n";
    let (_dir, file) = project(config, "doc.md", body);

    let (code, _out, err) = run(Command::new(env!("CARGO_BIN_EXE_ryl")).arg(&file));

    assert_eq!(code, 1, "stderr={err}");
    assert!(err.contains("6:4"), "fenced block linted: {err}");
    assert!(!err.contains("2:4"), "front matter must be skipped: {err}");
}

#[test]
fn file_shape_rules_are_suppressed_in_embedded_regions() {
    let config = "files = { markdown = [\"*.md\"] }\n[rules]\ndocument-start = \"enable\"\ncolons = \"enable\"\n";
    let body = "---\na:  1\n---\n\n```yaml\nb:  2\n```\n";
    let (_dir, file) = project(config, "doc.md", body);

    let (code, _out, err) = run(Command::new(env!("CARGO_BIN_EXE_ryl")).arg(&file));

    assert_eq!(code, 1, "stderr={err}");
    assert!(err.contains("colons"), "{err}");
    assert!(
        !err.contains("document-start"),
        "document-start must be suppressed: {err}"
    );
}

#[test]
fn crlf_markdown_maps_positions() {
    let body = "# t\r\n\r\n```yaml\r\nfoo:  bar\r\n```\r\n";
    let (_dir, file) = project(COLONS_ONLY, "doc.md", body);

    let (code, _out, err) = run(Command::new(env!("CARGO_BIN_EXE_ryl")).arg(&file));

    assert_eq!(code, 1, "stderr={err}");
    assert!(err.contains("4:6"), "{err}");
}

#[test]
fn multibyte_front_matter_columns_pass_through() {
    let body = "---\ncaf\u{e9}:  x\n---\n";
    let (_dir, file) = project(COLONS_ONLY, "doc.md", body);

    let (code, _out, err) = run(Command::new(env!("CARGO_BIN_EXE_ryl")).arg(&file));

    assert_eq!(code, 1, "stderr={err}");
    assert!(err.contains("2:7"), "{err}");
}

#[test]
fn non_yaml_fenced_block_is_ignored() {
    let body = "# t\n\n```python\nx =  1\n```\n";
    let (_dir, file) = project(COLONS_ONLY, "doc.md", body);

    let (code, out, err) = run(Command::new(env!("CARGO_BIN_EXE_ryl")).arg(&file));

    assert_eq!(code, 0, "stderr={err}");
    assert!(out.is_empty() && err.is_empty(), "out={out} err={err}");
}

#[test]
fn attribute_and_tilde_fences_are_linted() {
    let body = "```{.yaml}\na:  1\n```\n\n~~~yml\nb:  2\n~~~\n";
    let (_dir, file) = project(COLONS_ONLY, "doc.md", body);

    let (code, _out, err) = run(Command::new(env!("CARGO_BIN_EXE_ryl")).arg(&file));

    assert_eq!(code, 1, "stderr={err}");
    assert!(err.contains("2:4"), "attribute fence: {err}");
    assert!(err.contains("6:4"), "tilde fence: {err}");
}

#[test]
fn whitespace_only_front_matter_is_skipped() {
    let config =
        "files = { markdown = [\"*.md\"] }\n[rules]\ntrailing-spaces = \"enable\"\n";
    let body = "---\n   \n---\n";
    let (_dir, file) = project(config, "doc.md", body);

    let (code, out, err) = run(Command::new(env!("CARGO_BIN_EXE_ryl")).arg(&file));

    assert_eq!(code, 0, "stderr={err}");
    assert!(out.is_empty() && err.is_empty(), "out={out} err={err}");
}

#[test]
fn explicit_markdown_without_files_pattern_is_rejected() {
    let config = "[rules]\ncolons = \"enable\"\n";
    let body = "---\na:  1\n---\n";
    let (_dir, file) = project(config, "doc.md", body);

    let (code, _out, err) = run(Command::new(env!("CARGO_BIN_EXE_ryl")).arg(&file));

    assert_eq!(code, 2, "expected usage error: {err}");
    assert!(err.contains("no source kind matches"), "{err}");
}

#[test]
fn fix_rewrites_markdown_front_matter() {
    let config =
        "files = { markdown = [\"*.md\"] }\n[rules]\ntrailing-spaces = \"enable\"\n";
    let body = "---\nfoo: bar  \n---\n";
    let (_dir, file) = project(config, "doc.md", body);

    let (code, _out, err) = run(Command::new(env!("CARGO_BIN_EXE_ryl"))
        .arg("--fix")
        .arg(&file));

    assert_eq!(code, 0, "stderr={err}");
    assert!(!err.contains("does not modify markdown files"), "{err}");
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "---\nfoo: bar\n---\n",
        "trailing spaces in front matter must be fixed in place"
    );
}

#[test]
fn directory_scan_discovers_markdown() {
    let body = "---\na:  1\n---\n";
    let (dir, _file) = project(COLONS_ONLY, "doc.md", body);

    let (code, _out, err) =
        run(Command::new(env!("CARGO_BIN_EXE_ryl")).arg(dir.path()));

    assert_eq!(code, 1, "stderr={err}");
    assert!(err.contains("2:4"), "{err}");
}

#[test]
fn file_matching_two_kinds_is_a_hard_error() {
    let config = "files = { yaml = [\"*.md\"], markdown = [\"*.md\"] }\n[rules]\ncolons = \"enable\"\n";
    let body = "---\na:  1\n---\n";
    let (_dir, file) = project(config, "doc.md", body);

    let (code, _out, err) = run(Command::new(env!("CARGO_BIN_EXE_ryl")).arg(&file));

    assert_eq!(code, 2, "expected overlap error: {err}");
    assert!(err.contains("matches both"), "{err}");
}

#[test]
fn directory_scan_overlap_is_a_hard_error() {
    let config = "files = { yaml = [\"*.md\"], markdown = [\"*.md\"] }\n[rules]\ncolons = \"enable\"\n";
    let body = "---\na:  1\n---\n";
    let (dir, _file) = project(config, "doc.md", body);

    let (code, _out, err) =
        run(Command::new(env!("CARGO_BIN_EXE_ryl")).arg(dir.path()));

    assert_eq!(code, 2, "expected overlap error: {err}");
    assert!(err.contains("matches both"), "{err}");
}
