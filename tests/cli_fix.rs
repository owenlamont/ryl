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

#[test]
fn fix_applies_safe_newline_and_comment_fixes() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, "key: value #comment").unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ndocument-start = 'disable'\ncomments = 'enable'\nnew-lines = 'enable'\nnew-line-at-end-of-file = 'enable'\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("--fix").arg(&file));
    assert_eq!(
        code, 0,
        "fix should succeed: stdout={stdout} stderr={stderr}"
    );
    assert!(
        stderr.contains("Found 3 problems (3 fixed, 0 remaining)."),
        "expected fix summary in stderr: {stderr}"
    );
    assert!(stdout.is_empty(), "expected no stdout: {stdout}");

    let fixed = fs::read_to_string(&file).unwrap();
    assert_eq!(fixed, "key: value  # comment\n");
}

#[test]
fn fix_respects_toml_unfixable_rules() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, "key: value #comment").unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ndocument-start = 'disable'\ncomments = 'enable'\nnew-lines = 'enable'\nnew-line-at-end-of-file = 'enable'\n[fix]\nunfixable = ['comments']\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("--fix").arg(&file));
    assert_eq!(
        code, 1,
        "comment diagnostics should remain: stdout={stdout} stderr={stderr}"
    );
    assert!(
        stderr.contains("Found 3 problems (1 fixed, 2 remaining)."),
        "expected partial fix summary in stderr: {stderr}"
    );

    let fixed = fs::read_to_string(&file).unwrap();
    assert_eq!(fixed, "key: value #comment\n");
}

#[test]
fn fix_respects_toml_fixable_allowlist() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, "key: value #comment").unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ndocument-start = 'disable'\ncomments = 'enable'\nnew-lines = 'enable'\nnew-line-at-end-of-file = 'enable'\n[fix]\nfixable = ['comments']\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("--fix").arg(&file));
    assert_eq!(
        code, 1,
        "missing final newline should remain: stdout={stdout} stderr={stderr}"
    );
    assert!(
        stderr.contains("Found 3 problems (2 fixed, 1 remaining)."),
        "expected partial fix summary in stderr: {stderr}"
    );

    let fixed = fs::read_to_string(&file).unwrap();
    assert_eq!(fixed, "key: value  # comment");
}

#[test]
#[allow(clippy::permissions_set_readonly_false)]
fn fix_handles_write_error() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("read_only.yaml");
    // Missing newline so it needs a fix
    fs::write(&file, "key: value").unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ndocument-start = 'disable'\nnew-line-at-end-of-file = 'enable'\n",
    )
    .unwrap();

    let mut perms = fs::metadata(&file).unwrap().permissions();
    perms.set_readonly(true);
    fs::set_permissions(&file, perms).unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) = run(Command::new(exe).arg("--fix").arg(&file));

    // Reset permissions so tempdir can be cleaned up
    let mut perms = fs::metadata(&file).unwrap().permissions();
    perms.set_readonly(false);
    let _ = fs::set_permissions(&file, perms);

    assert_eq!(
        code, 2,
        "fix should fail on read-only file: stderr={stderr}"
    );
    assert!(
        stderr.contains("failed to write fixed file"),
        "error message should mention write failure: {stderr}"
    );
}

#[test]
fn fix_reports_summary_for_invalid_yaml() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("invalid.yaml");
    fs::write(&file, "key: [\n").unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ndocument-start = 'disable'\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("--fix").arg(&file));

    assert_eq!(
        code, 1,
        "invalid yaml should fail: stdout={stdout} stderr={stderr}"
    );
    assert!(
        stderr.contains("Found 1 problem (0 fixed, 1 remaining)."),
        "expected invalid-yaml summary in stderr: {stderr}"
    );
    assert!(stdout.is_empty(), "expected no stdout: {stdout}");
}

#[test]
fn fix_with_no_warnings_hides_warning_only_summary() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, "key: value #comment").unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ndocument-start = 'disable'\nnew-line-at-end-of-file = 'disable'\n[rules.comments]\nlevel = 'warning'\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe)
        .arg("--fix")
        .arg("--no-warnings")
        .arg(&file));

    assert_eq!(
        code, 0,
        "warning-only fix should pass: stdout={stdout} stderr={stderr}"
    );
    assert!(stderr.trim().is_empty(), "expected no stderr: {stderr}");
    assert!(stdout.is_empty(), "expected no stdout: {stdout}");
    assert_eq!(fs::read_to_string(&file).unwrap(), "key: value  # comment");
}

