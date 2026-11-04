use super::representation::{
    BLOCK_HEADER, first_content_line, scalar_tokens, yaml_lines,
};
use ryl::config::YamlLintConfig;
use ryl::config_schema::{LineEndingTarget, MarkerTarget, QuoteStyleTarget};
use ryl_yaml_parser::{ScalarStyle, Scanner, StrInput, TokenType};
use similar::{DiffTag, TextDiff};

pub(super) fn scalar_value(output: &str, index: usize) -> Option<noyalib::Value> {
    let tokens = scalar_tokens(output);
    let (span, style) = tokens.get(index)?;
    if !matches!(style, ScalarStyle::Literal | ScalarStyle::Folded) {
        return noyalib::from_str(span.slice(output)?).ok();
    }
    let first = first_content_line(output, span);
    let lines = yaml_lines(output);
    let header = *lines.get(first.checked_sub(2)?)?;
    let captures = BLOCK_HEADER.captures(header.trim_end_matches(['\r', '\n']))?;
    let indicator = &captures[1];
    let explicit = indicator.chars().find_map(|ch| ch.to_digit(10));
    let parent = explicit.map_or(0, |indent| {
        span.indent
            .unwrap_or(indent as usize)
            .saturating_sub(indent as usize)
    });
    let begin: usize = lines[..first - 1].iter().map(|line| line.len()).sum();
    let body = output.get(begin..span.end.byte_offset()?)?;
    let mut fragment = format!("{}a: {indicator}\n{body}", " ".repeat(parent));
    // noyalib needs a terminal break to retain the final block line.
    if !body.is_empty() && !body.ends_with(['\r', '\n']) {
        fragment.push('\n');
    }
    let mut value: std::collections::BTreeMap<String, noyalib::Value> =
        noyalib::from_str(&fragment).ok()?;
    value.remove("a")
}

pub fn refused(
    input: &str,
    output: &str,
    problem: &ryl::lint::LintProblem,
    notices: &[ryl::lint::LintProblem],
    before: &[ryl::lint::LintProblem],
) -> bool {
    let mut input_lines = yaml_lines(input);
    let mut output_lines = yaml_lines(output);
    if problem.rule != Some("new-lines") {
        for line in input_lines.iter_mut().chain(&mut output_lines) {
            *line = line.trim_end_matches(['\r', '\n']);
        }
    }
    if problem.rule == Some("indentation") {
        for line in input_lines.iter_mut().chain(&mut output_lines) {
            *line = line.trim_end_matches([' ', '\t']);
        }
    }
    let unchanged_input_line = if input == output {
        Some(problem.line)
    } else {
        TextDiff::from_slices(&input_lines, &output_lines)
            .ops()
            .iter()
            .find(|op| {
                op.tag() == DiffTag::Equal
                    && op.new_range().contains(&(problem.line - 1))
            })
            .map(|op| op.old_range().start + problem.line - op.new_range().start)
    }
    .or_else(|| {
        (output_lines
            .first()
            .map(|line| line.trim_end_matches(['\r', '\n']))
            == Some("---")
            && input_lines
                .first()
                .map(|line| line.trim_end_matches(['\r', '\n']))
                != Some("---")
            && notices.iter().any(|notice| {
                notice.message == "cannot re-indent this document safely"
            }))
        .then(|| problem.line.checked_sub(1))
        .flatten()
    });
    let unchanged_problem = before.iter().any(|old| {
        old.rule == problem.rule
            && old.message == problem.message
            && old.column == problem.column
            && Some(old.line) == unchanged_input_line
    }) || unchanged_fragment(input, output, problem, before)
        || unchanged_quoted_scalar(input, output, problem, before)
        || unchanged_boundary(input, output, problem, before);
    if unchanged_problem
        && notices.iter().any(|notice| {
            notice.message
                == "cannot format this file safely: block scalar value may change"
                || (notice.rule.is_none()
                    && notice.message.starts_with("syntax error:"))
        })
    {
        return true;
    }
    notices.iter().any(|notice| {
        if notice.rule != problem.rule {
            return false;
        }
        if unchanged_problem {
            return true;
        }
        notice.line == problem.line
            && refusal_column(output, notice) == Some(problem.column)
            && (notice.message == problem.message
                || match notice.rule {
                    Some("brackets") => notice.message.starts_with("cannot convert to block safely:")
                        && problem.message == ryl::rules::brackets::FORBID_MESSAGE,
                    Some("braces") => notice.message.starts_with("cannot convert to block safely:")
                        && problem.message == ryl::rules::braces::FORBID_MESSAGE,
                    Some("hyphens") => notice.message == "cannot move this mapping below its `-`: a comment follows the `-`"
                        && problem.message == ryl::rules::hyphens::MESSAGE_DASH_ON_OWN_LINE,
                    Some("indentation") => notice.message == "cannot re-indent this document safely"
                        && problem.message.starts_with("wrong indentation:"),
                    _ => false,
                })
    })
}

