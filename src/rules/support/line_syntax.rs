use std::collections::HashSet;

use granit_parser::{Event, Parser, ScalarStyle, Span, SpannedEventReceiver};

pub(crate) fn leading_whitespace_width(line: &str) -> usize {
    line.chars()
        .take_while(|ch| matches!(ch, ' ' | '\t'))
        .count()
}

/// The line-ending style to reuse when inserting a line into `buffer` (e.g. a
/// `document-start`/`-end` marker): `"\r\n"` if it contains any CRLF, else `"\r"`
/// if it uses a bare `\r` (a YAML 1.2 line break, issue #284 — so a `\r`-delimited
/// file reuses `\r` instead of mixing in LF), else `"\n"`.
pub(crate) fn buffer_newline(buffer: &str) -> &'static str {
    if buffer.contains("\r\n") {
        "\r\n"
    } else if buffer.contains('\r') {
        "\r"
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
            '#' if !in_single
                && !in_double
                && content[..idx]
                    .chars()
                    .next_back()
                    .is_none_or(char::is_whitespace) =>
            {
                return Some(idx);
            }
            _ => {}
        }
    }
    None
}

pub(crate) fn block_scalar_marker_index(content: &str) -> Option<usize> {
    let marker_idx = block_scalar_header_marker_index(content)?;
    let bytes = content.as_bytes();

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

pub(crate) fn block_scalar_header_marker_index(content: &str) -> Option<usize> {
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
    Some(tail - 1)
}

/// Split `buffer` into `(0-based index, content, ending)` triples on granit's
/// YAML 1.2 line-break set (`\r\n`, `\r`, `\n`): `content` excludes the break and
/// `ending` is the matched break (`"\r\n"`, `"\r"`, or `"\n"`), or `""` for a
/// final line with no trailing break. Re-joining every `content + ending`
/// reproduces the buffer byte-for-byte, and a trailing break yields no extra
/// empty entry. A bare `\r` is a line break here, matching the parser-based
/// rules and YAML 1.2 (issue #284); on supported LF/CRLF input this is identical
/// to a `\n`-only split.
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
        while idx < bytes.len() && !matches!(bytes[idx], b'\n' | b'\r') {
            idx += 1;
        }

        let next_start = if idx >= bytes.len() {
            bytes.len()
        } else if bytes[idx] == b'\r' && bytes.get(idx + 1) == Some(&b'\n') {
            idx + 2
        } else {
            idx + 1
        };

        let current = (line_idx, &buffer[start..idx], &buffer[idx..next_start]);
        line_idx += 1;
        start = next_start;
        Some(current)
    })
}

/// Line *contents* on the same YAML 1.2 break set, indexable by a 1-based line
/// number (`lines[line - 1]`) so a granit token's line number lands on its line
/// exactly. Equivalent to mapping [`split_lines_preserve_endings`] to its
/// content.
pub(crate) fn line_contents(buffer: &str) -> Vec<&str> {
    split_lines_preserve_endings(buffer)
        .map(|(_, content, _)| content)
        .collect()
}

/// CR-aware analog of `str::split_inclusive('\n')`: yield each line *including*
/// its trailing YAML 1.2 break (`\r\n`, `\r`, or `\n`); the final piece carries
/// no break when the buffer does not end with one. Concatenating the pieces
/// reproduces the buffer, so callers can map a line index 1:1 onto a granit
/// (CR-aware) line number (issue #284).
pub(crate) fn split_lines_inclusive(buffer: &str) -> impl Iterator<Item = &str> {
    let bytes = buffer.as_bytes();
    let mut start = 0usize;
    std::iter::from_fn(move || {
        if start == buffer.len() {
            return None;
        }
        let mut idx = start;
        while idx < bytes.len() && !matches!(bytes[idx], b'\n' | b'\r') {
            idx += 1;
        }
        let end = if idx >= bytes.len() {
            bytes.len()
        } else if bytes[idx] == b'\r' && bytes.get(idx + 1) == Some(&b'\n') {
            idx + 2
        } else {
            idx + 1
        };
        let piece = &buffer[start..end];
        start = end;
        Some(piece)
    })
}
