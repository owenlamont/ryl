#![cfg(feature = "lsp")]
//! Unit tests for the language server's pure bridges: position/URI encoding and
//! the lint/fix-to-LSP analysis layer. These need no live connection, so they
//! pin the fiddly parts (UTF-8/16/32 column math over astral-plane characters,
//! `file:` URI decoding, whole-document edit ranges) cheaply and exhaustively.

use std::path::Path;
use std::sync::atomic::AtomicBool;

use lsp_server::{ErrorCode, RequestId};
use lsp_types::{
    Diagnostic, NumberOrString, Position, PositionEncodingKind, PrepareRenameResponse,
    Range,
};
use tempfile::tempdir;

use ryl::config::{SourceKind, YamlLintConfig};
use ryl::lsp::analysis::{diagnostics, fix_all_edit, fix_rule_edit};
use ryl::lsp::encoding::{
    PositionEncoding, full_range, negotiate, offset_at, path_to_uri, problem_range,
    range_contains, uri_to_path,
};
use ryl::lsp::hover::hover;
use ryl::lsp::rename::{prepare_rename, rename_edits};
use ryl::lsp::{OpenText, Settings, workspace_response, workspace_scan};

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
    // "a: 😀 ": the trailing space is the 5th code point (ryl reports col 5).
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
fn range_contains_is_end_exclusive_except_for_zero_width() {
    let one_char = Range::new(Position::new(0, 3), Position::new(0, 4));
    assert!(
        range_contains(one_char, Position::new(0, 3)),
        "the start is inclusive"
    );
    assert!(
        !range_contains(one_char, Position::new(0, 4)),
        "the end is exclusive, so a cursor just past the token does not match"
    );
    let zero_width = Range::new(Position::new(0, 3), Position::new(0, 3));
    assert!(
        range_contains(zero_width, Position::new(0, 3)),
        "a zero-width range (an end-of-line diagnostic) matches at its point"
    );
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

// --- offset_at: the inverse of the forward column conversion (incremental sync) ---

#[test]
fn offset_at_counts_code_units_across_an_astral_char() {
    // "a: 😀 b": the emoji is 1 code point / 2 UTF-16 units / 4 UTF-8 bytes, so the byte
    // offset of `b` (8) is reached at a different `character` per encoding.
    let text = "a: \u{1F600} b";
    assert_eq!(
        offset_at(text, Position::new(0, 6), PositionEncoding::Utf16),
        8,
        "UTF-16: a,:,space=3 + emoji=2 + space=1 -> char 6 is byte 8"
    );
    assert_eq!(
        offset_at(text, Position::new(0, 8), PositionEncoding::Utf8),
        8,
        "UTF-8: the emoji is 4 bytes"
    );
    assert_eq!(
        offset_at(text, Position::new(0, 5), PositionEncoding::Utf32),
        8,
        "UTF-32: the emoji is one unit"
    );
}

#[test]
fn offset_at_snaps_a_mid_surrogate_character_to_the_char_start() {
    // UTF-16 character 4 lands inside the surrogate pair of the emoji at byte 3.
    let text = "a: \u{1F600} b";
    assert_eq!(
        offset_at(text, Position::new(0, 4), PositionEncoding::Utf16),
        3,
        "a mid-char position snaps back to the char start"
    );
}

#[test]
fn offset_at_clamps_columns_and_lines_past_the_end() {
    let text = "ab\ncd";
    assert_eq!(
        offset_at(text, Position::new(0, 99), PositionEncoding::Utf16),
        2,
        "a character past the line clamps to its content end"
    );
    assert_eq!(
        offset_at(text, Position::new(1, 1), PositionEncoding::Utf16),
        4,
        "second line, second char"
    );
    assert_eq!(
        offset_at(text, Position::new(9, 0), PositionEncoding::Utf16),
        text.len(),
        "a line past the document clamps to the text end"
    );
}

#[test]
fn offset_at_handles_a_phantom_line_after_a_trailing_break() {
    assert_eq!(
        offset_at("ab\n", Position::new(1, 0), PositionEncoding::Utf16),
        3,
        "the position after a trailing break is the text end"
    );
    assert_eq!(
        offset_at("", Position::new(0, 0), PositionEncoding::Utf16),
        0,
        "empty text"
    );
}

// --- path_to_uri: inverse of uri_to_path, round-tripping local paths ---

#[test]
fn path_to_uri_round_trips_a_path_with_spaces() {
    let uri = path_to_uri(Path::new("/home/u/a b.yaml"));
    assert_eq!(
        uri.as_str(),
        "file:///home/u/a%20b.yaml",
        "space is encoded"
    );
    assert_eq!(
        uri_to_path(uri.as_str()).expect("uri -> path"),
        Path::new("/home/u/a b.yaml"),
        "round-trips back to the original path"
    );
}

#[test]
fn path_to_uri_adds_the_leading_slash_for_a_drive_path() {
    let uri = path_to_uri(Path::new("C:/proj/x.yaml"));
    assert_eq!(
        uri.as_str(),
        "file:///C:/proj/x.yaml",
        "drive gets a leading slash"
    );
}

// --- fix_rule_edit: a single rule's safe fix in isolation ---

#[test]
fn fix_rule_edit_fixes_only_the_named_rule() {
    // Two fixable problems: a comma-spacing issue and a trailing space.
    let cfg = yaml_cfg("[rules]\ntrailing-spaces = \"enable\"\ncommas = \"enable\"\n");
    let source = "a: [1 ,2] \n";
    let trailing = fix_rule_edit(
        source,
        Path::new("/proj/x.yaml"),
        &cfg,
        Path::new("/proj"),
        SourceKind::Yaml,
        PositionEncoding::Utf16,
        "trailing-spaces",
    )
    .expect("trailing-spaces is fixable here");
    assert_eq!(
        trailing.new_text, "a: [1 ,2]\n",
        "only the trailing space is removed; the comma spacing is left for its own rule"
    );
    let commas = fix_rule_edit(
        source,
        Path::new("/proj/x.yaml"),
        &cfg,
        Path::new("/proj"),
        SourceKind::Yaml,
        PositionEncoding::Utf16,
        "commas",
    )
    .expect("commas is fixable here");
    assert!(
        commas.new_text.contains("[1, 2]") && commas.new_text.ends_with("] \n"),
        "only comma spacing is fixed; the trailing space remains: {:?}",
        commas.new_text
    );
}

#[test]
fn fix_rule_edit_is_none_for_an_unfixable_rule_or_markdown() {
    let cfg = yaml_cfg("[rules]\ntrailing-spaces = \"enable\"\nanchors = \"enable\"\n");
    assert!(
        fix_rule_edit(
            "a: 1 \n",
            Path::new("/proj/x.yaml"),
            &cfg,
            Path::new("/proj"),
            SourceKind::Yaml,
            PositionEncoding::Utf16,
            "anchors",
        )
        .is_none(),
        "anchors has no safe fix"
    );
    assert!(
        fix_rule_edit(
            "```yaml\na: 1 \n```\n",
            Path::new("/proj/x.md"),
            &cfg,
            Path::new("/proj"),
            SourceKind::Markdown,
            PositionEncoding::Utf16,
            "trailing-spaces",
        )
        .is_none(),
        "per-rule fix is not offered for markdown"
    );
}

// --- hover: rule id + message + docs link for a covered position ---

fn diagnostic(rule: Option<&str>, range: Range, message: &str) -> Diagnostic {
    Diagnostic {
        range,
        code: rule.map(|rule| NumberOrString::String(rule.to_string())),
        message: message.to_string(),
        ..Default::default()
    }
}

fn one_char(line: u32, character: u32) -> Range {
    Range::new(
        Position::new(line, character),
        Position::new(line, character + 1),
    )
}

#[test]
fn hover_shows_the_rule_message_and_link_for_a_covered_position() {
    let diags = vec![diagnostic(
        Some("trailing-spaces"),
        one_char(0, 4),
        "trailing spaces",
    )];
    let hover = hover(&diags, Position::new(0, 4)).expect("position is covered");
    let lsp_types::HoverContents::Markup(markup) = hover.contents else {
        panic!("expected markup contents");
    };
    assert!(markup.value.contains("trailing-spaces"), "names the rule");
    assert!(
        markup.value.contains("trailing spaces"),
        "shows the message"
    );
    assert!(
        markup.value.contains("https://ryl-docs.pages.dev/rules/"),
        "links to the rules reference"
    );
}

#[test]
fn hover_is_none_away_from_any_diagnostic() {
    let diags = vec![diagnostic(Some("colons"), one_char(0, 4), "x")];
    assert!(
        hover(&diags, Position::new(2, 0)).is_none(),
        "no diagnostic covers the position"
    );
}

#[test]
fn hover_lists_every_overlapping_diagnostic() {
    let diags = vec![
        diagnostic(Some("colons"), one_char(0, 3), "colon issue"),
        diagnostic(None, one_char(0, 3), "syntax issue"),
    ];
    let hover = hover(&diags, Position::new(0, 3)).expect("covered");
    let lsp_types::HoverContents::Markup(markup) = hover.contents else {
        panic!("expected markup");
    };
    assert!(markup.value.contains("colon issue"), "first diagnostic");
    assert!(
        markup.value.contains("syntax issue"),
        "rule-less diagnostic also listed"
    );
    assert!(
        markup.value.contains("**ryl**"),
        "rule-less heading is bare ryl"
    );
}

// --- rename: anchor/alias spans, name validation, document scoping ---

#[test]
fn prepare_rename_reports_the_name_range_on_an_anchor() {
    // "a: &anchor 1": the name `anchor` occupies 0-based chars 4..10.
    let response = prepare_rename(
        "a: &anchor 1\nb: *anchor\n",
        Position::new(0, 6),
        PositionEncoding::Utf16,
    )
    .expect("position is on an anchor name");
    let PrepareRenameResponse::RangeWithPlaceholder { range, placeholder } = response
    else {
        panic!("expected a range + placeholder");
    };
    assert_eq!(placeholder, "anchor", "the current name is the placeholder");
    assert_eq!(
        (range.start.character, range.end.character),
        (4, 10),
        "the range covers the name, excluding the & sigil"
    );
}

#[test]
fn prepare_rename_is_none_off_an_anchor() {
    assert!(
        prepare_rename(
            "plain: text\n",
            Position::new(0, 0),
            PositionEncoding::Utf16
        )
        .is_none(),
        "an ordinary scalar is not renameable"
    );
}

#[test]
fn rename_edits_rewrite_the_anchor_and_its_aliases() {
    let edits = rename_edits(
        "a: &anchor 1\nb: *anchor\n",
        Position::new(0, 6),
        "renamed",
        PositionEncoding::Utf16,
    )
    .expect("valid rename")
    .expect("position is on an anchor");
    assert_eq!(edits.len(), 2, "the anchor and its one alias are renamed");
    assert!(
        edits.iter().all(|edit| edit.new_text == "renamed"),
        "every edit inserts the new name"
    );
}

#[test]
fn rename_edits_are_scoped_to_one_document() {
    // The same name in two documents (separated by `---`) is two distinct anchors.
    let edits = rename_edits(
        "a: &x 1\n---\nb: &x 2\n",
        Position::new(0, 4),
        "y",
        PositionEncoding::Utf16,
    )
    .expect("valid rename")
    .expect("on the first anchor");
    assert_eq!(
        edits.len(),
        1,
        "only the first document's anchor is renamed"
    );
    assert_eq!(edits[0].range.start.line, 0, "the edit is on line 0");
}

#[test]
fn rename_edits_reject_an_illegal_new_name() {
    let source = "a: &anchor 1\n";
    // Whitespace, flow indicators, the empty string, and control characters (which
    // LSP/JSON can carry escaped, e.g. NUL) are all rejected.
    for bad in ["with space", "a,b", "[x]", "", "ax\u{0}y"] {
        assert!(
            rename_edits(source, Position::new(0, 6), bad, PositionEncoding::Utf16)
                .is_err(),
            "{bad:?} is not a legal anchor name"
        );
    }
}

#[test]
fn rename_edits_are_none_off_an_anchor() {
    assert!(
        rename_edits(
            "plain: text\n",
            Position::new(0, 0),
            "x",
            PositionEncoding::Utf16
        )
        .expect("no error")
        .is_none(),
        "nothing to rename away from an anchor"
    );
}

#[test]
fn rename_edits_reject_a_name_that_collides_with_another_anchor() {
    // `*x` resolves to `&x`; renaming `x` -> `y` would silently rebind it to `&y`'s value.
    let text = "a: &x 1\nb: &y 2\nc: *x\n";
    assert!(
        rename_edits(text, Position::new(0, 4), "y", PositionEncoding::Utf16).is_err(),
        "renaming onto an existing anchor name is rejected"
    );
    assert!(
        rename_edits(text, Position::new(0, 4), "z", PositionEncoding::Utf16)
            .expect("a fresh name is valid")
            .is_some(),
        "renaming to an unused name is allowed"
    );
    assert!(
        rename_edits(text, Position::new(0, 4), "x", PositionEncoding::Utf16)
            .expect("the same name is valid")
            .is_some(),
        "renaming to the same name is a no-op, not a collision"
    );
}

// --- workspace_scan / workspace_response: the background pull's pure core ---

fn workspace_project() -> tempfile::TempDir {
    // An adjacent .ryl.toml shields config discovery from the walk (no HOME needed).
    let dir = tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join(".ryl.toml"),
        "[rules]\ntrailing-spaces = \"enable\"\n",
    )
    .expect("config");
    dir
}