fn unchanged_fragment(
    input: &str,
    output: &str,
    problem: &ryl::lint::LintProblem,
    before: &[ryl::lint::LintProblem],
) -> bool {
    if !matches!(
        problem.rule,
        Some("colons" | "hyphens" | "comments" | "indentation" | "trailing-spaces")
    ) {
        return false;
    }
    let scalar_identity =
        if matches!(problem.rule, Some("indentation" | "colons" | "hyphens")) {
            let tokens = scalar_tokens(output);
            let Some(index) = tokens
                .iter()
                .position(|(span, _)| span.start.line() == problem.line)
            else {
                return false;
            };
            let old = scalar_tokens(input);
            let Some((span, _)) = old.get(index) else {
                return false;
            };
            if !scalar_value(input, index)
                .is_some_and(|value| Some(value) == scalar_value(output, index))
            {
                return false;
            }
            Some(span.start.line())
        } else {
            None
        };
    let normalize = |source: &str| {
        yaml_lines(source)
            .into_iter()
            .map(|line| {
                let content = line.trim_end_matches(['\r', '\n']);
                format!(
                    "{content}{}",
                    if line.ends_with(['\r', '\n']) {
                        "\n"
                    } else {
                        ""
                    }
                )
            })
            .collect::<String>()
    };
    let (input, output) = (normalize(input), normalize(output));
    let Some(at) = diagnostic_point(&output, problem.line, problem.column) else {
        return false;
    };
    let chars: Vec<_> = output.chars().collect();
    let line_start = chars[..at]
        .iter()
        .rposition(|&ch| ch == '\n')
        .map_or(0, |i| i + 1);
    let line_end = chars[at..]
        .iter()
        .position(|&ch| ch == '\n')
        .map_or(chars.len(), |i| at + i);
    let mut start = at;
    let mut end = (at + 1).min(chars.len());
    match problem.rule {
        Some("indentation") => {
            start = line_start;
            end = chars[line_start..line_end]
                .iter()
                .position(|ch| !matches!(ch, ' ' | '\t'))
                .map_or(line_end, |i| line_start + i + 1);
        }
        Some("comments") => {
            let comment = Scanner::new(StrInput::new(&output))
                .map_while(Result::ok)
                .find_map(|token| {
                    let (span, kind) = token.into_parts();
                    matches!(kind, TokenType::Comment(_))
                        .then_some(span)
                        .filter(|span| span.start.line() == problem.line)
                });
            let Some(comment) = comment else {
                return false;
            };
            start = line_start + comment.start.col();
            end = line_end;
        }
        Some("trailing-spaces") => {
            start = line_end;
            end = (line_end + 1).min(chars.len());
        }
        _ => {}
    }
    while start > line_start && matches!(chars[start - 1], ' ' | '\t') {
        start -= 1;
    }
    while end < line_end && matches!(chars[end], ' ' | '\t') {
        end += 1;
    }
    if problem.rule != Some("indentation") {
        start = start.saturating_sub(1);
        end = (end + 1).min(chars.len());
    }
    TextDiff::from_chars(&input, &output)
        .ops()
        .iter()
        .any(|op| {
            if op.tag() != DiffTag::Equal
                || op.new_range().start > start
                || op.new_range().end < end
            {
                return false;
            }
            let old_at = op.old_range().start + at - op.new_range().start;
            before.iter().any(|old| {
                old.rule == problem.rule
                    && old.message == problem.message
                    && scalar_identity.is_none_or(|line| line == old.line)
                    && diagnostic_point(&input, old.line, old.column) == Some(old_at)
            })
        })
}

