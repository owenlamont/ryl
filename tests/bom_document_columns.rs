use std::fs;

use ryl::rules::{
    colons, comments, document_start, hyphens, indentation, trailing_spaces,
};
use tempfile::tempdir;

mod common;
use common::cli::{run, ryl};

fn indent_config() -> indentation::Config {
    indentation::Config::new(
        indentation::SpacesSetting::Fixed(2),
        indentation::IndentSequencesSetting::True,
        true,
    )
}

#[test]
fn document_prefix_bom_is_not_indentation() {
    for newline in ["\n", "\r\n", "\r"] {
        for prefix in ["", "a: a\n...\n"] {
            for body in [
                "---\na: 1\n",
                "a:\n  b: 1\n",
                "- a\n- b\n",
                "? a\n: b\n",
                "|\n  text\n",
                "\"å\n text\"\n",
            ] {
                let input = format!("{prefix}\u{feff}{body}").replace('\n', newline);
                assert_eq!(
                    indentation::check(&input, &indent_config()),
                    [],
                    "{input:?}"
                );
            }
        }
    }
}

#[test]
fn reindent_preserves_bom_and_closes_indicator_gaps() {
    for prefix in ["", "a: a\n...\n"] {
        for (body, expected) in [
            ("  a: 1\n", "a: 1\n"),
            ("a:\n    b: 1\n", "a:\n  b: 1\n"),
            ("-   a: 1\n    b: 2\n", "- a: 1\n  b: 2\n"),
            ("?   a\n:   b\n", "? a\n: b\n"),
            ("# comment\na:\n    b: 1\n", "# comment\na:\n  b: 1\n"),
        ] {
            let input = format!("{prefix}\u{feff}{body}");
            let output = indentation::reindent(&input, &indent_config());
            assert_eq!(
                output.text,
                format!("{prefix}\u{feff}{expected}"),
                "{input:?}"
            );
            assert_eq!(output.refused, []);
            assert_eq!(indentation::check(&output.text, &indent_config()), []);
        }
        for own_line in [false, true] {
            let cfg = indent_config().with_dash_on_own_line(own_line);
            let (body, expected) = if own_line {
                ("- a: 1\n  b: 2\n", "-\n  a: 1\n  b: 2\n")
            } else {
                ("-\n  a: 1\n  b: 2\n", "- a: 1\n  b: 2\n")
            };
            let input = format!("{prefix}\u{feff}{body}");
            let output = indentation::reindent(&input, &cfg);
            assert_eq!(
                output.text,
                format!("{prefix}\u{feff}{expected}"),
                "{input:?}"
            );
            assert_eq!(output.refused, []);
        }
    }
}

#[test]
fn formatter_added_marker_passes_the_same_lint_config() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join(".ryl.toml"),
        "[format]\ndocument-start = 'add'\n[lint.rules.indentation]\nspaces = 'consistent'\n",
    ).unwrap();
    let path = dir.path().join("input.yaml");
    fs::write(&path, "a: a\n...\n\u{feff}\na: 1\n").unwrap();
    for subcommand in ["check", "format", "check"] {
        let (code, stdout, stderr) = run(ryl(dir.path()).arg(subcommand).arg(&path));
        assert_eq!(code, 0, "{subcommand}: {stdout} {stderr}");
    }
    assert_eq!(
        fs::read_to_string(path).unwrap(),
        "---\na: a\n...\n\u{feff}---\n\na: 1\n"
    );
}

#[test]
fn sibling_spacing_rules_keep_bom_and_target_their_whitespace() {
    let prefix = "a: a\n...\n\u{feff}";
    let dash = format!("{prefix}-   å\n");
    let hits = hyphens::check(&dash, &hyphens::Config::new(1));
    assert_eq!((hits[0].line, hits[0].column), (3, 5));
    assert_eq!(
        hyphens::fix(&dash, &hyphens::Config::new(1)),
        Some(format!("{prefix}- å\n"))
    );
    let mapping = format!("{prefix}å :   b\n");
    let hits = colons::check(&mapping, &colons::Config::new(0, 1));
    assert_eq!(
        hits.iter()
            .map(|hit| (hit.line, hit.column))
            .collect::<Vec<_>>(),
        [(3, 3), (3, 7)]
    );
    assert_eq!(
        colons::fix(&mapping, &colons::Config::new(0, 1)),
        Some(format!("{prefix}å: b\n"))
    );
    let comment = format!("{prefix}#bad\na: 1\n");
    let cfg = comments::Config::exact_gap(2, true);
    let hits = comments::check(&comment, &cfg);
    assert_eq!((hits[0].line, hits[0].column), (3, 2));
    assert_eq!(
        comments::fix(&comment, &cfg),
        Some(format!("{prefix}# bad\na: 1\n"))
    );
    let marker = format!("{prefix}---\na: 1\n");
    assert_eq!(
        document_start::check(&marker, &document_start::Config::new(true))
            .iter()
            .map(|hit| hit.line)
            .collect::<Vec<_>>(),
        [1]
    );
    let hits = document_start::check(&marker, &document_start::Config::new(false));
    assert_eq!((hits[0].line, hits[0].column), (3, 1));
    let trailing = format!("{prefix}å: b  \n");
    assert_eq!(
        trailing_spaces::check(&trailing),
        [trailing_spaces::Violation { line: 3, column: 6 }]
    );
    assert_eq!(
        trailing_spaces::fix(&trailing),
        Some(format!("{prefix}å: b\n"))
    );
}

#[test]
fn quoted_bom_is_content_and_keeps_its_column() {
    for input in [
        "\"å\n\u{feff}text\"\n",
        "'å\n\u{feff}text'\n",
        "\"å\n\u{feff}text\"",
    ] {
        let output = indentation::reindent(input, &indent_config());
        assert_eq!(output.text, input);
        let ordinary = input.replace('\u{feff}', "x");
        assert_eq!(
            output.refused,
            indentation::reindent(&ordinary, &indent_config()).refused
        );
        assert_eq!(
            indentation::check(input, &indent_config()),
            indentation::check(&ordinary, &indent_config())
        );
    }
}
