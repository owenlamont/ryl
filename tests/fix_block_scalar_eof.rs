use ryl::rules::{document_end, new_line_at_end_of_file};

// Verified against ruamel.yaml and PyYAML, which follow the spec here: a break after an
// unterminated last line joins a `+` or clipped scalar's value, and not a `-` one's.
const UNSTRIPPED: [&str; 4] =
    ["a: |+\n  a\n ", "a: |+\n  a", "a: |\n  a", "a: >\n  a\n  b"];

#[test]
fn the_final_newline_fixes_leave_an_unstripped_scalar_at_the_end_of_file() {
    let marker = document_end::Config::new(true);
    for input in UNSTRIPPED {
        assert_eq!(new_line_at_end_of_file::fix(input, "\n"), None, "{input:?}");
        assert_eq!(document_end::fix(input, &marker), None, "{input:?}");
        assert!(new_line_at_end_of_file::check(input).is_some(), "{input:?}");
    }
    assert_eq!(
        document_end::fix("- >+2\n    a\n", &marker).as_deref(),
        Some("- >+2\n    a\n...\n")
    );
}

#[test]
fn the_final_newline_fixes_still_end_other_files() {
    let marker = document_end::Config::new(true);
    for (input, newline, ended) in [
        ("a: |-\n  a", "a: |-\n  a\n", "a: |-\n  a\n...\n"),
        (
            "a: |+\n  a\n# c",
            "a: |+\n  a\n# c\n",
            "a: |+\n  a\n# c\n...\n",
        ),
        (
            "a: |+\n  a\nb: 1",
            "a: |+\n  a\nb: 1\n",
            "a: |+\n  a\nb: 1\n...\n",
        ),
    ] {
        assert_eq!(
            new_line_at_end_of_file::fix(input, "\n").as_deref(),
            Some(newline)
        );
        assert_eq!(document_end::fix(input, &marker).as_deref(), Some(ended));
    }
}
