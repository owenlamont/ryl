use std::fs;
use std::process::Command;

use tempfile::tempdir;

mod common;
use common::cli::{command_output, run};

/// `block-scalar-chomping` is a ryl-only rule, so it is configured through TOML
/// rather than the yamllint-compatible YAML config that `-d` carries.
fn lint_with_toml_config(content: &str, config: &str) -> (i32, String) {
    let dir = tempdir().unwrap();
    let file = dir.path().join("doc.yaml");
    fs::write(&file, content).unwrap();
    let config_path = dir.path().join(".ryl.toml");
    fs::write(&config_path, config).unwrap();
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) =
        run(Command::new(exe).arg("-c").arg(&config_path).arg(&file));
    (code, command_output(&stdout, &stderr).to_string())
}

const ENABLE: &str = "[rules]\nblock-scalar-chomping = \"enable\"\n";

#[test]
fn flags_bare_literal_and_folded_clip_headers() {
    // A bare `|`/`>` defaults to clip; both are flagged at the marker column,
    // including a nested mapping value, while explicit `-`/`+` headers are not.
    let (code, output) = lint_with_toml_config(
        "literal: |\n  one\nfolded: >\n  two\nnested:\n  inner: |\n    three\nstrip: |-\n  keep me\nkeep: >+\n  fine\n",
        ENABLE,
    );
    assert_eq!(code, 1, "bare clip headers should fail: {output}");
    for pos in ["1:10", "3:9", "6:10"] {
        assert!(
            output.contains(pos),
            "expected a clip diagnostic at {pos}: {output}"
        );
    }
    assert!(
        output.contains("missing explicit chomping indicator")
            && output.contains("block-scalar-chomping"),
        "message text and bare rule id expected: {output}"
    );
    // Exactly the three clip headers fire; the explicit `|-`/`>+` headers on lines
    // 8 and 10 are not flagged. A count is robust across output formats — a
    // negative `line:` substring check would collide with the GitHub format's
    // `col=10` rendering of the line-1/line-6 diagnostics.
    assert_eq!(
        output.matches("block-scalar-chomping").count(),
        3,
        "exactly the three bare headers should be flagged: {output}"
    );
}

#[test]
fn flags_indentation_only_header() {
    // `|2` carries an indentation indicator but no chomping indicator, so the
    // chomping is still the implicit clip default and the header is flagged.
    let (code, output) = lint_with_toml_config("block: |2\n  body\n", ENABLE);
    assert_eq!(code, 1, "indentation-only header should fail: {output}");
    assert!(
        output.contains("1:8") && output.contains("block-scalar-chomping"),
        "indentation-only header flagged at the marker: {output}"
    );
}

#[test]
fn ignores_pipe_inside_quoted_scalar() {
    // The scanner — not a text scan — decides what is a block header, so a `|`
    // inside a quoted scalar is never treated as one.
    let (code, output) =
        lint_with_toml_config("a: \"pipe | not a header\"\nb: 1\n", ENABLE);
    assert_eq!(code, 0, "a quoted pipe is not a block header: {output}");
    assert!(
        !output.contains("block-scalar-chomping"),
        "quoted pipe must not be flagged: {output}"
    );
}

#[test]
fn flags_empty_and_blank_only_block_scalars() {
    // Granit anchors an end-of-stream empty scalar on its header, but anchors an
    // empty scalar before another node on that following node. Both forms and a
    // blank-only body must still report their own headers.
    let (code, output) =
        lint_with_toml_config("first: |\nsecond: |\n\nblank: |\n  \ncafé: |", ENABLE);
    assert_eq!(code, 1, "empty block scalars should fail: {output}");
    for pos in ["1:8", "2:9", "4:8", "6:7"] {
        assert!(
            output.contains(pos),
            "expected an empty-scalar diagnostic at {pos}: {output}"
        );
    }
    assert!(
        output.matches("block-scalar-chomping").count() == 4,
        "each empty or blank-only scalar should be flagged once: {output}"
    );
}

