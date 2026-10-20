use std::path::Path;

use ryl::config::YamlLintConfig;
use ryl::format::format_str;
use ryl::rules::{braces, commas, empty_lines, trailing_spaces};
use ryl::text_version::TextVersion;

#[test]
fn byte_edits_merge_in_source_order_and_keep_epoch_guards() {
    let mut source = TextVersion::new("é : [ a,b ]\r\n");
    let edits = vec![
        (10..11, String::new()),
        (2..3, String::new()),
        (9..9, " ".into()),
    ];
    let stale = source.analysis().plan_byte_edits(edits.clone()).unwrap();
    let plan = source.analysis().plan_byte_edits(edits).unwrap();
    assert_eq!(source.apply(plan), Some(true));
    assert_eq!(&*source, "é: [ a, b]\r\n");
    assert_eq!(source.apply(stale), None);
}

#[test]
fn byte_edits_reject_conflicting_or_invalid_ranges() {
    let source = TextVersion::new("é : x\n");
    for ranges in [
        vec![(0, 3), (2, 4)],
        vec![(2, 2), (2, 3)],
        vec![(1, 2)],
        vec![(0, 1)],
        vec![(8, 8)],
        vec![(0, 8)],
        vec![(3, 2)],
        vec![],
    ] {
        assert!(
            source
                .analysis()
                .plan_byte_edits(
                    ranges
                        .into_iter()
                        .map(|(start, end)| (start..end, String::new()))
                        .collect()
                )
                .is_none()
        );
    }
}

#[test]
fn adjacent_byte_edits_and_boundary_insertions_compose() {
    let mut source = TextVersion::new("a b");
    let plan = source
        .analysis()
        .plan_byte_edits(vec![
            (0..1, "é".into()),
            (1..2, ": ".into()),
            (3..3, "\n".into()),
        ])
        .unwrap();
    assert_eq!(source.apply(plan), Some(true));
    assert_eq!(&*source, "é: b\n");
}

#[test]
fn comment_punctuation_and_finishing_edits_share_unicode_lines() {
    assert_format(
        "é : { a :  [ x,y ] } #note  \n\n\n",
        "é: {a: [x, y]}  # note\n",
        "max-blank-lines = 0\n",
    );
}

#[test]
fn finishing_reanalyzes_whitespace_only_lines_before_blank_limits() {
    assert_format(
        "é: x  \n  \n\nnext: y  \n",
        "é: x\nnext: y\n",
        "max-blank-lines = 0\n",
    );
}

#[test]
fn same_length_edit_rejects_old_plan() {
    let mut source = TextVersion::new("a: x\n");
    let stale = source.analysis().plan("a: z\n".to_string());
    let edit = source.analysis().plan("b: x\n".to_string());
    assert_eq!(source.apply(edit), Some(true));
    assert_eq!(source.apply(stale), None);
    assert_eq!(source.analysis().text(), "b: x\n");
}

#[test]
fn identical_documents_have_distinct_epochs() {
    let source = TextVersion::new("a: x\n");
    let mut other = TextVersion::new("a: x\n");
    assert_eq!(
        other.apply(source.analysis().plan("b: x\n".to_string())),
        None
    );
}

#[test]
fn byte_identical_edit_keeps_existing_plans_valid() {
    let mut source = TextVersion::new("a: x\n");
    let pending = source.analysis().plan("a: y\n".to_string());
    let unchanged = source
        .analysis()
        .plan_byte_edits(vec![(3..4, "x".into())])
        .unwrap();
    assert_eq!(source.apply(unchanged), Some(false));
    assert_eq!(source.apply(pending), Some(true));
    assert_eq!(source.into_text(), "a: y\n");
}

#[test]
fn scalar_protection_tracks_spacing_quote_and_header_edits() {
    assert_format(
        "a: 'é'\nb: {  c: [ 1,2 ] }\ns: |+\n  value #body\n  \n\n",
        "a: é\nb: {c: [1, 2]}\ns: |+\n  value #body\n\n\n",
        "",
    );
}

#[test]
fn utf8_byte_length_change_before_block_scalar_rejects_old_plan() {
    let mut source = TextVersion::new("a: x\ns: |+\n  value\n  \n\n");
    let stale = source
        .analysis()
        .plan("a: x\ns: |+\n  value\n\n\n".to_string());
    let edit = source.analysis().plan(source.replace("a: x", "a: é"));
    assert_eq!(source.apply(edit), Some(true));
    assert_eq!(source.apply(stale), None);
    assert_format(&source, "a: é\ns: |+\n  value\n\n\n", "");
}