fn unchanged_quoted_scalar(
    input: &str,
    output: &str,
    problem: &ryl::lint::LintProblem,
    before: &[ryl::lint::LintProblem],
) -> bool {
    if problem.rule != Some("quoted-strings") {
        return false;
    }
    let tokens = scalar_tokens(output);
    let Some((index, (_, style))) = tokens.iter().enumerate().find(|(_, (span, _))| {
        (span.start.line(), span.start.col() + 1) == (problem.line, problem.column)
    }) else {
        return false;
    };
    let old = scalar_tokens(input);
    let Some((span, old_style)) = old.get(index) else {
        return false;
    };
    style == old_style
        && scalar_value(input, index)
            .is_some_and(|value| Some(value) == scalar_value(output, index))
        && before.iter().any(|old| {
            old.rule == problem.rule
                && old.message == problem.message
                && (old.line, old.column) == (span.start.line(), span.start.col() + 1)
        })
}

fn unchanged_boundary(
    input: &str,
    output: &str,
    problem: &ryl::lint::LintProblem,
    before: &[ryl::lint::LintProblem],
) -> bool {
    let same = |line, column| {
        before.iter().any(|old| {
            old.rule == problem.rule
                && old.message == problem.message
                && (old.line, old.column) == (line, column)
        })
    };
    match problem.rule {
        Some("new-lines") => {
            let ending = |text: &str| {
                text.find(['\r', '\n']).map(|at| {
                    (
                        text[..at].chars().count() + 1,
                        if text[at..].starts_with("\r\n") {
                            "\r\n"
                        } else if text.as_bytes()[at] == b'\r' {
                            "\r"
                        } else {
                            "\n"
                        },
                    )
                })
            };
            match (ending(input), ending(output)) {
                (Some((old_column, old)), Some((column, new))) => {
                    old == new
                        && (problem.line, problem.column) == (1, column)
                        && same(1, old_column)
                }
                _ => false,
            }
        }
        Some("new-line-at-end-of-file") => {
            match (
                ryl::rules::new_line_at_end_of_file::check(input),
                ryl::rules::new_line_at_end_of_file::check(output),
            ) {
                (Some(old), Some(new)) => {
                    (problem.line, problem.column) == (new.line, new.column)
                        && same(old.line, old.column)
                }
                _ => false,
            }
        }
        Some(rule @ ("document-start" | "document-end")) => {
            let positions = |text: &str| {
                let mut positions = Vec::new();
                for required in [true, false] {
                    if rule == "document-start" {
                        positions.extend(
                            ryl::rules::document_start::check(
                                text,
                                &ryl::rules::document_start::Config::new(required),
                            )
                            .into_iter()
                            .map(|v| (v.line, v.column, required)),
                        );
                    } else {
                        positions.extend(
                            ryl::rules::document_end::check(
                                text,
                                &ryl::rules::document_end::Config::new(required),
                            )
                            .into_iter()
                            .map(|v| (v.line, v.column, required)),
                        );
                    }
                }
                positions.sort_unstable();
                positions
            };
            let (old, new) = (positions(input), positions(output));
            old.len() == new.len()
                && new
                    .iter()
                    .position(|&(line, column, _)| {
                        (line, column) == (problem.line, problem.column)
                    })
                    .is_some_and(|index| {
                        let (line, column, state) = old[index];
                        state == new[index].2 && same(line, column)
                    })
        }
        _ => false,
    }
}

fn diagnostic_point(text: &str, line: usize, column: usize) -> Option<usize> {
    let lines = yaml_lines(text);
    let index = line.checked_sub(1)?;
    let prefix = lines.get(..index)?;
    let content = lines.get(index)?.trim_end_matches(['\r', '\n']);
    let column = column.checked_sub(1)?;
    (column <= content.chars().count()).then(|| {
        prefix
            .iter()
            .map(|line| line.chars().count())
            .sum::<usize>()
            + column
    })
}

fn refusal_column(output: &str, notice: &ryl::lint::LintProblem) -> Option<usize> {
    if notice.rule != Some("hyphens")
        || notice.message
            != "cannot move this mapping below its `-`: a comment follows the `-`"
    {
        return Some(notice.column);
    }
    let mut dash = None;
    let lines = yaml_lines(output);
    for token in Scanner::new(StrInput::new(output)).map_while(Result::ok) {
        let (span, kind) = token.into_parts();
        match kind {
            TokenType::BlockEntry => {
                let prefix: String = lines
                    .get(span.start.line() - 1)
                    .copied()
                    .unwrap_or_default()
                    .chars()
                    .take(span.start.col())
                    .collect();
                let prefix = prefix.trim_end_matches([' ', '\t']);
                dash = prefix
                    .ends_with('-')
                    .then(|| (span.start.line(), prefix.chars().count()));
            }
            TokenType::Anchor(_) | TokenType::Tag(..) | TokenType::Comment(_) => {}
            TokenType::BlockMappingStart => {
                if dash.take() == Some((notice.line, notice.column))
                    && span.start.line() == notice.line
                {
                    return Some(span.start.col() + 1);
                }
            }
            _ => dash = None,
        }
    }
    None
}

