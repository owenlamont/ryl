use granit_parser::{ScalarStyle, Scanner, StrInput, TokenType};
use ryl::config::YamlLintConfig;
use ryl::config_schema::{LineEndingTarget, MarkerTarget, QuoteStyleTarget};

pub fn refused(
    problem: &ryl::lint::LintProblem,
    notices: &[ryl::lint::LintProblem],
) -> bool {
    notices
        .iter()
        .any(|notice| notice.line == problem.line && notice.rule == problem.rule)
}

fn scalar_tokens(output: &str) -> Vec<(granit_parser::Span, ScalarStyle)> {
    Scanner::new(StrInput::new(output))
        .map_while(Result::ok)
        .filter_map(|token| match token.into_parts() {
            (span, TokenType::Scalar(style, _)) => Some((span, style)),
            _ => None,
        })
        .collect()
}

fn first_content_line(output: &str, span: &granit_parser::Span) -> usize {
    let lines: Vec<_> = output.lines().collect();
    let mut first = span.start.line();
    if span.indent.is_none()
        && lines
            .get(first - 1)
            .and_then(|line| line.chars().nth(span.start.col()))
            .is_some_and(|ch| matches!(ch, '|' | '>'))
    {
        return first + 1;
    }
    while first > 1 && lines[first - 2].trim_matches([' ', '\t']).is_empty() {
        first -= 1;
    }
    first
}

fn scalar_value(output: &str, index: usize) -> Option<String> {
    let tokens = scalar_tokens(output);
    let (span, style) = tokens.get(index)?;
    if !matches!(style, ScalarStyle::Literal | ScalarStyle::Folded) {
        return serde_yaml_ng::from_str(span.slice(output)?).ok();
    }
    let first = first_content_line(output, span);
    let lines: Vec<_> = output.split_inclusive('\n').collect();
    let header = *lines.get(first.checked_sub(2)?)?;
    static HEADER: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"([|>][1-9+-]{0,2})(?:[ \t]*(?:#.*)?)$").unwrap()
    });
    let captures = HEADER.captures(header.trim_end_matches(['\r', '\n']))?;
    let indicator = &captures[1];
    let explicit = indicator.chars().find_map(|ch| ch.to_digit(10));
    let parent = explicit.map_or(0, |indent| {
        span.indent
            .unwrap_or(indent as usize)
            .saturating_sub(indent as usize)
    });
    let begin: usize = lines[..first - 1].iter().map(|line| line.len()).sum();
    let body = output.get(begin..span.end.byte_offset()?)?;
    let fragment = format!("{}a: {indicator}\n{body}", " ".repeat(parent));
    let mut value: std::collections::BTreeMap<String, String> =
        serde_yaml_ng::from_str(&fragment).ok()?;
    value.remove("a")
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
    let Some((index, (_, style))) =
        scalar_tokens(output)
            .into_iter()
            .enumerate()
            .find(|(_, (span, style))| {
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
        (Some(before), Some(after)) => {
            before != after
                || (rule != Some("document-end")
                    && matches!(style, ScalarStyle::Literal | ScalarStyle::Folded))
        }
        _ => false,
    }
}

