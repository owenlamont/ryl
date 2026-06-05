use std::sync::LazyLock;

use ryl::config::YamlLintConfig;
use ryl::rules::{new_line_at_end_of_file, new_lines, trailing_spaces};

#[derive(Debug, Clone)]
pub struct Span {
    pub rule: &'static str,
    pub line: usize,
    pub column: usize,
}

const TRIGGER_ALL_RULES: &str = "rules:
  anchors:
    forbid-undeclared-aliases: true
    forbid-duplicated-anchors: true
    forbid-unused-anchors: true
  braces: enable
  brackets: enable
  colons: enable
  commas: enable
  comments: enable
  comments-indentation: enable
  document-end: enable
  document-start: enable
  empty-lines: enable
  empty-values: enable
  float-values:
    require-numeral-before-decimal: true
    forbid-scientific-notation: true
    forbid-nan: true
    forbid-inf: true
  hyphens: enable
  indentation:
    spaces: 2
    indent-sequences: true
  key-duplicates: enable
  key-ordering: enable
  line-length:
    max: 20
    allow-non-breakable-words: false
  new-line-at-end-of-file: enable
  new-lines:
    type: unix
  octal-values: enable
  quoted-strings:
    quote-type: single
    required: only-when-needed
  trailing-spaces: enable
  truthy: enable
";

#[must_use]
pub fn trigger_all_config() -> &'static YamlLintConfig {
    static CONFIG: LazyLock<YamlLintConfig> = LazyLock::new(|| {
        YamlLintConfig::from_yaml_str(TRIGGER_ALL_RULES)
            .expect("property-check trigger config must parse")
    });
    &CONFIG
}

// `tags` is ryl-only, so it is configured through TOML rather than the YAML
// trigger above (yamllint-compatible YAML config rejects ryl-only rules).
const TAGS_RULE_TOML: &str = "[rules.tags]
forbid-unsafe-tags = true
forbid-removed-types = true
allowed-tags = [\"!keep\"]
";

#[must_use]
pub fn tags_config() -> &'static YamlLintConfig {
    static CONFIG: LazyLock<YamlLintConfig> = LazyLock::new(|| {
        YamlLintConfig::from_toml_str(TAGS_RULE_TOML)
            .expect("property-check tags trigger config must parse")
    });
    &CONFIG
}

macro_rules! collect_standard {
    ($spans:ident, $cfg:expr, $content:expr, $module:path) => {{
        use $module as rule;
        let resolved = rule::Config::resolve($cfg);
        for violation in rule::check($content, &resolved) {
            $spans.push(Span {
                rule: rule::ID,
                line: violation.line,
                column: violation.column,
            });
        }
    }};
}

#[must_use]
pub fn collect_spans(content: &str, cfg: &YamlLintConfig) -> Vec<Span> {
    let mut spans = Vec::new();
    collect_standard!(spans, cfg, content, ryl::rules::anchors);
    collect_standard!(spans, cfg, content, ryl::rules::braces);
    collect_standard!(spans, cfg, content, ryl::rules::brackets);
    collect_standard!(spans, cfg, content, ryl::rules::colons);
    collect_standard!(spans, cfg, content, ryl::rules::commas);
    collect_standard!(spans, cfg, content, ryl::rules::comments);
    collect_standard!(spans, cfg, content, ryl::rules::comments_indentation);
    collect_standard!(spans, cfg, content, ryl::rules::document_end);
    collect_standard!(spans, cfg, content, ryl::rules::document_start);
    collect_standard!(spans, cfg, content, ryl::rules::empty_lines);
    collect_standard!(spans, cfg, content, ryl::rules::empty_values);
    collect_standard!(spans, cfg, content, ryl::rules::float_values);
    collect_standard!(spans, cfg, content, ryl::rules::hyphens);
    collect_standard!(spans, cfg, content, ryl::rules::indentation);
    collect_standard!(spans, cfg, content, ryl::rules::key_duplicates);
    collect_standard!(spans, cfg, content, ryl::rules::key_ordering);
    collect_standard!(spans, cfg, content, ryl::rules::line_length);
    collect_standard!(spans, cfg, content, ryl::rules::octal_values);
    collect_standard!(spans, cfg, content, ryl::rules::quoted_strings);
    collect_standard!(spans, tags_config(), content, ryl::rules::tags);
    collect_standard!(spans, cfg, content, ryl::rules::truthy);

    if let Some(violation) = new_line_at_end_of_file::check(content) {
        spans.push(Span {
            rule: new_line_at_end_of_file::ID,
            line: violation.line,
            column: violation.column,
        });
    }
    let new_lines_cfg = new_lines::Config::resolve(cfg);
    if let Some(violation) =
        new_lines::check(content, new_lines_cfg, new_lines::platform_newline())
    {
        spans.push(Span {
            rule: new_lines::ID,
            line: violation.line,
            column: violation.column,
        });
    }
    for violation in trailing_spaces::check(content) {
        spans.push(Span {
            rule: trailing_spaces::ID,
            line: violation.line,
            column: violation.column,
        });
    }

    spans
}

#[must_use]
pub fn line_char_lengths(content: &str) -> Vec<usize> {
    content
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line).chars().count())
        .collect()
}

pub fn check_spans_in_bounds(content: &str, spans: &[Span]) -> Result<(), String> {
    let lengths = line_char_lengths(content);
    for span in spans {
        if span.line < 1 || span.line > lengths.len() {
            return Err(format!(
                "rule `{}` reported line {} outside 1..={} for input {content:?}",
                span.rule,
                span.line,
                lengths.len()
            ));
        }
        let max_column = lengths[span.line - 1] + 1;
        if span.column < 1 || span.column > max_column {
            return Err(format!(
                "rule `{}` reported column {} outside 1..={} on line {} for input {content:?}",
                span.rule, span.column, max_column, span.line
            ));
        }
    }
    Ok(())
}