#[test]
fn workspace_scan_lints_each_root_file() {
    let dir = workspace_project();
    std::fs::write(dir.path().join("bad.yaml"), "a: 1 \n").expect("yaml");
    let reports = workspace_scan(
        &[dir.path().to_path_buf()],
        &OpenText::new(),
        &Settings::default(),
        PositionEncoding::Utf16,
        &AtomicBool::new(false),
    )
    .expect("an uncancelled scan returns reports");
    assert!(!reports.is_empty(), "the flagged file is reported");
}

#[test]
fn workspace_scan_returns_none_when_cancelled() {
    let dir = workspace_project();
    // A file must exist so the per-item cancel check in the parallel map runs.
    std::fs::write(dir.path().join("a.yaml"), "a: 1\n").expect("yaml");
    assert!(
        workspace_scan(
            &[dir.path().to_path_buf()],
            &OpenText::new(),
            &Settings::default(),
            PositionEncoding::Utf16,
            &AtomicBool::new(true),
        )
        .is_none(),
        "a cancelled scan yields no report"
    );
}

#[test]
fn workspace_response_is_ok_or_cancelled() {
    let ok = workspace_response(RequestId::from(1), Some(Vec::new()));
    assert!(ok.error.is_none(), "a completed scan is an ok response");
    let cancelled = workspace_response(RequestId::from(2), None);
    assert_eq!(
        cancelled.error.expect("a cancelled scan is an error").code,
        ErrorCode::RequestCanceled as i32,
        "cancellation maps to the RequestCancelled code"
    );
}