#[test]
fn fix_missing_file_reports_read_error() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("missing.yaml");

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("--fix").arg(&file));

    assert_eq!(
        code, 2,
        "missing file should fail: stdout={stdout} stderr={stderr}"
    );
    assert!(
        stderr.contains("failed to read"),
        "expected read error in stderr: {stderr}"
    );
    assert!(stdout.is_empty(), "expected no stdout: {stdout}");
}

#[test]
fn fix_applies_new_safe_spacing_rules() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(
        &file,
        "root:\n  mapping: {  key: [1 ,2]   }\n  empty: []\n # wrong\n  next: value\n",
    )
    .unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ndocument-start = 'disable'\nnew-line-at-end-of-file = 'disable'\ncomments-indentation = 'enable'\ncommas = 'enable'\nbraces = 'enable'\n[rules.brackets]\nmin-spaces-inside-empty = 1\nmax-spaces-inside-empty = 1\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("--fix").arg(&file));

    assert_eq!(
        code, 0,
        "new spacing fixes should succeed: stdout={stdout} stderr={stderr}"
    );
    assert!(
        stderr.contains("Found 6 problems (6 fixed, 0 remaining)."),
        "expected all-new-fixes summary in stderr: {stderr}"
    );
    assert!(stdout.is_empty(), "expected no stdout: {stdout}");
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "root:\n  mapping: {key: [1, 2]}\n  empty: [ ]\n  # wrong\n  next: value\n"
    );
}

#[test]
fn fix_comments_indentation_handles_crlf_blank_lines_without_newline_normalization() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(
        &file,
        "root:\r\n  # first\r\n\r\n # second\r\n  value: 1\r\n",
    )
    .unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ndocument-start = 'disable'\nnew-lines = 'disable'\nnew-line-at-end-of-file = 'disable'\ncomments-indentation = 'enable'\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("--fix").arg(&file));

    assert_eq!(
        code, 0,
        "comments-indentation fix should succeed on CRLF input: stdout={stdout} stderr={stderr}"
    );
    assert!(
        stderr.contains("Found 1 problem (1 fixed, 0 remaining)."),
        "expected one fixed CRLF comments-indentation problem: {stderr}"
    );
    assert!(stdout.is_empty(), "expected no stdout: {stdout}");
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        "root:\r\n  # first\r\n\r\n  # second\r\n  value: 1\r\n"
    );
}

#[test]
fn fix_under_best_practice_converges_in_one_invocation_for_escape_sequences() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, "key: \"a\\ta\"\n").unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules.quoted-strings]\nquote-type = \"single\"\nrequired = \"only-when-needed\"\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (_, _, _) = run(Command::new(exe).arg("--fix").arg(&file));
    let first_pass = fs::read_to_string(&file).unwrap();

    let (_, _, _) = run(Command::new(exe).arg("--fix").arg(&file));
    let second_pass = fs::read_to_string(&file).unwrap();

    assert_eq!(
        first_pass, second_pass,
        "one --fix invocation should reach the fixed point; first={first_pass:?} second={second_pass:?}"
    );
}

#[test]
fn fix_with_toml_allow_double_quotes_for_escaping_silences_quoted_strings_diagnostic() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, "message: \"line1\\nline2\"\n").unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules.quoted-strings]\nquote-type = \"single\"\nrequired = \"only-when-needed\"\nallow-double-quotes-for-escaping = true\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, stdout, stderr) = run(Command::new(exe).arg("--fix").arg(&file));

    assert_eq!(
        code, 0,
        "fix should succeed without quoted-strings diagnostics: stdout={stdout} stderr={stderr}"
    );
    assert!(
        !stdout.contains("quoted-strings") && !stderr.contains("quoted-strings"),
        "allow-double-quotes-for-escaping should silence quoted-strings diagnostic: stdout={stdout} stderr={stderr}"
    );
    let fixed = fs::read_to_string(&file).unwrap();
    assert!(
        fixed.contains("\"line1\\nline2\""),
        "double-quoted escape sequence should be retained verbatim: {fixed:?}"
    );
}

