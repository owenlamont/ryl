use ryl_yaml_parser::{
    Marker, Parser, ScalarStyle, Scanner, Span, StrInput, TokenKind,
};

#[test]
fn every_scanned_token_reports_its_kind() {
    let yaml = "%YAML 1.2\n%TAG !e! tag:x,2000:\n%FOO bar\n---\n- !e!t &a [b, {c: *a}]\n- ? k\n  : v # note\n...\n";
    let kinds: Vec<String> = Scanner::new(StrInput::new(yaml))
        .map(|token| format!("{:?}", token.unwrap().token_type().kind()))
        .map(|kind| kind.split([' ', '{', '(']).next().unwrap().to_owned())
        .collect();
    for expected in [
        "StreamStart",
        "VersionDirective",
        "TagDirective",
        "ReservedDirective",
        "DocumentStart",
        "BlockSequenceStart",
        "BlockEntry",
        "Tag",
        "Anchor",
        "FlowSequenceStart",
        "Scalar",
        "FlowEntry",
        "FlowMappingStart",
        "Key",
        "Value",
        "Alias",
        "FlowMappingEnd",
        "FlowSequenceEnd",
        "BlockMappingStart",
        "Comment",
        "BlockEnd",
        "DocumentEnd",
        "StreamEnd",
    ] {
        assert!(
            kinds.iter().any(|kind| kind == expected),
            "{expected} missing from {kinds:?}"
        );
    }
    assert_eq!(
        TokenKind::Scalar {
            style: ScalarStyle::Plain,
            value: "b"
        },
        Scanner::new(StrInput::new("b"))
            .nth(1)
            .unwrap()
            .unwrap()
            .token_type()
            .kind()
    );
}

#[test]
fn double_quoted_escapes_decode_every_named_character() {
    let (event, _) = Parser::new_from_str(r#""\a\v\e\N\_\L\P""#)
        .nth(2)
        .unwrap()
        .unwrap();
    assert_eq!(
        event.scalar().unwrap().0,
        "\u{7}\u{b}\u{1b}\u{85}\u{a0}\u{2028}\u{2029}"
    );
}

#[test]
fn byte_range_needs_offsets_at_both_ends() {
    let with_offsets = Parser::new_from_str("a").nth(2).unwrap().unwrap().1;
    let without = Span::new(with_offsets.start, Marker::new(1, 1, 1));
    assert_eq!(with_offsets.byte_range(), Some(0..1));
    assert_eq!(without.byte_range(), None);
}
