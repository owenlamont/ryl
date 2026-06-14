#![cfg(feature = "lsp")]
//! Unit tests for the language server's pure bridges: position/URI encoding and
//! the lint/fix-to-LSP analysis layer. These need no live connection, so they
//! pin the fiddly parts (UTF-8/16/32 column math over astral-plane characters,
//! `file:` URI decoding, whole-document edit ranges) cheaply and exhaustively.

use std::path::Path;

use lsp_types::PositionEncodingKind;

use ryl::config::{SourceKind, YamlLintConfig};
use ryl::lsp::analysis::{diagnostics, fix_all_edit};
use ryl::lsp::encoding::{
    PositionEncoding, full_range, negotiate, problem_range, uri_to_path,
};

#[test]
fn negotiate_prefers_clients_first_supported_kind() {
    assert_eq!(
        negotiate(Some(&[
            PositionEncodingKind::UTF8,
            PositionEncodingKind::UTF16
        ])),
        PositionEncoding::Utf8,
        "the client's most-preferred supported kind wins"
    );
}

#[test]
fn negotiate_skips_unknown_kinds() {
    assert_eq!(
        negotiate(Some(&[
            PositionEncodingKind::new("utf-7"),
            PositionEncodingKind::UTF32,
        ])),
        PositionEncoding::Utf32,
        "an unrecognised kind is skipped to the next supported one"
    );
}

#[test]
fn negotiate_honours_an_explicit_utf16_preference() {
    assert_eq!(
        negotiate(Some(&[PositionEncodingKind::UTF16])),
        PositionEncoding::Utf16,
        "an explicit UTF-16 request is matched, not merely defaulted to"
    );
}

#[test]
fn negotiate_defaults_to_utf16_without_client_preference() {
    assert_eq!(
        negotiate(None),
        PositionEncoding::Utf16,
        "no list -> UTF-16"
    );
    assert_eq!(
        negotiate(Some(&[])),
        PositionEncoding::Utf16,
        "empty list -> UTF-16"
    );
}

#[test]
fn problem_range_counts_code_units_across_an_astral_char() {
    // "a: 😀 " — the trailing space is the 5th code point (ryl reports col 5).
    // U+1F600 is one code point but two UTF-16 units and four UTF-8 bytes, so a
    // correct conversion gives a different column per encoding (a naive char count
    // would wrongly give 4 for all three).
    let lines = ["a: \u{1F600} "];
    let utf16 = problem_range(&lines, 1, 5, PositionEncoding::Utf16);
    assert_eq!(
        (utf16.start.line, utf16.start.character),
        (0, 5),
        "UTF-16 start"
    );
    assert_eq!(utf16.end.character, 6, "UTF-16 end is one char past start");

    let utf8 = problem_range(&lines, 1, 5, PositionEncoding::Utf8);
    assert_eq!(utf8.start.character, 7, "UTF-8 counts the emoji as 4 bytes");
    assert_eq!(utf8.end.character, 8, "UTF-8 end");

    let utf32 = problem_range(&lines, 1, 5, PositionEncoding::Utf32);
    assert_eq!(
        utf32.start.character, 4,
        "UTF-32 counts the emoji as one unit"
    );
    assert_eq!(utf32.end.character, 5, "UTF-32 end");
}

#[test]
fn problem_range_clamps_a_column_past_end_of_line() {
    let lines = ["ab"];
    let range = problem_range(&lines, 1, 99, PositionEncoding::Utf16);
    assert_eq!(
        range.start.character, 2,
        "column past the line clamps to its length"
    );
    assert_eq!(
        range.end.character, 2,
        "and the end clamps too (zero-width)"
    );
}

#[test]
fn problem_range_for_a_line_beyond_the_document_is_empty() {
    let lines: [&str; 0] = [];
    let range = problem_range(&lines, 3, 1, PositionEncoding::Utf16);
    assert_eq!(
        (range.start.line, range.start.character),
        (2, 0),
        "missing line -> empty prefix"
    );
}

#[test]
fn full_range_ends_at_phantom_line_when_text_ends_in_a_break() {
    let lf = full_range("a\nb\n", PositionEncoding::Utf16);
    assert_eq!(
        (lf.end.line, lf.end.character),
        (2, 0),
        "LF end sits at col 0 of line 2"
    );
    let cr = full_range("a\rb\r", PositionEncoding::Utf16);
    assert_eq!(
        (cr.end.line, cr.end.character),
        (2, 0),
        "bare CR is a line break too"
    );
}

