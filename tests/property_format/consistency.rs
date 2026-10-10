pub fn refused(
    problem: &ryl::lint::LintProblem,
    notices: &[ryl::lint::LintProblem],
) -> bool {
    notices
        .iter()
        .any(|notice| notice.line == problem.line && notice.rule == problem.rule)
}

use granit_parser::{ScalarStyle, Scanner, StrInput, TokenType};
use ryl::config::YamlLintConfig;
use ryl::config_schema::{LineEndingTarget, MarkerTarget, QuoteStyleTarget};

#[derive(Debug, PartialEq, Eq)]
pub enum ContentWhitespace {
    ValueBearing,
    ValueSafePending,
}

pub fn content_whitespace(
    output: &str,
    rule: Option<&str>,
    line: usize,
    column: usize,
) -> Option<ContentWhitespace> {
    if !matches!(
        rule,
        Some("trailing-spaces" | "empty-lines" | "new-line-at-end-of-file")
    ) {
        return None;
    }
    let raw_lines: Vec<_> = output.lines().collect();
    let content_line = Scanner::new(StrInput::new(output))
        .map_while(Result::ok)
        .map(granit_parser::Token::into_parts)
        .any(|(span, kind)| {
            if !matches!(
                kind,
                TokenType::Scalar(ScalarStyle::Literal | ScalarStyle::Folded, _)
            ) {
                return false;
            }
            let mut first = span.start.line();
            if span.indent.is_none() {
                first += 1;
            } else {
                while first > 1
                    && raw_lines[first - 2].trim_matches([' ', '\t']).is_empty()
                {
                    first -= 1;
                }
            }
            line >= first
                && (line < span.end.line()
                    || (line == span.end.line() && span.end.col() > 0))
        });
    if !content_line {
        return None;
    }
    let lines: Vec<_> = output.split_inclusive('\n').collect();
    let raw = line.checked_sub(1).and_then(|index| lines.get(index))?;
    let content = raw.trim_end_matches(['\n', '\r']);
    let start: usize = lines[..line - 1].iter().map(|line| line.len()).sum();
    let mut repaired = output.to_owned();
    if content.is_empty() {
        let mut first = line - 1;
        let mut last = line;
        while first > 0 && lines[first - 1].trim_end_matches(['\n', '\r']).is_empty() {
            first -= 1;
        }
        while last < lines.len()
            && lines[last].trim_end_matches(['\n', '\r']).is_empty()
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
        let (offset, _) = content.char_indices().nth(column.saturating_sub(1))?;
        if !content[offset..].chars().all(|ch| matches!(ch, ' ' | '\t')) {
            return None;
        }
        repaired.replace_range(start + offset..start + content.len(), "");
    }
    match (loaded_strings(output), loaded_strings(&repaired)) {
        (Ok(before), Ok(after)) => Some(if before == after {
            ContentWhitespace::ValueSafePending
        } else {
            ContentWhitespace::ValueBearing
        }),
        _ => None,
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

#[derive(Default)]
struct LoadedStrings(Vec<String>);

impl<'de> serde::Deserialize<'de> for LoadedStrings {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Self, D::Error> {
        struct Strings;
        impl<'de> serde::de::Visitor<'de> for Strings {
            type Value = LoadedStrings;
            fn expecting(
                &self,
                formatter: &mut std::fmt::Formatter<'_>,
            ) -> std::fmt::Result {
                formatter.write_str("a YAML value")
            }
            fn visit_str<E: serde::de::Error>(
                self,
                value: &str,
            ) -> Result<Self::Value, E> {
                Ok(LoadedStrings(vec![value.to_owned()]))
            }
            fn visit_bool<E: serde::de::Error>(
                self,
                _: bool,
            ) -> Result<Self::Value, E> {
                Ok(LoadedStrings::default())
            }
            fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<Self::Value, E> {
                Ok(LoadedStrings::default())
            }
            fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<Self::Value, E> {
                Ok(LoadedStrings::default())
            }
            fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<Self::Value, E> {
                Ok(LoadedStrings::default())
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(LoadedStrings::default())
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> Result<Self::Value, A::Error> {
                let mut strings = LoadedStrings::default();
                while let Some(value) = sequence.next_element::<LoadedStrings>()? {
                    strings.0.extend(value.0);
                }
                Ok(strings)
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut mapping: A,
            ) -> Result<Self::Value, A::Error> {
                let mut strings = LoadedStrings::default();
                while let Some((key, value)) =
                    mapping.next_entry::<LoadedStrings, LoadedStrings>()?
                {
                    strings.0.extend(key.0);
                    strings.0.extend(value.0);
                }
                Ok(strings)
            }
            fn visit_enum<A: serde::de::EnumAccess<'de>>(
                self,
                tagged: A,
            ) -> Result<Self::Value, A::Error> {
                let (_, value) = tagged.variant::<String>()?;
                serde::de::VariantAccess::newtype_variant(value)
            }
        }
        deserializer.deserialize_any(Strings)
    }
}

fn loaded_strings(input: &str) -> Result<Vec<String>, serde_yaml_ng::Error> {
    let mut strings = Vec::new();
    let input = input.replace("\n\u{feff}---", "\n---");
    for document in serde_yaml_ng::Deserializer::from_str(&input) {
        let value: LoadedStrings = serde::Deserialize::deserialize(document)?;
        strings.extend(value.0);
    }
    Ok(strings)
}
