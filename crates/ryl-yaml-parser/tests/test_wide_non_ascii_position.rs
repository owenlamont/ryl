use granit_parser::{Event, Parser};

#[test]
fn test_wide_non_ascii_positions() {
    let yaml = "emoji: \u{1F602}\nnext: item";
    let mut parser = Parser::new_from_str(yaml);

    while !matches!(parser.next().unwrap().unwrap().0, Event::MappingStart(..)) {}

    // key: "emoji"
    let (event, span) = parser.next().unwrap().unwrap();
    assert!(matches!(event, Event::Scalar(ref v, ..) if v == "emoji"));
    assert_eq!(span.start.index(), 0);
    assert_eq!(span.end.index(), 5);

    // value: "\u{1F602}"
    let (event, span) = parser.next().unwrap().unwrap();
    if let Event::Scalar(v, _, _, _) = event {
        assert_eq!(v, "\u{1F602}");
        assert_eq!(span.start.index(), 7);
        assert_eq!(span.end.index(), 8);
        assert_eq!(span.len(), 1);
        assert_eq!(span.start.byte_offset(), Some(7));
        assert_eq!(span.end.byte_offset(), Some(11));
        assert_eq!(span.start.line(), 1);
        assert_eq!(span.start.col(), 7);
    }

    // next key: "next"
    let (event, span) = parser.next().unwrap().unwrap();
    if let Event::Scalar(v, _, _, _) = event {
        assert_eq!(v, "next");
        assert_eq!(span.start.index(), 9);
        assert_eq!(span.start.byte_offset(), Some(12));
        assert_eq!(span.start.line(), 2);
        assert_eq!(span.start.col(), 0);
    }
}

#[test]
fn test_wide_chars_in_comments() {
    let yaml = "key: value # \u{1F602} emoji comment\nnext: item";
    let mut parser = Parser::new_from_str(yaml);

    while !matches!(parser.next().unwrap().unwrap().0, Event::Scalar(ref v, ..) if v == "value") {}

    let (event, span) = parser.next().unwrap().unwrap();
    if let Event::Scalar(v, ..) = event {
        assert_eq!(v, "next");
        assert_eq!(span.start.line(), 2);
        assert_eq!(span.start.col(), 0);
        assert_eq!(span.start.index(), 29);
        assert_eq!(span.start.byte_offset(), Some(32));
    }
}

#[test]
fn test_block_scalar_wide_chars() {
    let yaml = "key: |\n  \u{1F602}\n  \u{1F680}";
    let mut parser = Parser::new_from_str(yaml);
    while !matches!(parser.next().unwrap().unwrap().0, Event::Scalar(ref v, ..) if v == "key") {}

    let (event, span) = parser.next().unwrap().unwrap();
    if let Event::Scalar(v, _, _, _) = event {
        assert_eq!(v, "\u{1F602}\n\u{1F680}\n");
        assert_eq!(span.start.line(), 2);
        assert_eq!(span.start.col(), 2);
        assert_eq!(span.end.line(), 3);
        assert_eq!(span.end.col(), 3);
    }
}