#[test]
fn full_range_ends_at_last_char_without_a_trailing_break() {
    let range = full_range("a\nbc", PositionEncoding::Utf16);
    assert_eq!(
        (range.end.line, range.end.character),
        (1, 2),
        "ends at end of last line"
    );
}

#[test]
fn full_range_of_empty_text_is_zero_width() {
    let range = full_range("", PositionEncoding::Utf16);
    assert_eq!(
        (range.start.line, range.start.character),
        (0, 0),
        "empty start"
    );
    assert_eq!((range.end.line, range.end.character), (0, 0), "empty end");
}

#[test]
fn uri_to_path_decodes_a_plain_file_uri() {
    let path = uri_to_path("file:///home/u/a%20b.yaml").expect("file URI -> path");
    assert_eq!(path, Path::new("/home/u/a b.yaml"), "percent-decoded path");
}

#[test]
fn uri_to_path_accepts_localhost_authority() {
    let path = uri_to_path("file://localhost/srv/x.yaml")
        .expect("localhost is a no-op authority");
    assert_eq!(path, Path::new("/srv/x.yaml"));
}

#[test]
fn uri_to_path_handles_a_unc_host() {
    let path = uri_to_path("file://server/share/x.yaml").expect("UNC host");
    assert_eq!(
        path,
        Path::new("//server/share/x.yaml"),
        "host becomes a UNC prefix"
    );
}

#[test]
fn uri_to_path_strips_a_windows_drive_slash() {
    let path = uri_to_path("file:///C:/Users/x.yaml").expect("drive URI");
    assert_eq!(
        path,
        Path::new("C:/Users/x.yaml"),
        "leading slash before a drive is dropped"
    );
}

#[test]
fn uri_to_path_rejects_non_file_and_pathless_uris() {
    assert!(
        uri_to_path("http://example.com/x").is_none(),
        "non-file scheme"
    );
    assert!(
        uri_to_path("untitled:Untitled-1").is_none(),
        "untitled buffer"
    );
    // `file://host` with no path slash: authority present, empty path.
    assert_eq!(
        uri_to_path("file://host").expect("authority-only file URI"),
        Path::new("//host"),
        "no path component"
    );
}

#[test]
fn uri_to_path_keeps_a_literal_percent_without_two_hex_digits() {
    let path = uri_to_path("file:///x/100%done").expect("lenient percent decode");
    assert_eq!(path, Path::new("/x/100%done"), "a bare % is kept literally");
}

#[test]
fn uri_to_path_accepts_a_case_insensitive_scheme() {
    // URI schemes are case-insensitive (RFC 3986); a client may send `FILE://`.
    let path = uri_to_path("FILE:///srv/x.yaml").expect("uppercase scheme");
    assert_eq!(path, Path::new("/srv/x.yaml"));
}

#[test]
fn uri_to_path_accepts_an_authority_less_file_uri() {
    // RFC 8089 allows the minimal `file:/path` form (no `//authority`).
    let path = uri_to_path("file:/srv/x.yaml").expect("authority-less file URI");
    assert_eq!(path, Path::new("/srv/x.yaml"));
}

#[test]
fn uri_to_path_decodes_uppercase_hex_escapes() {
    // %2A uses an uppercase hex digit; 0x2A is '*'.
    let path = uri_to_path("file:///x/a%2Ab.yaml").expect("uppercase hex escape");
    assert_eq!(path, Path::new("/x/a*b.yaml"), "uppercase hex decodes");
}

fn yaml_cfg(toml: &str) -> YamlLintConfig {
    YamlLintConfig::from_toml_str(toml).expect("test config parses")
}

#[test]
fn diagnostics_map_a_yaml_problem_to_lsp() {
    let cfg = yaml_cfg("[rules]\ntrailing-spaces = \"enable\"\n");
    let diags = diagnostics(
        "a: \u{1F600} \n",
        Path::new("/proj/x.yaml"),
        &cfg,
        Path::new("/proj"),
        SourceKind::Yaml,
        PositionEncoding::Utf16,
    );
    assert_eq!(diags.len(), 1, "one trailing-space problem");
    let diag = &diags[0];
    assert_eq!(diag.source.as_deref(), Some("ryl"), "source tag");
    assert_eq!(
        diag.range.start.character, 5,
        "UTF-16 column of the trailing space"
    );
    assert_eq!(
        diag.code,
        Some(lsp_types::NumberOrString::String(
            "trailing-spaces".to_string()
        )),
        "rule id as the diagnostic code"
    );
    assert_eq!(diag.severity, Some(lsp_types::DiagnosticSeverity::ERROR));
}