#[test]
fn scalar_loader_resolves_yaml_12_core_values() {
    assert_eq!(scalar_value("true", 0), Some(noyalib::Value::Bool(true)));
    assert_eq!(scalar_value("null", 0), Some(noyalib::Value::Null));
    assert_eq!(scalar_value("011", 0), Some(noyalib::Value::from(11_i64)));
    assert_eq!(scalar_value("on", 0), Some(noyalib::Value::from("on")));
}

#[test]
fn logical_line_oracles_match_all_yaml_line_endings() {
    for newline in ["\n", "\r\n", "\r"] {
        let comma = "before: x\na: [x,]\n".replace('\n', newline);
        assert!(closing_comma(&comma, Some("commas"), 2, 7));
        assert!(!closing_comma(&comma, Some("commas"), 2, 6));
        let literal = "a: |-\n  value \n".replace('\n', newline);
        assert!(content_whitespace(&literal, Some("trailing-spaces"), 2, 8));
        assert!(!content_whitespace(&literal, Some("trailing-spaces"), 2, 3));
        let plain = "a: value \n".replace('\n', newline);
        assert!(!content_whitespace(&plain, Some("trailing-spaces"), 1, 9));
        let terminated = "a: |+\n  value\n".replace('\n', newline);
        assert!(!content_whitespace(&terminated, Some("document-end"), 2, 1));
    }
}

#[test]
fn colon_refusal_mapping_requires_unchanged_spacing_and_location() {
    let cfg = YamlLintConfig::from_toml_str(
        "[lint.rules.colons]\nmax-spaces-before = 0\nmax-spaces-after = 1\n",
    )
    .unwrap();
    for newline in ["\n", "\r\n", "\r"] {
        let input = "ékey  :   first\n  second\n".replace('\n', newline);
        let output = "ékey  :   first second\n".replace('\n', newline);
        let lint = |text: &str| {
            ryl::lint::lint_str(
                text,
                crate::property::config::synthetic_path(),
                &cfg,
                crate::property::config::synthetic_base_dir(),
            )
        };
        let before = lint(&input);
        let problems: Vec<_> = lint(&output)
            .into_iter()
            .filter(|p| p.rule == Some("colons"))
            .collect();
        assert_eq!(problems.len(), 2);
        for problem in problems {
            assert!(unchanged_fragment(&input, &output, &problem, &before));
            assert!(!unchanged_fragment(&input, &output, &problem, &[]));
            let mut changed = problem.clone();
            changed.column += 1;
            assert!(!unchanged_fragment(&input, &output, &changed, &before));
            changed = problem.clone();
            changed.message.push_str(" changed");
            assert!(!unchanged_fragment(&input, &output, &changed, &before));
            assert!(!unchanged_fragment(
                &input,
                &output.replace("ékey", "ébay"),
                &problem,
                &before
            ));
            let changed_spacing = output.replacen("  :   ", "   :    ", 1);
            assert!(!unchanged_fragment(
                &input,
                &changed_spacing,
                &problem,
                &before
            ));
            let moved = format!("clean: x{newline}{output}");
            assert!(!unchanged_fragment(&input, &moved, &problem, &before));
        }
    }
}

