pub(crate) fn leading_whitespace_width(line: &str) -> usize {
    line.chars()
        .take_while(|ch| matches!(ch, ' ' | '\t'))
        .count()
}

pub(crate) fn strip_trailing_comment_preserving_quotes(content: &str) -> &str {
    match comment_start_preserving_quotes(content) {
        Some(idx) => content[..idx].trim_end(),
        None => content.trim_end(),
    }
}

pub(crate) fn comment_start_preserving_quotes(content: &str) -> Option<usize> {
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
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '#' if !in_single && !in_double => return Some(idx),
            _ => {}
        }
    }
    None
}

pub(crate) fn block_scalar_marker_index(content: &str) -> Option<usize> {
    let trimmed = content.trim_end_matches(|ch: char| ch.is_whitespace());
    let bytes = trimmed.as_bytes();

    let mut tail = bytes.len();
    let mut saw_digit = false;
    let mut saw_chomp = false;
    while tail > 0 {
        match bytes[tail - 1] {
            b'-' | b'+' if !saw_chomp => {
                saw_chomp = true;
                tail -= 1;
            }
            b'1'..=b'9' if !saw_digit => {
                saw_digit = true;
                tail -= 1;
            }
            _ => break,
        }
    }

    if tail > 0 && matches!(bytes[tail - 1], b'|' | b'>') {
        Some(tail - 1)
    } else {
        None
    }
}

#[derive(Debug, Default)]
pub(crate) struct BlockScalarTracker {
    state: Option<BlockScalarState>,
}

#[derive(Debug)]
struct BlockScalarState {
    indicator_indent: usize,
    content_indent: Option<usize>,
}

impl BlockScalarTracker {
    pub(crate) fn consume_line(&mut self, indent: usize, content: &str) -> bool {
        let Some(state) = self.state.as_mut() else {
            return false;
        };

        if content.trim().is_empty() {
            return true;
        }

        if let Some(content_indent) = state.content_indent {
            if indent >= content_indent {
                return true;
            }

            if indent <= state.indicator_indent {
                self.state = None;
                return false;
            }

            state.content_indent = Some(content_indent.min(indent));
            return true;
        }

        if indent > state.indicator_indent {
            state.content_indent = Some(indent);
            return true;
        }

        self.state = None;
        false
    }

    pub(crate) fn observe_indicator(&mut self, indent: usize, content: &str) {
        let candidate = strip_trailing_comment_preserving_quotes(content).trim_end();
        if block_scalar_marker_index(candidate).is_some() {
            self.state = Some(BlockScalarState {
                indicator_indent: indent,
                content_indent: None,
            });
        }
    }
}

pub(crate) fn is_at_value_position(
    chars: &[(usize, char)],
    idx: usize,
    flow_depth: u32,
) -> bool {
    let mut cursor = idx;
    while cursor > 0 {
        let prev = chars[cursor - 1].1;
        if matches!(prev, ' ' | '\t') {
            cursor -= 1;
            continue;
        }
        return match prev {
            ':' if flow_depth > 0 => true,
            ':' | '-' | '?' => cursor < idx,
            '[' | '{' | ',' => true,
            _ => false,
        };
    }
    true
}

pub(crate) fn split_lines_preserve_endings(
    buffer: &str,
) -> impl Iterator<Item = (usize, &str, &str)> {
    let mut start = 0usize;
    let mut line_idx = 0usize;
    std::iter::from_fn(move || {
        if start == buffer.len() {
            return None;
        }

        let bytes = buffer.as_bytes();
        let mut idx = start;
        while idx < bytes.len() && bytes[idx] != b'\n' {
            idx += 1;
        }

        let (line, ending, next_start) = if idx < bytes.len() {
            if idx > start && bytes[idx - 1] == b'\r' {
                (&buffer[start..idx - 1], &buffer[idx - 1..=idx], idx + 1)
            } else {
                (&buffer[start..idx], &buffer[idx..=idx], idx + 1)
            }
        } else {
            (&buffer[start..], "", bytes.len())
        };

        let current = (line_idx, line, ending);
        line_idx += 1;
        start = next_start;
        Some(current)
    })
}