#[test]
fn diagnostics_carry_warning_severity_when_configured() {
    let cfg = yaml_cfg("[rules.trailing-spaces]\nlevel = \"warning\"\n");
    let diags = diagnostics(
        "a: 1 \n",
        Path::new("/proj/x.yaml"),
        &cfg,
        Path::new("/proj"),
        SourceKind::Yaml,
        PositionEncoding::Utf16,
    );
    assert_eq!(
        diags[0].severity,
        Some(lsp_types::DiagnosticSeverity::WARNING)
    );
}

#[test]
fn diagnostics_for_a_syntax_error_have_no_rule_code() {
    let cfg = yaml_cfg("[rules]\ntrailing-spaces = \"enable\"\n");
    let diags = diagnostics(
        "a: [b\n",
        Path::new("/proj/x.yaml"),
        &cfg,
        Path::new("/proj"),
        SourceKind::Yaml,
        PositionEncoding::Utf16,
    );
    assert_eq!(diags.len(), 1, "a syntax error replaces rule diagnostics");
    assert!(diags[0].code.is_none(), "syntax errors carry no rule code");
}

#[test]
fn diagnostics_lint_embedded_yaml_in_markdown() {
    let cfg = yaml_cfg("[rules]\ntrailing-spaces = \"enable\"\n");
    let diags = diagnostics(
        "# doc\n\n```yaml\na: 1 \n```\n",
        Path::new("/proj/x.md"),
        &cfg,
        Path::new("/proj"),
        SourceKind::Markdown,
        PositionEncoding::Utf16,
    );
    assert_eq!(diags.len(), 1, "the fenced yaml block is linted");
}

#[test]
fn fix_all_edit_replaces_the_whole_document_when_fixable() {
    let cfg = yaml_cfg("[rules]\ntrailing-spaces = \"enable\"\n");
    let edit = fix_all_edit(
        "a: 1 \n",
        Path::new("/proj/x.yaml"),
        &cfg,
        Path::new("/proj"),
        SourceKind::Yaml,
        PositionEncoding::Utf16,
    )
    .expect("a fixable document yields an edit");
    assert_eq!(edit.new_text, "a: 1\n", "the trailing space is removed");
    assert_eq!(
        (edit.range.start.line, edit.range.start.character),
        (0, 0),
        "covers from start"
    );
}

#[test]
fn fix_all_edit_is_none_when_already_clean() {
    let cfg = yaml_cfg("[rules]\ntrailing-spaces = \"enable\"\n");
    assert!(
        fix_all_edit(
            "a: 1\n",
            Path::new("/proj/x.yaml"),
            &cfg,
            Path::new("/proj"),
            SourceKind::Yaml,
            PositionEncoding::Utf16,
        )
        .is_none(),
        "a conforming document needs no edit"
    );
}

#[test]
fn fix_all_edit_fixes_embedded_markdown_yaml() {
    let cfg = yaml_cfg("[rules]\ntrailing-spaces = \"enable\"\n");
    let edit = fix_all_edit(
        "```yaml\na: 1 \n```\n",
        Path::new("/proj/x.md"),
        &cfg,
        Path::new("/proj"),
        SourceKind::Markdown,
        PositionEncoding::Utf16,
    )
    .expect("the fenced block is fixable");
    assert!(
        edit.new_text.contains("a: 1\n"),
        "trailing space removed inside the fence"
    );
}

#[test]
fn fix_all_edit_skips_markdown_with_an_unsupported_bare_cr() {
    let cfg = yaml_cfg("[rules]\ntrailing-spaces = \"enable\"\n");
    // A lone CR (not part of CRLF) makes the markdown host unfixable.
    assert!(
        fix_all_edit(
            "# h\rmore\n\n```yaml\na: 1 \n```\n",
            Path::new("/proj/x.md"),
            &cfg,
            Path::new("/proj"),
            SourceKind::Markdown,
            PositionEncoding::Utf16,
        )
        .is_none(),
        "a bare CR markdown host is left untouched"
    );
}