#[test]
fn fix_preserves_trailing_spaces_after_backslash_in_multiline_double_quoted_scalar() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, "key: \"a\\  \n  b\"\n").unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ndocument-start = 'disable'\ntrailing-spaces = 'enable'\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let _ = run(Command::new(exe).arg("--fix").arg(&file));

    let fixed = fs::read_to_string(&file).unwrap();
    assert_eq!(
        fixed, "key: \"a\\  \n  b\"\n",
        "stripping spaces between `\\` and the newline inside a multi-line double-quoted scalar would turn the backslash into a line-continuation escape and change the parsed value: {fixed:?}"
    );
}

#[test]
fn fix_strips_trailing_spaces_but_preserves_block_scalar_content() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(
        &file,
        "key: value   \nblock: |\n  line with trailing   \n  line two\n",
    )
    .unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ndocument-start = 'disable'\ntrailing-spaces = 'enable'\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) = run(Command::new(exe).arg("--fix").arg(&file));

    let fixed = fs::read_to_string(&file).unwrap();
    assert_eq!(
        fixed, "key: value\nblock: |\n  line with trailing   \n  line two\n",
        "trailing space outside block scalar should be stripped, inside preserved: {fixed:?}"
    );
    assert_eq!(
        code, 1,
        "block-scalar trailing space should remain reported: stderr={stderr}"
    );
    assert!(
        stderr.contains("trailing-spaces"),
        "block-scalar trailing space should still be flagged: {stderr}"
    );
}

#[test]
fn fix_inserts_document_start_marker_when_required() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, "key: value\n").unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ndocument-start = 'enable'\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) = run(Command::new(exe).arg("--fix").arg(&file));

    let fixed = fs::read_to_string(&file).unwrap();
    assert_eq!(
        fixed, "---\nkey: value\n",
        "missing --- should be prepended: {fixed:?}"
    );
    assert_eq!(code, 0, "fix should succeed: stderr={stderr}");
}

#[test]
fn fix_skips_document_start_when_buffer_already_uses_document_markers() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, "---\na: b\n...\nb: c\n").unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ndocument-start = 'enable'\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let _ = run(Command::new(exe).arg("--fix").arg(&file));

    let fixed = fs::read_to_string(&file).unwrap();
    assert_eq!(
        fixed, "---\na: b\n...\nb: c\n",
        "must not prepend `---` when stream already contains document markers: {fixed:?}"
    );
}

#[test]
fn fix_document_start_skips_buffer_without_document_events() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, "# just a comment\n").unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ndocument-start = 'enable'\nnew-line-at-end-of-file = 'disable'\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let _ = run(Command::new(exe).arg("--fix").arg(&file));

    let fixed = fs::read_to_string(&file).unwrap();
    assert_eq!(
        fixed, "# just a comment\n",
        "comment-only buffer with no document events must not get a `---` inserted: {fixed:?}"
    );
}

#[test]
fn fix_comments_preserves_quoted_value_after_flow_colon_without_space() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, "flow: {\"a\":\"b #c\"}\n").unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ndocument-start = 'disable'\ncomments = 'enable'\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let _ = run(Command::new(exe).arg("--fix").arg(&file));

    let fixed = fs::read_to_string(&file).unwrap();
    assert_eq!(
        fixed, "flow: {\"a\":\"b #c\"}\n",
        "`#` inside a quoted value immediately after a flow-context `:` must not be treated as a comment: {fixed:?}"
    );
}

#[test]
fn fix_does_not_skip_continuation_when_plain_scalar_ends_with_marker_like_suffix() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, "desc: version >2\n  body   #c\n").unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ndocument-start = 'disable'\ntrailing-spaces = 'enable'\ncomments = 'enable'\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let _ = run(Command::new(exe).arg("--fix").arg(&file));

    let fixed = fs::read_to_string(&file).unwrap();
    assert_eq!(
        fixed, "desc: version >2\n  body   # c\n",
        "plain scalar ending with `>2`/`|2`/`|` must not be treated as a block-scalar header — continuation lines must still receive comments-rule fixes: {fixed:?}"
    );
}

#[test]
fn fix_empty_lines_preserves_blanks_inside_multiline_plain_scalar() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, "key: a\n\n  b\n").unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ndocument-start = 'disable'\nempty-lines = { max = 0, max-start = 0, max-end = 0 }\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let _ = run(Command::new(exe).arg("--fix").arg(&file));

    let fixed = fs::read_to_string(&file).unwrap();
    assert_eq!(
        fixed, "key: a\n\n  b\n",
        "blank lines inside a multi-line plain scalar are paragraph breaks and must be preserved: {fixed:?}"
    );
}