#[test]
fn collection_conversion_refreshes_scalar_protection() {
    assert_format(
        "a:\n  - 'é'\n  - two\ns: |+\n  value\n  \n\n",
        "a: [é, two]\ns: |+\n  value\n\n\n",
        "sequence-style = 'flow'\nmapping-style = 'flow'\n",
    );
}

#[test]
fn folding_refreshes_scalar_protection_for_the_next_pass() {
    let cfg = YamlLintConfig::from_toml_str(
        "indent-width = 2\nline-length = 16\n[format]\nfold-long-lines = true\n",
    )
    .unwrap();
    let source = "a: 'é'\nb: aaa bbb ccc ddd eee\ns: |+\n  value\n  \n\n";
    let expected = "a: é\nb: aaa bbb ccc\n  ddd eee\ns: |+\n  value\n\n\n";
    let formatted = format_str(source, &cfg, Path::new("unicode.yaml"), &[]);
    assert_eq!(formatted, expected);
    assert_eq!(
        format_str(&formatted, &cfg, Path::new("unicode.yaml"), &[]),
        expected
    );
}

#[test]
fn partial_scalar_events_still_protect_punctuation() {
    let source = "a: ['x,y',z]\nb: {  k: 'a,b'  }\n---\nbad: [\n";
    assert_eq!(
        commas::fix(source, &commas::Config::new(0, 1, 1)).unwrap(),
        "a: ['x,y', z]\nb: {  k: 'a,b'  }\n---\nbad: [\n"
    );
    assert_eq!(
        braces::fix(
            source,
            &braces::Config::new(braces::Forbid::None, 0, 0, 0, 0)
        )
        .unwrap(),
        "a: ['x,y',z]\nb: {k: 'a,b'}\n---\nbad: [\n"
    );
    assert_eq!(trailing_spaces::fix(source), None);
    assert_eq!(
        empty_lines::fix(source, &empty_lines::Config::new(0, 0, 0)),
        None
    );
    assert_format(source, source, "");
}

#[test]
fn reconciled_quote_edit_refreshes_protected_scalars() {
    assert_format(
        "a: 'é'  # ryl disable-line rule:quoted-strings\nb: 'é'\ns: |+\n  value\n  \n\n",
        "a: 'é'  # ryl disable-line rule:quoted-strings\nb: é\ns: |+\n  value\n\n\n",
        "",
    );
}

#[test]
fn newline_and_document_marker_edits_refresh_protected_scalars() {
    assert_format(
        "a: 'é'\r\ns: |+\r\n  value\r\n  \r\n\r\n",
        "---\na: é\ns: |+\n  value\n\n\n...\n",
        "document-start = 'add'\ndocument-end = 'add'\n",
    );
}

fn assert_format(source: &str, expected: &str, settings: &str) {
    let cfg = YamlLintConfig::from_toml_str(&format!(
        "indent-width = 2\n[format]\n{settings}"
    ))
    .unwrap();
    let formatted = format_str(source, &cfg, Path::new("unicode.yaml"), &[]);
    assert_eq!(formatted, expected);
    assert_eq!(
        format_str(&formatted, &cfg, Path::new("unicode.yaml"), &[]),
        expected
    );
}

#[test]
fn bare_cr_edits_refresh_comment_spacing_and_scalar_eof_facts() {
    assert_format(
        "a: \"é\" #comment\rb: [ 1,2 ]\rs: |+\r  value\r  ",
        "a: é  # comment\nb: [1, 2]\ns: |+\n  value\n\n",
        "",
    );
}

#[test]
fn later_document_bom_keeps_distinct_raw_and_indentation_columns() {
    assert_format(
        "\u{feff}a: \"é\" #comment\r\n...\r\n\u{feff}b:\r\n    -  café\r\n    -  \"two\"\r\n",
        "\u{feff}a: é  # comment\n...\n\u{feff}b:\n  - café\n  - two\n",
        "",
    );
}

#[test]
fn reconciled_spacing_refreshes_comment_and_eof_facts() {
    assert_format(
        "a: \"é\" # ryl disable-line rule:comments\nb: \"界\" #plain\nc:  café # ryl disable-line rule:colons\ns: |+\n  value\n  ",
        "a: é # ryl disable-line rule:comments\nb: 界  # plain\nc:  café  # ryl disable-line rule:colons\ns: |+\n  value\n\n",
        "",
    );
}

#[test]
fn partial_late_document_does_not_authorize_snapshot_edits() {
    let source = "a: \"é\" #comment\n...\n---\nb: *missing\n";
    assert_format(source, source, "");
}
