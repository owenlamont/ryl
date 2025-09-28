use crate::config::YamlLintConfig;

pub const ID: &str = "comments-indentation";
pub const MESSAGE: &str = "comment not indented like content";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Config;

impl Config {
    #[must_use]
    pub const fn resolve(_cfg: &YamlLintConfig) -> Self {
        Self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Violation {
    pub line: usize,
    pub column: usize,
}

#[must_use]
pub fn check(buffer: &str, _cfg: &Config) -> Vec<Violation> {
    let mut diagnostics: Vec<Violation> = Vec::new();
    if buffer.is_empty() {
        return diagnostics;
    }

    let lines: Vec<LineInfo> = buffer
        .lines()
        .map(|line| classify_line(line.trim_end_matches('\r')))
        .collect();

    let prev_content_indents = compute_prev_content_indents(&lines);
    let next_content_indents = compute_next_content_indents(&lines);

    let mut last_comment_indent: Option<usize> = None;

    for (idx, line) in lines.iter().enumerate() {
        match line.kind {
            LineKind::Comment => {
                let prev_indent = prev_content_indents[idx].unwrap_or(0);
                let next_indent = next_content_indents[idx].unwrap_or(0);

                let reference_indent = last_comment_indent.map_or_else(
                    || prev_indent.max(next_indent),
                    |previous_comment_indent| previous_comment_indent,
                );

                if line.indent != reference_indent && line.indent != next_indent {
                    diagnostics.push(Violation {
                        line: idx + 1,
                        column: line.indent + 1,
                    });
                }

                last_comment_indent = Some(line.indent);
            }
            LineKind::Other => {
                last_comment_indent = None;
            }
            LineKind::Empty => {}
        }
    }

    diagnostics
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LineInfo {
    indent: usize,
    kind: LineKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineKind {
    Empty,
    Comment,
    Other,
}

fn classify_line(line: &str) -> LineInfo {
    let indent = leading_whitespace_width(line);
    let trimmed = line[indent..].trim_start_matches([' ', '\t']);

    if trimmed.is_empty() {
        LineInfo {
            indent,
            kind: LineKind::Empty,
        }
    } else if trimmed.starts_with('#') {
        LineInfo {
            indent,
            kind: LineKind::Comment,
        }
    } else {
        LineInfo {
            indent,
            kind: LineKind::Other,
        }
    }
}

fn leading_whitespace_width(line: &str) -> usize {
    line.chars()
        .take_while(|ch| matches!(ch, ' ' | '\t'))
        .count()
}

fn compute_prev_content_indents(lines: &[LineInfo]) -> Vec<Option<usize>> {
    let mut result: Vec<Option<usize>> = Vec::with_capacity(lines.len());
    let mut latest: Option<usize> = None;
    for line in lines {
        if line.kind == LineKind::Other {
            latest = Some(line.indent);
        }
        result.push(latest);
    }
    result
}

fn compute_next_content_indents(lines: &[LineInfo]) -> Vec<Option<usize>> {
    let mut result: Vec<Option<usize>> = vec![None; lines.len()];
    let mut upcoming: Option<usize> = None;
    for (idx, line) in lines.iter().enumerate().rev() {
        if line.kind == LineKind::Other {
            upcoming = Some(line.indent);
        }
        result[idx] = upcoming;
    }
    result
}