#[test]
fn fix_empty_lines_preserves_blanks_inside_flow_quoted_value_without_space() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, "flow: {\"a\":\"b\n\nc\"}\n").unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ndocument-start = 'disable'\nempty-lines = { max = 0, max-start = 0, max-end = 0 }\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let _ = run(Command::new(exe).arg("--fix").arg(&file));

    let fixed = fs::read_to_string(&file).unwrap();
    assert_eq!(
        fixed, "flow: {\"a\":\"b\n\nc\"}\n",
        "blank lines inside a quoted value immediately after a flow-context `:` must be preserved: {fixed:?}"
    );
}

#[test]
fn fix_comments_handles_quote_chars_at_line_start_and_in_plain_scalars() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(
        &file,
        "'quoted-key': value\nplain: a\"b\nother: c #comment\n",
    )
    .unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ndocument-start = 'disable'\ncomments = 'enable'\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let _ = run(Command::new(exe).arg("--fix").arg(&file));

    let fixed = fs::read_to_string(&file).unwrap();
    assert_eq!(
        fixed, "'quoted-key': value\nplain: a\"b\nother: c  # comment\n",
        "quote at line start should toggle quote state cleanly; `\"` inside plain scalar must not toggle; comment fix must run for `other: c #comment`: {fixed:?}"
    );
}

#[test]
fn fix_skips_document_start_when_yaml_directive_present() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, "%YAML 1.1\nkey: value\n").unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ndocument-start = 'enable'\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let _ = run(Command::new(exe).arg("--fix").arg(&file));

    let fixed = fs::read_to_string(&file).unwrap();
    assert_eq!(
        fixed, "%YAML 1.1\nkey: value\n",
        "fix must not rewrite files with YAML directives: {fixed:?}"
    );
}

#[test]
fn fix_appends_document_end_marker_when_required() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, "---\nkey: value\n").unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ndocument-end = 'enable'\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let (code, _stdout, stderr) = run(Command::new(exe).arg("--fix").arg(&file));

    let fixed = fs::read_to_string(&file).unwrap();
    assert_eq!(
        fixed, "---\nkey: value\n...\n",
        "missing ... should be appended: {fixed:?}"
    );
    assert_eq!(code, 0, "fix should succeed: stderr={stderr}");
}

#[test]
fn fix_document_end_appends_marker_when_leading_comments_precede_start_marker() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, "# c\n---\nkey: value\n").unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ndocument-end = 'enable'\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let _ = run(Command::new(exe).arg("--fix").arg(&file));

    let fixed = fs::read_to_string(&file).unwrap();
    assert_eq!(
        fixed, "# c\n---\nkey: value\n...\n",
        "leading comments/blanks before the only `---` must not be treated as a separate document — fix must still append `...`: {fixed:?}"
    );
}

#[test]
fn fix_skips_document_end_for_multi_document_streams() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, "key: a\n---\nkey: b\n").unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ndocument-end = 'enable'\ndocument-start = 'disable'\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let _ = run(Command::new(exe).arg("--fix").arg(&file));

    let fixed = fs::read_to_string(&file).unwrap();
    assert_eq!(
        fixed, "key: a\n---\nkey: b\n",
        "fix must leave multi-document streams untouched: {fixed:?}"
    );
}

#[test]
fn fix_uses_crlf_when_buffer_uses_crlf_line_endings() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, "key: value\r\n").unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ndocument-start = 'enable'\ndocument-end = 'enable'\nnew-lines = 'disable'\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let _ = run(Command::new(exe).arg("--fix").arg(&file));

    let fixed = fs::read_to_string(&file).unwrap();
    assert_eq!(
        fixed, "---\r\nkey: value\r\n...\r\n",
        "marker insertions must use the buffer's line ending: {fixed:?}"
    );
}

#[test]
fn fix_appends_newline_when_buffer_lacks_final_newline() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, "key: value").unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ndocument-end = 'enable'\ndocument-start = 'disable'\nnew-line-at-end-of-file = 'disable'\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let _ = run(Command::new(exe).arg("--fix").arg(&file));

    let fixed = fs::read_to_string(&file).unwrap();
    assert_eq!(
        fixed, "key: value\n...\n",
        "fix must insert a newline before `...` when buffer lacks one: {fixed:?}"
    );
}

