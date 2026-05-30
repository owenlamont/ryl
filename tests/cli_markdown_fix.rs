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
    fs::write(&file, body.as_bytes()).unwrap();
    (dir, file)
}

fn fix(file: &std::path::Path) -> (i32, String, String) {
    run(Command::new(env!("CARGO_BIN_EXE_ryl"))
        .arg("--fix")
        .arg(file))
}

const TRAILING: &str =
    "files = { markdown = [\"*.md\"] }\n[rules]\ntrailing-spaces = \"enable\"\n";
const COMMAS: &str =
    "files = { markdown = [\"*.md\"] }\n[rules]\ncommas = \"enable\"\n";

#[test]
fn fix_rewrites_indented_fenced_block_preserving_indent() {
    let body = "- item\n\n  ```yaml\n  nums: [1,2,3]\n  ```\n";
    let (_dir, file) = project(COMMAS, "doc.md", body);

    let (code, _out, err) = fix(&file);

    assert_eq!(code, 0, "stderr={err}");
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "- item\n\n  ```yaml\n  nums: [1, 2, 3]\n  ```\n",
        "fenced block fixed and re-indented to the fence column"
    );
}

#[test]
fn fix_preserves_crlf_in_fenced_block() {
    let body = "# t\r\n\r\n```yaml\r\nnums: [1,2,3]\r\n```\r\n";
    let (_dir, file) = project(COMMAS, "doc.md", body);

    let (code, _out, err) = fix(&file);

    assert_eq!(code, 0, "stderr={err}");
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "# t\r\n\r\n```yaml\r\nnums: [1, 2, 3]\r\n```\r\n",
        "CRLF newlines must be preserved through the fix"
    );
}

#[test]
fn fix_handles_front_matter_and_multiple_fenced_blocks() {
    let body = "---\nfoo: [1,2]\n---\n\n```yaml\nbar: [3,4]\n```\n\nText\n\n```yaml\nbaz: [5,6]\n```\n";
    let (_dir, file) = project(COMMAS, "doc.md", body);

    let (code, _out, err) = fix(&file);

    assert_eq!(code, 0, "stderr={err}");
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "---\nfoo: [1, 2]\n---\n\n```yaml\nbar: [3, 4]\n```\n\nText\n\n```yaml\nbaz: [5, 6]\n```\n",
        "front matter and every fenced block must be fixed back-to-front"
    );
}

#[test]
fn fix_does_not_inject_document_start_or_final_newline() {
    let config = "files = { markdown = [\"*.md\"] }\n[rules]\ndocument-start = \"enable\"\ntrailing-spaces = \"enable\"\n";
    let body = "---\nfoo: bar  \n---\n\n```yaml\nbaz: qux  \n```\n";
    let (_dir, file) = project(config, "doc.md", body);

    let (code, _out, err) = fix(&file);

    assert_eq!(code, 0, "stderr={err}");
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "---\nfoo: bar\n---\n\n```yaml\nbaz: qux\n```\n",
        "file-shape rules must not inject --- or trailing newlines into fragments"
    );
}

#[test]
fn fix_skips_ragged_indent_block_but_reports() {
    let body = "Intro\n\n   ```yaml\n   a: [1,2 ]\n  b: 3\n   ```\n";
    let (_dir, file) = project(COMMAS, "doc.md", body);

    let (code, _out, err) = fix(&file);

    assert_eq!(code, 1, "stderr={err}");
    assert!(err.contains("4:10") && err.contains("commas"), "{err}");
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        body,
        "ragged-indent block must be left byte-identical"
    );
}

#[test]
fn fix_skips_tab_indented_block_but_reports() {
    let body = "- item\n\n\t```yaml\n\tnums: [1,2]\n\t```\n";
    let (_dir, file) = project(COMMAS, "doc.md", body);

    let (code, _out, err) = fix(&file);

    assert_eq!(code, 1, "stderr={err}");
    assert!(err.contains("commas"), "tab block must still report: {err}");
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        body,
        "tab-indented block must be left byte-identical"
    );
}

