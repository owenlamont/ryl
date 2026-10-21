use granit_parser::{Parser, ScanError};

fn first_error_from_str(input: &str) -> ScanError {
    Parser::new_from_str(input)
        .find_map(Result::err)
        .expect("input should produce a parser error")
}

fn first_error_from_iter(input: &str) -> ScanError {
    Parser::new_from_iter(input.chars())
        .find_map(Result::err)
        .expect("input should produce a parser error")
}

#[test]
fn test_non_ascii_comment_start() {
    let yaml = "\
# A \u{AC00}
a1:
  b: 1
a2:
  b: 2
";
    for item in Parser::new_from_str(yaml) {
        if let Err(e) = item {
            panic!("Error: {}", e.info());
        }
    }
}

#[test]
fn test_non_ascii_comment_many() {
    let yaml = "\
# A \u{AC00}\
\u{AC00}: \u{AC00}
a1: # A \u{AC00}
  b: 1 # A \u{AC00}
a2: # A \u{AC00} # A \u{AC00}
  b: 2 # A \u{AC00}\
  c: [ 1, 2, 3 ] # \u{AC00}
  d: # \u{AC00}
    - 1 \u{AC00}
    - 2 \u{AC00}
    - 3 \u{AC00}
# A \u{AC00}
";

    for item in Parser::new_from_str(yaml) {
        if let Err(e) = item {
            panic!("Unexpected error: {}", e.info());
        }
    }
}

#[test]
fn test_non_ascii_comment() {
    let yaml = "\
a1:
  b: 1
# A \u{AC00}
a2:
  b: 2
";

    for item in Parser::new_from_str(yaml) {
        if let Err(e) = item {
            panic!("Unexpected error: {}", e.info());
        }
    }
}

#[test]
fn test_non_ascii_comment_error_marker_matches_between_backends() {
    let yaml = "# \u{1F602}\nkey: [1, 2]]\n";

    let str_error = first_error_from_str(yaml);
    let iter_error = first_error_from_iter(yaml);

    assert_eq!(str_error.info(), "misplaced bracket");
    assert_eq!(iter_error.info(), str_error.info());
    assert_eq!(
        (
            iter_error.marker().index(),
            iter_error.marker().line(),
            iter_error.marker().col(),
        ),
        (
            str_error.marker().index(),
            str_error.marker().line(),
            str_error.marker().col(),
        )
    );
    assert_eq!(str_error.marker().index(), 15);
    assert_eq!(str_error.marker().line(), 2);
    assert_eq!(str_error.marker().col(), 11);
}

#[test]
fn non_ascii_reserved_directive_marker_matches_between_backends() {
    let yaml = "%FOO café\n%YAML 1.1 1.2\n---\n";

    let str_error = first_error_from_str(yaml);
    let iter_error = first_error_from_iter(yaml);

    assert_eq!(iter_error.info(), str_error.info());
    assert_eq!(
        (
            iter_error.marker().index(),
            iter_error.marker().line(),
            iter_error.marker().col(),
        ),
        (
            str_error.marker().index(),
            str_error.marker().line(),
            str_error.marker().col(),
        )
    );
    assert_eq!(str_error.marker().index(), 10);
    assert_eq!(str_error.marker().line(), 2);
    assert_eq!(str_error.marker().col(), 0);
}
