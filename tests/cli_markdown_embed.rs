use std::fs;
use std::process::Command;

use tempfile::tempdir;

mod common;
use common::cli::run;

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
fn many_fenced_blocks_map_to_correct_host_lines() {
    // The extractor derives each block's line offset by binary search over
    // precomputed newline positions; a regression to rescanning from the document
    // start is quadratic over many blocks. Each 4-line block puts its content (with
    // a trailing space) on host line `4*i + 2`, so this pins that a block far down
    // the document still maps to the right host line.
    let dir = tempdir().unwrap();
    let body = "```yaml\nk: v  \n```\n\n".repeat(2000);
    let file = dir.path().join("many.md");
    fs::write(&file, &body).unwrap();

    let (code, _out, err) = run(Command::new(env!("CARGO_BIN_EXE_ryl"))
        .arg("--markdown")
        .arg("-d")
        .arg("rules:\n  trailing-spaces: enable\n")
        .arg(&file));

    assert_eq!(code, 1, "trailing spaces should be reported: {err}");
    assert!(
        err.contains("2:5"),
        "first block trailing-space position: {err}"
    );
    assert!(
        err.contains("7998:5"),
        "last block (i=1999) must map to host line 4*1999+2 = 7998: {err}"
    );
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
fn blockquoted_fence_column_accounts_for_quote_marker() {
    let config = "files = { markdown = [\"*.md\"] }\n[rules]\ntruthy = \"enable\"\n";
    let body = "> ```yaml\n> foo: True\n> ```\n";
    let (_dir, file) = project(config, "doc.md", body);

    let (code, _out, err) = run(Command::new(env!("CARGO_BIN_EXE_ryl")).arg(&file));

    assert_eq!(code, 1, "stderr={err}");
    assert!(
        err.contains("2:8"),
        "blockquote `> ` prefix must shift the column to 8: {err}"
    );
}

#[test]
fn fence_nested_in_front_matter_is_not_double_linted() {
    let config = "files = { markdown = [\"*.md\"] }\n[rules]\ntruthy = \"enable\"\n";
    let body = "---\ndesc: |\n  ```yaml\n  inner: True\n  ```\n---\n";
    let (_dir, file) = project(config, "doc.md", body);

    let (code, out, err) = run(Command::new(env!("CARGO_BIN_EXE_ryl")).arg(&file));

    assert_eq!(code, 0, "stderr={err}");
    assert!(
        out.is_empty() && err.is_empty(),
        "a fence inside a front-matter scalar is string content, not a document: out={out} err={err}"
    );
}

#[test]
fn fence_crossing_front_matter_terminator_is_dropped() {
    let config = "files = { markdown = [\"*.md\"] }\n[rules]\ncommas = \"enable\"\n";
    let body = "---\ntags: [x,y]\ndesc: |\n  ```yaml\n  inner: [1,2]\n---\nafter: [3,4]\n```\n\ntext\n";
    let (_dir, file) = project(config, "doc.md", body);

    let (code, _out, err) = run(Command::new(env!("CARGO_BIN_EXE_ryl")).arg(&file));

    assert_eq!(code, 1, "stderr={err}");
    assert!(err.contains("2:10"), "front matter is linted: {err}");
    assert!(
        !err.contains("5:13") && !err.contains("7:11"),
        "a fence opened inside front matter and closed after the terminator must \
         not be linted as a separate region: {err}"
    );
}

#[test]
fn fence_inside_disabled_front_matter_is_not_linted() {
    let config = "files = { markdown = [\"*.md\"] }\nmarkdown = { front-matter = false }\n[rules]\ncommas = \"enable\"\n";
    let body = "---\ndesc: |\n  ```yaml\n  inner: [1,2]\n  ```\n---\n\ntext\n";
    let (_dir, file) = project(config, "doc.md", body);

    let (code, out, err) = run(Command::new(env!("CARGO_BIN_EXE_ryl")).arg(&file));

    assert_eq!(code, 0, "stderr={err}");
    assert!(
        out.is_empty() && err.is_empty(),
        "a fence inside the front-matter scalar must stay unlinted when front-matter \
         is disabled: out={out} err={err}"
    );
}

#[test]
fn fence_opening_on_last_front_matter_line_is_dropped() {
    let config = "files = { markdown = [\"*.md\"] }\n[rules]\ncommas = \"enable\"\n";
    let body = "---\ndesc: |\n  ```yaml\n---\nafter: [1,2]\n```\n";
    let (_dir, file) = project(config, "doc.md", body);

    let (code, out, err) = run(Command::new(env!("CARGO_BIN_EXE_ryl")).arg(&file));

    assert_eq!(code, 0, "stderr={err}");
    assert!(
        out.is_empty() && err.is_empty(),
        "a fence whose opener is the last front-matter line (content starting on the \
         terminator) must not be linted as a body fence: out={out} err={err}"
    );
}

#[test]
fn body_fence_immediately_after_front_matter_is_linted() {
    let config = "files = { markdown = [\"*.md\"] }\n[rules]\ncommas = \"enable\"\n";
    let body = "---\na: 1\n---\n```yaml\nnums: [1,2]\n```\n";
    let (_dir, file) = project(config, "doc.md", body);

    let (code, _out, err) = run(Command::new(env!("CARGO_BIN_EXE_ryl")).arg(&file));

    assert_eq!(code, 1, "stderr={err}");
    assert!(
        err.contains("5:10") && err.contains("commas"),
        "a real body fence directly after the terminator must still be linted: {err}"
    );
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

// Embedded-markdown regression tests for the rules added in #252-#256: each must
// fire inside a fenced block with positions remapped to the host markdown file
// (#277 item 6). ryl-only options go through TOML config.

#[test]
fn merge_keys_fires_in_fenced_block() {
    let cfg = "files = { markdown = [\"*.md\"] }\n[rules]\nmerge-keys = \"enable\"\n";
    let body = "intro\n\n```yaml\nbase: &b {x: 1}\nchild:\n  <<: *b\n```\n";
    let (_dir, file) = project(cfg, "doc.md", body);

    let (code, _out, err) = run(Command::new(env!("CARGO_BIN_EXE_ryl")).arg(&file));

    assert_eq!(code, 1, "{err}");
    assert!(
        err.contains("6:3")
            && err.contains("forbidden merge key")
            && err.contains("merge-keys"),
        "merge key remapped to host line 6: {err}"
    );
}

#[test]
fn key_duplicates_canonical_fires_in_fenced_block() {
    let cfg = "files = { markdown = [\"*.md\"] }\n[rules.key-duplicates]\ncheck-canonical = true\n";
    let body = "```yaml\n0xB: a\n11: b\n```\n";
    let (_dir, file) = project(cfg, "doc.md", body);

    let (code, _out, err) = run(Command::new(env!("CARGO_BIN_EXE_ryl")).arg(&file));

    assert_eq!(code, 1, "{err}");
    assert!(
        err.contains("3:1")
            && err.contains("duplication of key \"11\"")
            && err.contains("key-duplicates"),
        "canonical integer duplicate remapped to host line 3: {err}"
    );
}

#[test]
fn unicode_line_breaks_fires_in_fenced_block() {
    let cfg = "files = { markdown = [\"*.md\"] }\n[rules]\nunicode-line-breaks = \"enable\"\n";
    // A raw U+2028 line separator inside the fenced scalar (content, not a break).
    let body = "```yaml\nkey: a\u{2028}b\n```\n";
    let (_dir, file) = project(cfg, "doc.md", body);

    let (code, _out, err) = run(Command::new(env!("CARGO_BIN_EXE_ryl")).arg(&file));

    assert_eq!(code, 1, "{err}");
    assert!(
        err.contains("2:7") && err.contains("unicode-line-breaks"),
        "raw U+2028 flagged at host 2:7 inside the fenced block: {err}"
    );
}

#[test]
fn anchors_ambiguous_name_fires_in_fenced_block() {
    let cfg = "files = { markdown = [\"*.md\"] }\n[rules.anchors]\nforbid-ambiguous-anchor-alias-names = true\n";
    let body = "```yaml\na: &:foo 1\n```\n";
    let (_dir, file) = project(cfg, "doc.md", body);

    let (code, _out, err) = run(Command::new(env!("CARGO_BIN_EXE_ryl")).arg(&file));

    assert_eq!(code, 1, "{err}");
    assert!(
        err.contains("2:4")
            && err.contains("ambiguous anchor name")
            && err.contains("anchors"),
        "colon-welded anchor name flagged at host 2:4 in the fenced block: {err}"
    );
}

#[test]
fn block_scalar_chomping_fires_in_fenced_block() {
    // The rule runs in embedded YAML (it is not markdown-suppressed); a bare block
    // header inside a fenced block maps back to its host line and marker column.
    let cfg = "files = { markdown = [\"*.md\"] }\n[rules]\nblock-scalar-chomping = \"enable\"\n";
    let body = "intro\n\n```yaml\nscript: |\n  echo hi\n```\n";
    let (_dir, file) = project(cfg, "doc.md", body);

    let (code, _out, err) = run(Command::new(env!("CARGO_BIN_EXE_ryl")).arg(&file));

    assert_eq!(code, 1, "{err}");
    assert!(
        err.contains("4:9") && err.contains("block-scalar-chomping"),
        "fenced bare block header flagged at host 4:9: {err}"
    );
}

#[test]
fn bare_cr_markdown_host_is_loudly_skipped_not_silently_missed() {
    // `pulldown-cmark` doesn't honour CommonMark §2.1's bare-`\r` line ending, so it
    // can't locate fences/front matter in a `\r`-delimited host. ryl reports an error
    // instead of silently extracting (and checking) nothing (issue #284). The guard
    // fires before extraction, so the enabled rule set is irrelevant.
    let body = "# T\r\r```yaml\rk: v  \r```\r";
    let (_dir, file) = project(COLONS_ONLY, "doc.md", body);

    let (code, out, err) = run(Command::new(env!("CARGO_BIN_EXE_ryl")).arg(&file));

    assert_eq!(
        code, 1,
        "a bare-CR markdown host must fail loudly: {out}{err}"
    );
    let output = format!("{out}{err}");
    assert!(
        output.contains("1:1") && output.contains("carriage return"),
        "expected a CR-host notice: {output}"
    );
}

#[test]
fn fix_skips_a_bare_cr_markdown_host_and_leaves_it_unchanged() {
    let body = "# T\r\r```yaml\rk: v  \r```\r";
    let (_dir, file) = project(COLONS_ONLY, "doc.md", body);
    let before = fs::read(&file).unwrap();

    let (_code, out, err) = run(Command::new(env!("CARGO_BIN_EXE_ryl"))
        .arg("--fix")
        .arg(&file));

    let output = format!("{out}{err}");
    assert!(
        output.contains("skipped by --fix") && output.contains("carriage return"),
        "expected a --fix skip notice: {output}"
    );
    assert_eq!(
        fs::read(&file).unwrap(),
        before,
        "a bare-CR markdown host must be left byte-for-byte unchanged"
    );
}