#[test]
fn fix_handles_unclosed_fence_at_eof() {
    let body = "# t\n\n```yaml\nnums: [1,2]";
    let (_dir, file) = project(COMMAS, "doc.md", body);

    let (code, _out, err) = fix(&file);

    assert_eq!(code, 0, "stderr={err}");
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "# t\n\n```yaml\nnums: [1, 2]",
        "unclosed fence is fixed without inventing a closing fence or newline"
    );
}

#[test]
fn fix_preserves_utf8_bom() {
    let body = "\u{feff}---\nfoo: bar  \n---\n";
    let (_dir, file) = project(TRAILING, "doc.md", body);

    let (code, _out, err) = fix(&file);

    assert_eq!(code, 0, "stderr={err}");
    let fixed = fs::read(&file).unwrap();
    assert_eq!(&fixed[..3], &[0xEF, 0xBB, 0xBF], "BOM must be preserved");
    assert_eq!(
        String::from_utf8(fixed).unwrap(),
        "\u{feff}---\nfoo: bar\n---\n",
        "content after the BOM must be fixed"
    );
}

#[test]
fn fix_is_idempotent() {
    let body = "---\nfoo: [1,2]\n---\n";
    let (_dir, file) = project(COMMAS, "doc.md", body);

    let (_c, _o, _e) = fix(&file);
    let once = fs::read_to_string(&file).unwrap();
    let (code, _out, err) = fix(&file);
    let twice = fs::read_to_string(&file).unwrap();

    assert_eq!(code, 0, "stderr={err}");
    assert_eq!(once, twice, "a second --fix must be a no-op");
    assert!(
        !err.contains("fixed"),
        "no fixes reported on the second run: {err}"
    );
}

#[test]
fn fix_reports_fixed_and_remaining_summary() {
    let config = "files = { markdown = [\"*.md\"] }\n[rules]\ncommas = \"enable\"\ncolons = \"enable\"\n";
    let body = "```yaml\nnums: [1,2]\nfoo:  bar\n```\n";
    let (_dir, file) = project(config, "doc.md", body);

    let (code, _out, err) = fix(&file);

    assert_eq!(code, 1, "stderr={err}");
    assert!(err.contains("(1 fixed, 1 remaining)"), "{err}");
}

#[test]
fn fix_leaves_clean_markdown_untouched() {
    let body = "---\nfoo: bar\n---\n";
    let (_dir, file) = project(COMMAS, "doc.md", body);

    let (code, _out, err) = fix(&file);

    assert_eq!(code, 0, "stderr={err}");
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        body,
        "no fixes, no rewrite"
    );
    assert!(!err.contains("fixed"), "{err}");
}

#[test]
fn fix_skips_empty_region() {
    let body = "---\n   \n---\n\n```yaml\nnums: [1,2]\n```\n";
    let (_dir, file) = project(COMMAS, "doc.md", body);

    let (code, _out, err) = fix(&file);

    assert_eq!(code, 0, "stderr={err}");
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "---\n   \n---\n\n```yaml\nnums: [1, 2]\n```\n",
        "whitespace-only region is skipped while the fixable block is fixed"
    );
}

#[test]
fn markdown_flag_is_noop_when_globs_already_configured() {
    let body = "```yaml\nnums: [1,2]\n```\n";
    let (_dir, file) = project(COMMAS, "doc.md", body);

    let (code, _out, err) = run(Command::new(env!("CARGO_BIN_EXE_ryl"))
        .arg("--fix")
        .arg("--markdown")
        .arg(&file));

    assert_eq!(code, 0, "stderr={err}");
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "```yaml\nnums: [1, 2]\n```\n",
        "--markdown leaves an already-configured markdown glob set untouched"
    );
}