#[test]
fn quote_refusal_mapping_requires_the_same_scalar_value_style_and_diagnostic() {
    let cfg = YamlLintConfig::from_toml_str(
        "[lint.rules.quoted-strings]\nquote-type = 'double'\nrequired = false\n",
    )
    .unwrap();
    let lint = |text: &str| {
        ryl::lint::lint_str(
            text,
            crate::property::config::synthetic_path(),
            &cfg,
            crate::property::config::synthetic_base_dir(),
        )
    };
    let input = "a: 'first\n  second'\n";
    let output = "a: 'first second'\n";
    let before = lint(input);
    let problem = lint(output)
        .into_iter()
        .find(|p| p.rule == Some("quoted-strings"))
        .unwrap();
    assert!(unchanged_quoted_scalar(input, output, &problem, &before));
    assert!(!unchanged_quoted_scalar(input, output, &problem, &[]));
    for changed in [
        "a: 'first changed'\n",
        "a: first second\n",
        "a: x\nb: 'first second'\n",
    ] {
        assert!(!unchanged_quoted_scalar(input, changed, &problem, &before));
    }
    let plain = "a: first second\n";
    assert!(!unchanged_quoted_scalar(plain, output, &problem, &before));
    let mut changed = problem.clone();
    changed.message.push_str(" changed");
    assert!(!unchanged_quoted_scalar(input, output, &changed, &before));
}

#[test]
fn reflow_refusal_fragments_keep_only_unchanged_local_concerns() {
    let cfg = YamlLintConfig::from_yaml_str(
        "extends: default\nrules:\n  indentation: {spaces: 2}\n",
    )
    .unwrap();
    let lint = |text: &str| {
        ryl::lint::lint_str(
            text,
            crate::property::config::synthetic_path(),
            &cfg,
            crate::property::config::synthetic_base_dir(),
        )
    };
    for (rule, input, output, changed) in [
        (
            "indentation",
            "a:\n   text: 'first\n     second'\n",
            "a:\n   text: 'first second'\n",
            "a:\n    text: 'first second'\n",
        ),
        (
            "hyphens",
            "items:\n  -   'first\n      second'\n",
            "items:\n  -   'first second'\n",
            "items:\n  -    'first second'\n",
        ),
        (
            "comments",
            "a: 'first\n  second' #bad\n",
            "a: 'first second' #bad\n",
            "a: 'first second' #changed\n",
        ),
        (
            "trailing-spaces",
            "a: 'first\n  second'  \n",
            "a: 'first second'  \n",
            "a: 'first second'   \n",
        ),
    ] {
        let before = lint(input);
        let problems: Vec<_> = lint(output)
            .into_iter()
            .filter(|p| p.rule == Some(rule))
            .collect();
        assert!(!problems.is_empty(), "{rule}: {:?}", lint(output));
        for problem in problems {
            assert!(
                unchanged_fragment(input, output, &problem, &before),
                "{problem:?}"
            );
            assert!(
                !unchanged_fragment(input, changed, &problem, &before),
                "{rule}"
            );
            assert!(!unchanged_fragment(input, output, &problem, &[]));
            let mut wrong = problem.clone();
            wrong.column += 1;
            assert!(!unchanged_fragment(input, output, &wrong, &before));
        }
    }
}

#[test]
fn reflow_refusal_boundaries_keep_document_and_stream_identity() {
    let cfg = YamlLintConfig::from_yaml_str(
        "extends: default\nrules:\n  document-end: {present: true}\n",
    )
    .unwrap();
    let lint = |text: &str| {
        ryl::lint::lint_str(
            text,
            crate::property::config::synthetic_path(),
            &cfg,
            crate::property::config::synthetic_base_dir(),
        )
    };
    for (rule, input, output, changed) in [
        (
            "new-lines",
            "a: 'first\r  second'\r",
            "a: 'first second'\r",
            "a: 'first second'\r\n",
        ),
        (
            "new-line-at-end-of-file",
            "a: 'first\n  second'",
            "a: 'first second'",
            "a: 'first second'\n",
        ),
        (
            "document-start",
            "a: 'first\n  second'\n...\nb: 'third\n  fourth'\n",
            "a: 'first second'\n...\nb: 'third fourth'\n",
            "---\na: 'first second'\n...\n---\nb: 'third fourth'\n",
        ),
        (
            "document-end",
            "a: 'first\n  second'\n---\nb: 'third\n  fourth'\n",
            "a: 'first second'\n---\nb: 'third fourth'\n",
            "a: 'first second'\n...\n---\nb: 'third fourth'\n...\n",
        ),
    ] {
        let before = lint(input);
        let problems: Vec<_> = lint(output)
            .into_iter()
            .filter(|p| p.rule == Some(rule))
            .collect();
        assert!(!problems.is_empty(), "{rule}: {:?}", lint(output));
        for problem in problems {
            assert!(
                unchanged_boundary(input, output, &problem, &before),
                "{problem:?}"
            );
            assert!(
                !unchanged_boundary(input, changed, &problem, &before),
                "{rule}"
            );
            assert!(!unchanged_boundary(input, output, &problem, &[]));
            let mut wrong = problem.clone();
            wrong.column += 1;
            assert!(!unchanged_boundary(input, output, &wrong, &before));
        }
    }
}