#[test]
fn fix_preserves_block_scalar_body_when_marker_carries_indent_indicator() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(
        &file,
        "literal: |2\n   line with trailing   \n   line two\n",
    )
    .unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ndocument-start = 'disable'\ntrailing-spaces = 'enable'\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let _ = run(Command::new(exe).arg("--fix").arg(&file));

    let fixed = fs::read_to_string(&file).unwrap();
    assert_eq!(
        fixed, "literal: |2\n   line with trailing   \n   line two\n",
        "explicit-indent block-scalar bodies must be left untouched: {fixed:?}"
    );
}

#[test]
fn fix_preserves_block_scalar_body_with_chomp_and_indent_indicator() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, "folded: >-2\n   first\n\n   second\n").unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ndocument-start = 'disable'\nempty-lines = { max = 0, max-start = 0, max-end = 0 }\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let _ = run(Command::new(exe).arg("--fix").arg(&file));

    let fixed = fs::read_to_string(&file).unwrap();
    assert_eq!(
        fixed, "folded: >-2\n   first\n\n   second\n",
        "explicit-chomp-and-indent block-scalar bodies must keep their blank lines: {fixed:?}"
    );
}

#[test]
fn fix_preserves_blank_lines_inside_multiline_double_quoted_scalar() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, "key: \"a\n\nb\"\n").unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ndocument-start = 'disable'\nempty-lines = { max = 0, max-start = 0, max-end = 0 }\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let _ = run(Command::new(exe).arg("--fix").arg(&file));

    let fixed = fs::read_to_string(&file).unwrap();
    assert_eq!(
        fixed, "key: \"a\n\nb\"\n",
        "blank lines inside a multi-line double-quoted scalar must be preserved: {fixed:?}"
    );
}

#[test]
fn fix_trims_blanks_after_plain_scalar_containing_apostrophe() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, "key: it's\n\n\nkey2: b\n").unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ndocument-start = 'disable'\nempty-lines = { max = 0, max-start = 0, max-end = 0 }\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let _ = run(Command::new(exe).arg("--fix").arg(&file));

    let fixed = fs::read_to_string(&file).unwrap();
    assert_eq!(
        fixed, "key: it's\nkey2: b\n",
        "apostrophe inside a plain scalar must not block trimming of subsequent blank-line runs: {fixed:?}"
    );
}

#[test]
fn fix_handles_doubled_quote_escape_in_multiline_single_quoted_scalar() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, "key: 'a''b\n\nc'\n").unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ndocument-start = 'disable'\nempty-lines = { max = 0, max-start = 0, max-end = 0 }\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let _ = run(Command::new(exe).arg("--fix").arg(&file));

    let fixed = fs::read_to_string(&file).unwrap();
    assert_eq!(
        fixed, "key: 'a''b\n\nc'\n",
        "doubled '' escape inside a multi-line single-quoted scalar must not exit quote tracking: {fixed:?}"
    );
}

#[test]
fn fix_preserves_blank_lines_inside_multiline_single_quoted_scalar() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(&file, "key: 'a\n\nb'\n").unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ndocument-start = 'disable'\nempty-lines = { max = 0, max-start = 0, max-end = 0 }\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let _ = run(Command::new(exe).arg("--fix").arg(&file));

    let fixed = fs::read_to_string(&file).unwrap();
    assert_eq!(
        fixed, "key: 'a\n\nb'\n",
        "blank lines inside a multi-line single-quoted scalar must be preserved: {fixed:?}"
    );
}

#[test]
fn fix_trims_consecutive_blank_lines_outside_block_scalars() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("input.yaml");
    fs::write(
        &file,
        "key: a\n\n\n\n\nblock: |\n  one\n\n\n  two\nlast: z\n",
    )
    .unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\nempty-lines = { max = 2, max-start = 0, max-end = 0 }\ndocument-start = 'disable'\n",
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_ryl");
    let _ = run(Command::new(exe).arg("--fix").arg(&file));

    let fixed = fs::read_to_string(&file).unwrap();
    assert_eq!(
        fixed, "key: a\n\n\nblock: |\n  one\n\n\n  two\nlast: z\n",
        "outside runs trimmed to max=2, inner block-scalar blanks preserved: {fixed:?}"
    );
}