#[test]
fn does_not_treat_header_like_first_content_as_the_header() {
    // A literal scalar may begin with content that itself looks like a standalone
    // block header. The scanner token starts there, but the diagnostic belongs to
    // the actual header above it.
    let (code, output) = lint_with_toml_config("key: |\n  |\n", ENABLE);
    assert_eq!(code, 1, "bare block scalar should fail: {output}");
    assert!(
        output.contains("1:6") && !output.contains("2:3"),
        "the diagnostic should point to the real header: {output}"
    );
}

#[test]
fn treats_bare_cr_as_a_line_break() {
    // A bare `\r` between header and content is a YAML 1.2 line break, so this is
    // a normal one-line header — handled identically to the `\n` form (1:10), like
    // every other granit-based rule. (Classic-Mac `\r`-only files are otherwise
    // unsupported; the byte-scanning rules count `\n` only — see unicode-line-breaks.)
    let (code, output) = lint_with_toml_config("literal: |\r  one\n", ENABLE);
    assert_eq!(code, 1, "bare-CR-separated header is flagged: {output}");
    assert!(
        output.contains("1:10") && output.contains("block-scalar-chomping"),
        "CR treated as a break: header on line 1, marker col 10: {output}"
    );
}

#[test]
fn counts_bare_cr_when_numbering_the_header_line() {
    // Two leading bare `\r`s are two line breaks, so the header `z: |` is on line 3
    // (not line 1) — the CR-aware count matches granit and ryl's other token rules.
    let (code, output) = lint_with_toml_config("x: 1\ry: 2\rz: |\n  body\n", ENABLE);
    assert_eq!(
        code, 1,
        "header is flagged on its CR-counted line: {output}"
    );
    assert!(
        output.contains("3:4") && output.contains("block-scalar-chomping"),
        "CR-counted header line 3, marker col 4: {output}"
    );
}

#[test]
fn flags_crlf_header() {
    // CRLF endings: `\r\n` is one break, so the header is line 1 and the body
    // line 2 — identical to the LF form (marker at col 6).
    let (code, output) = lint_with_toml_config("key: |\r\n  hi\r\n", ENABLE);
    assert_eq!(code, 1, "CRLF header is flagged: {output}");
    assert!(
        output.contains("1:6") && output.contains("block-scalar-chomping"),
        "CRLF header on line 1, marker col 6: {output}"
    );
}

#[test]
fn flags_header_without_trailing_newline() {
    // A file with no final line break still yields its last line, so a header at
    // end-of-file is located and flagged.
    let (code, output) = lint_with_toml_config("key: |\n  hi", ENABLE);
    assert_eq!(
        code, 1,
        "header without a trailing newline is flagged: {output}"
    );
    assert!(
        output.contains("1:6") && output.contains("block-scalar-chomping"),
        "header on line 1, marker col 6: {output}"
    );
}

#[test]
fn strips_trailing_comment_after_header() {
    // A comment may follow the header indicators; it is stripped before the
    // chomping check, so `| # ...` is still flagged at the marker.
    let (code, output) =
        lint_with_toml_config("script: | # run it\n  echo hi\n", ENABLE);
    assert_eq!(code, 1, "commented bare header should fail: {output}");
    assert!(
        output.contains("1:9") && output.contains("block-scalar-chomping"),
        "header before an inline comment flagged at the marker: {output}"
    );
}

#[test]
fn preserves_hash_characters_that_do_not_start_comments() {
    // An unquoted `#` is part of a plain scalar unless separated by whitespace.
    // Verbatim tag URIs may also contain `#`; header recovery must preserve both
    // forms instead of truncating the line before the scanner-confirmed marker.
    let (code, output) = lint_with_toml_config(
        "a#b: |\n  body\nvalue: !<tag:example.com,2000:app/foo#bar> |\n  body\n",
        ENABLE,
    );
    assert_eq!(code, 1, "bare headers after hashes should fail: {output}");
    for pos in ["1:6", "3:44"] {
        assert!(
            output.contains(pos),
            "expected a complete hash-bearing header at {pos}: {output}"
        );
    }
}