#[test]
fn scalar_loader_matches_reference_block_values() {
    let value: std::collections::BTreeMap<String, noyalib::Value> =
        noyalib::from_str("a: |\n  a\n  ").unwrap();
    assert_eq!(value.get("a"), Some(&noyalib::Value::from("a\n")));
    for (input, expected) in [
        ("a: |+", ""),
        ("a: |\n  a", "a\n"),
        ("a: |\n  a\n", "a\n"),
        ("a: |\n  a\n  ", "a\n"),
        ("a: |-\n  a", "a"),
        ("a: |+\n  a", "a\n"),
        ("a: |+\n  \n  ", "\n\n"),
        ("a: |+\n  \n  \n", "\n\n"),
        ("a: |+\n    ", "\n"),
        ("a: |+\n    \n", "\n"),
        ("a: >\n  a", "a\n"),
        ("a: >\n  a\n", "a\n"),
        ("a: >\n  a\n  ", "a\n"),
    ] {
        assert_eq!(
            scalar_value(input, 1),
            Some(noyalib::Value::from(expected)),
            "{input:?}"
        );
    }
}

/// Whether `commas` reports the comma before a closing bracket, whose spacing the
/// formatter takes from `brackets`/`braces` because no spacing satisfies both.
pub fn closing_comma(
    output: &str,
    rule: Option<&str>,
    line: usize,
    column: usize,
) -> bool {
    rule == Some("commas")
        && yaml_lines(output).get(line - 1).is_some_and(|text| {
            let rest: String = text.chars().skip(column.saturating_sub(2)).collect();
            rest.trim_start_matches(',')
                .trim_start_matches(' ')
                .starts_with([']', '}'])
        })
}

pub fn content_whitespace(
    output: &str,
    rule: Option<&str>,
    line: usize,
    column: usize,
) -> bool {
    if !matches!(
        rule,
        Some(
            "trailing-spaces"
                | "empty-lines"
                | "new-line-at-end-of-file"
                | "document-end"
        )
    ) {
        return false;
    }
    let tokens = scalar_tokens(output);
    let Some((index, (_, style))) =
        tokens.into_iter().enumerate().find(|(_, (span, style))| {
            let block = matches!(style, ScalarStyle::Literal | ScalarStyle::Folded);
            let first = if block {
                first_content_line(output, span)
            } else {
                span.start.line()
            };
            line >= first
                && (line < span.end.line()
                    || (line == span.end.line() && span.end.col() > 0))
                && (block || line > span.start.line())
        })
    else {
        return false;
    };
    let lines = yaml_lines(output);
    let Some(raw) = line.checked_sub(1).and_then(|index| lines.get(index)) else {
        return false;
    };
    let content = raw.trim_end_matches(['\r', '\n']);
    let mut repaired = output.to_owned();
    if rule == Some("document-end") {
        if output.ends_with(['\r', '\n'])
            || line != lines.len()
            || !matches!(style, ScalarStyle::Literal | ScalarStyle::Folded)
        {
            return false;
        }
        repaired.push_str("\n...\n");
    } else if content.is_empty() {
        let mut first = line - 1;
        let mut last = line;
        while first > 0 && lines[first - 1].trim_end_matches(['\r', '\n']).is_empty() {
            first -= 1;
        }
        while last < lines.len()
            && lines[last].trim_end_matches(['\r', '\n']).is_empty()
        {
            last += 1;
        }
        let begin: usize = lines[..first].iter().map(|line| line.len()).sum();
        let end: usize = lines[..last].iter().map(|line| line.len()).sum();
        repaired.replace_range(begin..end, "");
    } else if column == content.chars().count() + 1
        && line == lines.len()
        && !raw.ends_with(['\r', '\n'])
    {
        repaired.push('\n');
    } else {
        let Some((offset, _)) = content.char_indices().nth(column.saturating_sub(1))
        else {
            return false;
        };
        if !content[offset..].chars().all(|ch| matches!(ch, ' ' | '\t')) {
            return false;
        }
        let begin: usize = lines[..line - 1].iter().map(|line| line.len()).sum();
        repaired.replace_range(begin + offset..begin + content.len(), "");
    }
    match (scalar_value(output, index), scalar_value(&repaired, index)) {
        (Some(before), Some(after)) => before != after,
        (Some(_), None) => {
            match (
                noyalib::from_str::<noyalib::Value>(output),
                noyalib::from_str::<noyalib::Value>(&repaired),
            ) {
                (Ok(before), Ok(after)) => before != after,
                _ => false,
            }
        }
        _ => false,
    }
}

