use std::collections::HashSet;

use saphyr_parser::{Event, Parser, ScalarStyle, Span, SpannedEventReceiver};

pub(crate) fn leading_whitespace_width(line: &str) -> usize {
    line.chars()
        .take_while(|ch| matches!(ch, ' ' | '\t'))
        .count()
}

/// Returns `"\r\n"` if the buffer contains a CRLF newline, `"\n"` otherwise.
pub(crate) fn buffer_newline(buffer: &str) -> &'static str {
    if buffer.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

/// Collect line numbers (1-based) for every `Scalar` event whose `style` and
/// span satisfy `filter`. Block-scalar spans end at `(end.line, col=0)`, one
/// past the last body line; that trailing line is dropped from the range so
/// callers don't accidentally protect content unrelated to the scalar.
///
/// Returns `None` when the buffer cannot be parsed — callers should treat that
/// as "bail" rather than fix on a partial view of the document.
pub(crate) fn protected_scalar_lines<F>(
    buffer: &str,
    filter: F,
) -> Option<HashSet<usize>>
where
    F: FnMut(ScalarStyle, Span) -> bool,
{
    struct Collector<G> {
        protected: HashSet<usize>,
        filter: G,
    }
    impl<G: FnMut(ScalarStyle, Span) -> bool> SpannedEventReceiver<'_> for Collector<G> {
        fn on_event(&mut self, event: Event<'_>, span: Span) {
            if let Event::Scalar(_, style, _, _) = event
                && (self.filter)(style, span)
            {
                let start = span.start.line();
                let end = span.end.line();
                let last = if span.end.col() == 0 && end > start {
                    end - 1
                } else {
                    end
                };
                for line in start..=last {
                    self.protected.insert(line);
                }
            }
        }
    }
    let mut parser = Parser::new_from_str(buffer);
    let mut collector = Collector {
        protected: HashSet::new(),
        filter,
    };
    parser.load(&mut collector, true).ok()?;
    Some(collector.protected)
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

    if tail == 0 || !matches!(bytes[tail - 1], b'|' | b'>') {
        return None;
    }
    let marker_idx = tail - 1;

    let mut cursor = marker_idx;
    let mut consumed_whitespace = false;
    while cursor > 0 && matches!(bytes[cursor - 1], b' ' | b'\t') {
        cursor -= 1;
        consumed_whitespace = true;
    }
    if cursor == 0 {
        return Some(marker_idx);
    }
    if !consumed_whitespace {
        return None;
    }
    loop {
        let mut token_start = cursor;
        while token_start > 0 && !matches!(bytes[token_start - 1], b' ' | b'\t') {
            token_start -= 1;
        }
        if !matches!(bytes[token_start], b'!' | b'&') {
            break;
        }
        let mut next_cursor = token_start;
        while next_cursor > 0 && matches!(bytes[next_cursor - 1], b' ' | b'\t') {
            next_cursor -= 1;
        }
        if next_cursor == 0 {
            return Some(marker_idx);
        }
        cursor = next_cursor;
    }
    match bytes[cursor - 1] {
        b':' | b'-' | b'?' => Some(marker_idx),
        _ => None,
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
    let mut had_whitespace_before_quote = false;
    while cursor > 0 && matches!(chars[cursor - 1].1, ' ' | '\t') {
        cursor -= 1;
        had_whitespace_before_quote = true;
    }
    if cursor == 0 {
        return true;
    }
    loop {
        let mut token_start = cursor;
        while token_start > 0
            && !matches!(chars[token_start - 1].1, ' ' | '\t' | '[' | '{' | ',')
        {
            token_start -= 1;
        }
        if !matches!(chars[token_start].1, '!' | '&') {
            break;
        }
        let mut next_cursor = token_start;
        while next_cursor > 0 && matches!(chars[next_cursor - 1].1, ' ' | '\t') {
            next_cursor -= 1;
        }
        if next_cursor == 0 {
            return true;
        }
        if next_cursor == token_start {
            let prev = chars[next_cursor - 1].1;
            return prev == '[' || prev == '{' || prev == ',';
        }
        cursor = next_cursor;
    }
    match chars[cursor - 1].1 {
        ':' if flow_depth > 0 => true,
        ':' | '-' | '?' => had_whitespace_before_quote,
        '[' | '{' | ',' => true,
        _ => false,
    }
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
