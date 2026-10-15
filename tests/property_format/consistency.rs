use super::representation::{
    BLOCK_HEADER, first_content_line, scalar_tokens, scalar_value,
};
use granit_parser::{ScalarStyle, Scanner, StrInput, TokenType};
use ryl::config::YamlLintConfig;
use ryl::config_schema::{LineEndingTarget, MarkerTarget, QuoteStyleTarget};
use similar::{DiffTag, TextDiff};

pub fn refused(
    input: &str,
    output: &str,
    problem: &ryl::lint::LintProblem,
    notices: &[ryl::lint::LintProblem],
    before: &[ryl::lint::LintProblem],
) -> bool {
    let input_lines: Vec<_> = input.lines().collect();
    let output_lines: Vec<_> = output.lines().collect();
    let unchanged_input_line = TextDiff::from_slices(&input_lines, &output_lines)
        .ops()
        .iter()
        .find(|op| {
            op.tag() == DiffTag::Equal && op.new_range().contains(&(problem.line - 1))
        })
        .map(|op| op.old_range().start + problem.line - op.new_range().start);
    notices.iter().any(|notice| {
        if notice.rule != problem.rule {
            return false;
        }
        if before.iter().any(|old| {
            old.rule == problem.rule
                && old.message == problem.message
                && old.column == problem.column
                && Some(old.line) == unchanged_input_line
        }) {
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

fn refusal_column(output: &str, notice: &ryl::lint::LintProblem) -> Option<usize> {
    if notice.rule != Some("hyphens")
        || notice.message
            != "cannot move this mapping below its `-`: a comment follows the `-`"
    {
        return Some(notice.column);
    }
    let mut dash = None;
    for token in Scanner::new(StrInput::new(output)).map_while(Result::ok) {
        let (span, kind) = token.into_parts();
        match kind {
            TokenType::BlockEntry => {
                let prefix: String = output
                    .lines()
                    .nth(span.start.line() - 1)
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
    if matches!(rule, Some("new-line-at-end-of-file" | "document-end"))
        && !output.ends_with(['\r', '\n'])
        && tokens.last().is_some_and(|(span, style)| {
            matches!(style, ScalarStyle::Literal | ScalarStyle::Folded)
                && span.end.byte_offset() == Some(output.len())
                && first_content_line(output, span) <= line
                && output.lines().count() == line
                && output
                    .lines()
                    .nth(first_content_line(output, span) - 2)
                    .and_then(|header| BLOCK_HEADER.captures(header))
                    .is_some_and(|captures| !captures[1].contains('-'))
        })
        && match rule {
            Some("new-line-at-end-of-file") => output
                .lines()
                .last()
                .is_some_and(|last| column == last.chars().count() + 1),
            Some("document-end") => column == 1,
            _ => false,
        }
    {
        // Loaders disagree on the value of a break appended at block-scalar EOF.
        return true;
    }
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
    let lines: Vec<_> = output.split_inclusive('\n').collect();
    let Some(raw) = line.checked_sub(1).and_then(|index| lines.get(index)) else {
        return false;
    };
    let content = raw.trim_end_matches(['\r', '\n']);
    let mut repaired = output.to_owned();
    if rule == Some("document-end") {
        if output.ends_with('\n')
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
        && !raw.ends_with('\n')
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
         required = 'only-when-needed'\nallow-double-quotes-for-escaping = true\n\
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
