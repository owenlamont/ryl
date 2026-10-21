use granit_parser::{Marker, Span};

#[test]
fn constructors_and_metadata_builders_preserve_unicode_source_range() {
    let source = "!tag é中🙂\n";
    let start = Marker::new(5, 1, 5).with_byte_offset(Some(5));
    let end = Marker::new(8, 1, 8).with_byte_offset(Some(14));
    let tag = Marker::new(0, 1, 0).with_byte_offset(Some(0));

    let original = Span::new(start, end);
    let decorated = original.with_indent(Some(2)).with_tag_start(Some(tag));

    assert_eq!(original.indent, None);
    assert_eq!(original.tag_start(), None);
    assert_eq!(decorated.start, start);
    assert_eq!(decorated.end, end);
    assert_eq!(decorated.indent, Some(2));
    assert_eq!(decorated.tag_start, Some(tag));
    assert_eq!(decorated.tag_start(), Some(tag));
    assert_eq!(decorated.len(), 3);
    assert!(!decorated.is_empty());
    assert_eq!(decorated.byte_range(), Some(5..14));
    assert_eq!(decorated.slice(source), Some("é中🙂"));
    assert_eq!(decorated.with_indent(None).with_tag_start(None), original);
}

#[test]
fn external_open_patterns_and_public_field_updates_remain_supported() {
    let source = "aé中";
    let start = Marker::new(0, 1, 0).with_byte_offset(Some(0));
    let end = Marker::new(3, 1, 3).with_byte_offset(Some(6));
    let mut span = Span::new(start, end)
        .with_indent(Some(0))
        .with_tag_start(Some(start));

    let Span {
        start: actual_start,
        end: actual_end,
        indent,
        tag_start,
        ..
    } = span;
    assert_eq!(actual_start, start);
    assert_eq!(actual_end, end);
    assert_eq!(indent, Some(0));
    assert_eq!(tag_start, Some(start));

    // Non-exhaustiveness restricts construction and closed patterns, not field access.
    span.start = Marker::new(1, 1, 1).with_byte_offset(Some(1));
    span.end = Marker::new(2, 1, 2).with_byte_offset(Some(3));
    span.indent = Some(4);
    span.tag_start = None;
    assert_eq!(span.start.index(), 1);
    assert_eq!(span.end.index(), 2);
    assert_eq!(span.indent, Some(4));
    assert_eq!(span.tag_start(), None);
    assert_eq!(span.len(), 1);
    assert_eq!(span.byte_range(), Some(1..3));
    assert_eq!(span.slice(source), Some("é"));
}

#[test]
fn empty_and_default_spans_need_no_struct_literal() {
    let marker = Marker::new(3, 1, 3).with_byte_offset(Some(6));
    let empty = Span::empty(marker);

    assert_eq!(empty.start, marker);
    assert_eq!(empty.end, marker);
    assert_eq!(empty.indent, None);
    assert_eq!(empty.tag_start(), None);
    assert!(empty.is_empty());
    assert_eq!(empty.byte_range(), Some(6..6));
    assert_eq!(empty.slice("aé中"), Some(""));

    let default = Span::default();
    assert_eq!(default.start, Marker::default());
    assert_eq!(default.end, Marker::default());
    assert_eq!(default.indent, None);
    assert_eq!(default.tag_start(), None);
    assert!(default.is_empty());
    assert_eq!(default.byte_range(), None);
    assert_eq!(default.slice("aé中"), None);
}