#[test]
fn markdown_flag_scans_directory() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ncommas = \"enable\"\n",
    )
    .unwrap();
    fs::write(dir.path().join("doc.md"), "```yaml\nnums: [1,2]\n```\n").unwrap();

    let (code, _out, err) = run(Command::new(env!("CARGO_BIN_EXE_ryl"))
        .arg("--markdown")
        .arg(dir.path()));

    assert_eq!(code, 1, "stderr={err}");
    assert!(
        err.contains("commas"),
        "directory scan with --markdown: {err}"
    );
}

#[test]
fn fix_missing_markdown_file_reports_read_error() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join(".ryl.toml"), COMMAS).unwrap();
    let file = dir.path().join("missing.md");

    let (code, _out, err) = fix(&file);

    assert_eq!(code, 2, "missing markdown file must fail: {err}");
    assert!(err.contains("failed to read"), "{err}");
}

#[test]
#[allow(clippy::permissions_set_readonly_false)]
fn fix_markdown_write_error_is_reported() {
    let (_dir, file) = project(COMMAS, "doc.md", "```yaml\nnums: [1,2]\n```\n");

    let mut perms = fs::metadata(&file).unwrap().permissions();
    perms.set_readonly(true);
    fs::set_permissions(&file, perms).unwrap();

    let (code, _out, err) = fix(&file);

    let mut perms = fs::metadata(&file).unwrap().permissions();
    perms.set_readonly(false);
    let _ = fs::set_permissions(&file, perms);

    assert_eq!(code, 2, "read-only markdown fix must fail: {err}");
    assert!(err.contains("failed to write fixed file"), "{err}");
}

#[test]
fn fix_does_not_corrupt_fence_inside_front_matter() {
    let body = "---\nzzz: [9,9]\nfoo: |\n  ```yaml\n  bar: [1,2]\n  ```\n---\n";
    let (_dir, file) = project(COMMAS, "doc.md", body);

    let (code, _out, err) = fix(&file);

    assert_eq!(code, 0, "stderr={err}");
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "---\nzzz: [9, 9]\nfoo: |\n  ```yaml\n  bar: [1,2]\n  ```\n---\n",
        "a fence nested in a front-matter scalar must not be fixed as standalone \
         YAML or cause overlapping-splice corruption"
    );
}

#[test]
fn markdown_flag_applies_to_global_config() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("doc.md");
    fs::write(&file, "```yaml\nnums: [1,2]\n```\n").unwrap();

    let (code, _out, err) = run(Command::new(env!("CARGO_BIN_EXE_ryl"))
        .arg("--markdown")
        .arg("-d")
        .arg("rules: {commas: enable}")
        .arg(&file));

    assert_eq!(code, 1, "stderr={err}");
    assert!(
        err.contains("commas"),
        "--markdown must enable markdown on a -d/-c global config: {err}"
    );
}

#[test]
fn markdown_flag_wins_over_overlapping_yaml_glob() {
    let config = "files = { yaml = [\"*.md\"] }\n[rules]\ncommas = \"enable\"\n";
    let (_dir, file) = project(config, "doc.md", "```yaml\nnums: [1,2]\n```\n");

    let (code, _out, err) = run(Command::new(env!("CARGO_BIN_EXE_ryl"))
        .arg("--markdown")
        .arg(&file));

    assert_eq!(code, 1, "stderr={err}");
    assert!(err.contains("commas"), "linted as markdown: {err}");
    assert!(
        !err.contains("matches both"),
        "--markdown-injected globs must win over yaml on overlap: {err}"
    );
}

#[test]
fn markdown_flag_enables_fix_without_files_glob() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ncommas = \"enable\"\n",
    )
    .unwrap();
    let file = dir.path().join("doc.md");
    fs::write(&file, "```yaml\nnums: [1,2]\n```\n").unwrap();

    let (code, _out, err) = run(Command::new(env!("CARGO_BIN_EXE_ryl"))
        .arg("--fix")
        .arg("--markdown")
        .arg(&file));

    assert_eq!(code, 0, "stderr={err}");
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "```yaml\nnums: [1, 2]\n```\n",
        "--markdown enables embedded-YAML fixing without a [files].markdown glob"
    );
}