pub fn pending_empty_block_header(
    output: &str,
    problem: &ryl::lint::LintProblem,
) -> bool {
    if !matches!(
        problem.rule,
        Some("trailing-spaces" | "new-line-at-end-of-file")
    ) {
        return false;
    }
    let tokens: Vec<_> = Scanner::new(StrInput::new(output))
        .map_while(Result::ok)
        .map(granit_parser::Token::into_parts)
        .collect();
    if !tokens.windows(2).any(|pair| {
        matches!(&pair[1].1, TokenType::Scalar(ScalarStyle::Literal | ScalarStyle::Folded, value) if value.is_empty())
            && pair[0].0.start.line() == problem.line
    }) {
        return false;
    }
    let lines: Vec<_> = output.split_inclusive('\n').collect();
    let Some(raw) = problem
        .line
        .checked_sub(1)
        .and_then(|index| lines.get(index))
    else {
        return false;
    };
    let mut repaired = output.to_owned();
    if problem.rule == Some("new-line-at-end-of-file") {
        if problem.line != lines.len() || output.ends_with('\n') {
            return false;
        }
        repaired.push('\n');
    } else {
        let content = raw.trim_end_matches(['\r', '\n']);
        let Some((offset, _)) =
            content.char_indices().nth(problem.column.saturating_sub(1))
        else {
            return false;
        };
        if !content[offset..].chars().all(|ch| matches!(ch, ' ' | '\t')) {
            return false;
        }
        let begin: usize = lines[..problem.line - 1]
            .iter()
            .map(|line| line.len())
            .sum();
        repaired.replace_range(begin + offset..begin + content.len(), "");
    }
    let load_header = |document: &str| -> Option<String> {
        let raw = document.split_inclusive('\n').nth(problem.line - 1)?;
        static HEADER: std::sync::LazyLock<regex::Regex> =
            std::sync::LazyLock::new(|| {
                regex::Regex::new(r"(?:^|[ \t])([|>][1-9+-]{0,2}[ \t]*(?:#.*)?)$")
                    .unwrap()
            });
        let captures = HEADER.captures(raw.trim_end_matches(['\r', '\n']))?;
        let ending = if raw.ends_with('\n') { "\n" } else { "" };
        let fragment = format!("a: {}{ending}", &captures[1]);
        let mut value: std::collections::BTreeMap<String, String> =
            serde_yaml_ng::from_str(&fragment).ok()?;
        value.remove("a")
    };
    matches!((load_header(output), load_header(&repaired)), (Some(before), Some(after)) if before == after)
}

pub fn pending_explicit_key_indent(
    output: &str,
    problem: &ryl::lint::LintProblem,
) -> bool {
    if problem.rule != Some("indentation")
        || !problem.message.starts_with("wrong indentation:")
    {
        return false;
    }
    let tokens: Vec<_> = Scanner::new(StrInput::new(output))
        .map_while(Result::ok)
        .map(granit_parser::Token::into_parts)
        .collect();
    let start = tokens
        .iter()
        .filter(|(span, kind)| {
            matches!(kind, TokenType::DocumentStart)
                && span.start.line() <= problem.line
        })
        .map(|(span, _)| span.start.line())
        .max()
        .unwrap_or(1);
    let end = tokens
        .iter()
        .filter(|(span, kind)| {
            matches!(kind, TokenType::DocumentStart) && span.start.line() > problem.line
        })
        .map(|(span, _)| span.start.line())
        .min()
        .unwrap_or(usize::MAX);
    tokens.iter().any(|(span, kind)| {
        matches!(kind, TokenType::Key)
            && (start..end).contains(&span.start.line())
            && output
                .lines()
                .nth(span.start.line() - 1)
                .and_then(|line| line.chars().nth(span.start.col()))
                == Some('?')
    })
}

pub fn pending_multiline_quote(
    output: &str,
    problem: &ryl::lint::LintProblem,
    cfg: &YamlLintConfig,
) -> bool {
    problem.rule == Some("quoted-strings")
        && cfg.format().targets().quote_style == QuoteStyleTarget::Single
        && Scanner::new(StrInput::new(output))
            .map_while(Result::ok)
            .map(granit_parser::Token::into_parts)
            .any(|(span, kind)| {
                matches!(kind, TokenType::Scalar(ScalarStyle::DoubleQuoted, _))
                    && span.start.line() == problem.line
                    && span.start.col() + 1 == problem.column
                    && span.start.line() < span.end.line()
                    && span.slice(output).is_some_and(|raw| {
                        let converted =
                            format!("'{}'", raw[1..raw.len() - 1].replace('\'', "''"));
                        match (
                            serde_yaml_ng::from_str::<String>(raw),
                            serde_yaml_ng::from_str::<String>(&converted),
                        ) {
                            (Ok(before), Ok(after)) => before == after,
                            _ => false,
                        }
                    })
            })
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
