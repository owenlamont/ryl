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

    let mut block_tracker = BlockScalarTracker::default();
    let mut lines: Vec<LineInfo> = Vec::new();

    for raw_line in buffer.lines() {
        let line = raw_line.trim_end_matches('\r');
        let indent = leading_whitespace_width(line);
        let content = &line[indent..];

        let consumed = block_tracker.consume_line(indent, content);
        let kind = if consumed {
            LineKind::Other
        } else {
            classify_line_kind(content)
        };

        lines.push(LineInfo { indent, kind });
        block_tracker.observe_indicator(indent, content);
    }

    let prev_content_indents = compute_prev_content_indents(&lines);
    let next_content_indents = compute_next_content_indents(&lines);

    let mut last_comment_indent: Option<usize> = None;

    for (idx, line) in lines.iter().enumerate() {
        match line.kind {
            LineKind::Comment => {
                let prev_indent = prev_content_indents[idx].unwrap_or(0);
                let next_indent = next_content_indents[idx].unwrap_or(0);

                let reference_indent =
                    last_comment_indent.unwrap_or_else(|| prev_indent.max(next_indent));

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

fn classify_line_kind(content: &str) -> LineKind {
    let trimmed = content.trim_start_matches([' ', '\t']);

    if trimmed.is_empty() {
        LineKind::Empty
    } else if trimmed.starts_with('#') {
        LineKind::Comment
    } else {
        LineKind::Other
    }
}

fn leading_whitespace_width(line: &str) -> usize {
    line.chars()
        .take_while(|ch| matches!(ch, ' ' | '\t'))
        .count()
}

#[derive(Debug, Default)]
struct BlockScalarTracker {
    state: Option<BlockScalarState>,
}

#[derive(Debug)]
struct BlockScalarState {
    indicator_indent: usize,
    content_indent: Option<usize>,
}

impl BlockScalarTracker {
    fn consume_line(&mut self, indent: usize, content: &str) -> bool {
        let Some(state) = self.state.as_mut() else {
            return false;
        };

        if content.trim().is_empty() {
            return true;
        }

        let updated_indent = if let Some(content_indent) = state.content_indent {
            if indent >= content_indent {
                return true;
            }
            if indent <= state.indicator_indent {
                self.state = None;
                return false;
            }
            content_indent.min(indent)
        } else {
            if indent <= state.indicator_indent {
                self.state = None;
                return false;
            }
            indent
        };
        state.content_indent = Some(updated_indent);
        true
    }

    fn observe_indicator(&mut self, indent: usize, content: &str) {
        let candidate = strip_trailing_comment_for_block(content).trim_end();
        if is_block_scalar_indicator(candidate) {
            self.state = Some(BlockScalarState {
                indicator_indent: indent,
                content_indent: None,
            });
        }
    }
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

fn strip_trailing_comment_for_block(content: &str) -> &str {
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    for (idx, ch) in content.char_indices() {
        if ch == '\\' && !in_single {
            escaped = !escaped;
            continue;
        }

        if escaped {
            escaped = false;
            continue;
        }

        match ch {
            '\'' if !in_double => {
                in_single = !in_single;
            }
            '"' if !in_single => {
                in_double = !in_double;
            }
            '#' if !in_single && !in_double => {
                return content[..idx].trim_end();
            }
            _ => {}
        }
    }
    content.trim_end()
}

fn is_block_scalar_indicator(content: &str) -> bool {
    if content.is_empty() {
        return false;
    }

    let trimmed = content.trim_end_matches(|ch: char| ch.is_whitespace());
    trimmed.ends_with("|-")
        || trimmed.ends_with("|+")
        || trimmed.ends_with('|')
        || trimmed.ends_with(">-")
        || trimmed.ends_with(">+")
        || trimmed.ends_with('>')
}