#[test]
fn preserves_plain_key_fragments_that_resemble_node_properties() {
    // `!` and `&` may appear after the first character of a plain scalar. The
    // scanner confirms these are keys, so header recovery must not reinterpret
    // their trailing fragments as tag or anchor properties.
    let (code, output) =
        lint_with_toml_config("a !foo: |\n  body\na &foo: |\n  body\n", ENABLE);
    assert_eq!(
        code, 1,
        "bare headers after plain keys should fail: {output}"
    );
    for pos in ["1:9", "3:9"] {
        assert!(
            output.contains(pos),
            "expected a complete property-like key at {pos}: {output}"
        );
    }
}

#[test]
fn reports_char_based_column_after_multibyte_key() {
    // `café` is four characters but five bytes; the marker column counts
    // characters, so the `|` sits at column 7 (a byte offset would give 8).
    let (code, output) = lint_with_toml_config("café: |\n  body\n", ENABLE);
    assert_eq!(code, 1, "multibyte-key header should fail: {output}");
    assert!(
        output.contains("1:7") && output.contains("block-scalar-chomping"),
        "char-based marker column past the multibyte key: {output}"
    );
}

#[test]
fn recovers_header_on_its_own_line_above_a_blank_gap() {
    // The header need not be on the key's line; here it is on its own line and a
    // blank line separates it from the content. The diagnostic still points at
    // the `|` on line 2, not the content line.
    let (code, output) = lint_with_toml_config("key:\n  |\n\n  body\n", ENABLE);
    assert_eq!(code, 1, "own-line header should fail: {output}");
    assert!(
        output.contains("2:3") && output.contains("block-scalar-chomping"),
        "own-line header flagged above the blank gap: {output}"
    );
}

#[test]
fn rule_is_rejected_in_yaml_config() {
    // ryl-only: yamllint-compatible YAML config (here via `-d`) must reject it
    // rather than silently linting or clashing with a future yamllint rule.
    let dir = tempdir().unwrap();
    let file = dir.path().join("doc.yaml");
    fs::write(&file, "block: |\n  body\n").unwrap();
    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("-d")
        .arg("rules: {block-scalar-chomping: enable}")
        .arg(&file));
    assert_eq!(
        code, 2,
        "a ryl-only rule in YAML config is a usage error: stdout={stdout} stderr={stderr}"
    );
    let output = command_output(&stdout, &stderr);
    assert!(
        output.contains("block-scalar-chomping"),
        "error should name the rule: {output}"
    );
    assert!(
        output.to_lowercase().contains("toml"),
        "error should point to TOML config: {output}"
    );
}

#[test]
fn per_file_ignores_accept_the_rule_name() {
    // A `[per-file-ignores]` entry naming the rule must be accepted (the rule id
    // round-trips through `RuleName`), suppressing its diagnostics for that file.
    let dir = tempdir().unwrap();
    let file = dir.path().join("ignored.yaml");
    fs::write(&file, "block: |\n  body\n").unwrap();
    let config = dir.path().join(".ryl.toml");
    fs::write(
        &config,
        format!(
            "[rules]\nblock-scalar-chomping = \"enable\"\n[per-file-ignores]\n'{}' = ['block-scalar-chomping']\n",
            file.display()
        ),
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) =
        run(Command::new(exe).arg("-c").arg(&config).arg(&file));
    assert_eq!(
        code, 0,
        "per-file-ignores should suppress the rule: stdout={stdout} stderr={stderr}"
    );
    assert!(stdout.trim().is_empty(), "expected no stdout: {stdout}");
    assert!(stderr.trim().is_empty(), "expected no stderr: {stderr}");
}

#[test]
fn disabled_by_default() {
    // The rule is off unless explicitly enabled, so a bare header alone produces
    // no diagnostic under an unrelated rule's config.
    let (code, output) = lint_with_toml_config(
        "block: |\n  body\n",
        "[rules]\ntrailing-spaces = \"enable\"\n",
    );
    assert_eq!(code, 0, "rule is off by default: {output}");
    assert!(
        !output.contains("block-scalar-chomping"),
        "rule must not fire unless enabled: {output}"
    );
}