pub fn agreeing_lint(config: &str) -> String {
    let cfg = YamlLintConfig::from_toml_str(config).expect(config);
    let table = cfg.format().targets();
    let enabled = |target| if target { "enable" } else { "disable" };
    let quote = match table.quote_style {
        QuoteStyleTarget::Single => "single",
        QuoteStyleTarget::Double => "double",
        QuoteStyleTarget::Preserve => "any",
    };
    let ending = match table.line_ending {
        LineEndingTarget::Lf => "unix",
        LineEndingTarget::CrLf => "dos",
        LineEndingTarget::Native => "platform",
        LineEndingTarget::Auto => "unix",
    };
    let padding = u8::from(table.brace_spacing);
    let required = if table.keep_quotes {
        "false"
    } else {
        "'only-when-needed'"
    };
    let mut lint = format!(
        "{config}\n[lint.rules]\n\
         comments-indentation = 'enable'\ncommas = 'enable'\n\
         new-line-at-end-of-file = 'enable'\ntrailing-spaces = 'enable'\n\
         document-start = '{}'\ndocument-end = '{}'\n\
         [lint.rules.braces]\nmin-spaces-inside = {padding}\n\
         max-spaces-inside = {padding}\nmin-spaces-inside-empty = 0\n\
         max-spaces-inside-empty = 0\n\
         [lint.rules.brackets]\nmin-spaces-inside = 0\nmax-spaces-inside = 0\n\
         [lint.rules.colons]\nmax-spaces-before = 0\nmax-spaces-after = 1\n\
         [lint.rules.hyphens]\nmax-spaces-after = 1\ndash-on-own-line = {}\n\
         [lint.rules.indentation]\nindent-sequences = {}\n\
         [lint.rules.comments]\nmin-spaces-from-content = {}\n\
         max-spaces-from-content = {}\nrequire-starting-space = {}\n\
         [lint.rules.empty-lines]\nmax = {}\nmax-start = 0\nmax-end = 0\n\
         [lint.rules.quoted-strings]\nquote-type = '{quote}'\n\
         required = {required}\nallow-double-quotes-for-escaping = true\n\
         allow-quoted-quotes = true\n\
         [lint.rules.new-lines]\ntype = '{ending}'\n",
        enabled(table.document_start == MarkerTarget::Add),
        enabled(table.document_end == MarkerTarget::Add),
        table.dash_on_own_line,
        table.indent_sequences,
        table.comment_spacing,
        table.comment_spacing,
        table.comment_starting_space == MarkerTarget::Add,
        table.max_blank_lines,
    );
    if table.quote_style == QuoteStyleTarget::Preserve {
        let start = lint.find("[lint.rules.quoted-strings]").unwrap();
        let end = lint.find("[lint.rules.new-lines]").unwrap();
        lint.replace_range(start..end, "");
        lint = lint.replace("[lint.rules]", "[lint.rules]\nquoted-strings = 'disable'");
    }
    if table.line_ending == LineEndingTarget::Auto {
        lint.truncate(lint.find("[lint.rules.new-lines]").unwrap());
        lint = lint.replace("[lint.rules]", "[lint.rules]\nnew-lines = 'disable'");
    }
    lint
}

#[test]
fn agreeing_lint_accepts_retained_optional_quotes() {
    for style in ["single", "double", "preserve"] {
        let config = agreeing_lint(&format!(
            "[format]\nkeep-quotes = true\nquote-style = '{style}'\n",
        ));
        let cfg = YamlLintConfig::from_toml_str(&config).unwrap();
        let input = "plain: hello\nquoted: 'hello'\n";
        let output =
            ryl::format::format_str(input, &cfg, std::path::Path::new(""), &[]);
        assert!(ryl::format::conflicts(&cfg).is_empty(), "{config}");
        assert!(
            ryl::lint::lint_str(
                &output,
                std::path::Path::new(""),
                &cfg,
                std::path::Path::new("")
            )
            .is_empty(),
            "{config}: {output}"
        );
    }
}
