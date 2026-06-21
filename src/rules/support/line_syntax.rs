use std::collections::HashSet;

use granit_parser::{Event, Parser, ScalarStyle, Span, SpannedEventReceiver};

pub(crate) fn leading_whitespace_width(line: &str) -> usize {
    line.chars()
        .take_while(|ch| matches!(ch, ' ' | '\t'))
        .count()
}

/// The YAML 1.2 line break at byte `idx` as `(break length, canonical style)`, or
/// `None` when `idx` is not at a break. Single source of truth for the break set:
/// [`scan_lines`], [`first_line_break`], and `new_lines::fix` all classify through it.
pub(crate) fn line_break_at(bytes: &[u8], idx: usize) -> Option<(usize, &'static str)> {
    match bytes.get(idx)? {
        b'\r' if bytes.get(idx + 1) == Some(&b'\n') => Some((2, "\r\n")),
        b'\r' => Some((1, "\r")),
        b'\n' => Some((1, "\n")),
        _ => None,
    }
}

/// The buffer's dominant line-ending style to reuse when inserting a line: CRLF if
/// any is present, else bare `\r` if any (so a `\r`-delimited file is not mixed with
/// LF), else `\n`. Not built on [`line_break_at`]: that reports the first break
/// left-to-right ([`first_line_break`]), a different question.
pub(crate) fn buffer_newline(buffer: &str) -> &'static str {
    if buffer.contains("\r\n") {
        "\r\n"
    } else if buffer.contains('\r') {
        "\r"
    } else {
        "\n"
    }
}

/// The buffer's first YAML 1.2 line break as `(byte index, canonical style)`, or
/// `None`. Distinct from [`buffer_newline`], which reports the dominant style for
/// inserting a line; this reports the first ending verbatim, for callers reusing it.
pub(crate) fn first_line_break(buffer: &str) -> Option<(usize, &'static str)> {
    let bytes = buffer.as_bytes();
    (0..bytes.len())
        .find_map(|idx| line_break_at(bytes, idx).map(|(_, style)| (idx, style)))
}

/// 1-based line numbers of every `Scalar` event whose `style`/span satisfy `filter`.
/// A block-scalar span ends at `(end.line, col=0)`, one past the last body line; that
/// trailing line is dropped so callers don't protect content outside the scalar.
/// `None` (unparsable buffer) means bail, not fix on a partial view.
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

/// `(0-based index, content, ending)` triples per line; `ending` is the matched break
/// or `""` for an unterminated final line. Re-joining every `content + ending`
/// reproduces the buffer byte-for-byte; a trailing break yields no extra empty entry
/// (callers rely on this).
pub(crate) fn split_lines_preserve_endings(
    buffer: &str,
) -> impl Iterator<Item = (usize, &str, &str)> {
    scan_lines(buffer).enumerate().map(
        move |(line_idx, (start, content_end, next_start))| {
            (
                line_idx,
                &buffer[start..content_end],
                &buffer[content_end..next_start],
            )
        },
    )
}

/// Yields `(start, content_end, next_start)` byte offsets per line:
/// `[start..content_end]` is break-free content, `[content_end..next_start]` the
/// matched break (empty for a final unterminated line). Both public splitters below
/// are thin slicing adapters over this, so the break rule lives in one place.
fn scan_lines(buffer: &str) -> impl Iterator<Item = (usize, usize, usize)> {
    let bytes = buffer.as_bytes();
    let mut start = 0usize;
    std::iter::from_fn(move || {
        if start == bytes.len() {
            return None;
        }

        let mut idx = start;
        while idx < bytes.len() && line_break_at(bytes, idx).is_none() {
            idx += 1;
        }

        // Final line (no break): `line_break_at` is `None`, so the line ends at `idx`
        // with an empty ending; otherwise skip past the matched break.
        let next_start = idx + line_break_at(bytes, idx).map_or(0, |(len, _)| len);

        let current = (start, idx, next_start);
        start = next_start;
        Some(current)
    })
}

/// Line contents indexable by 1-based line number (`lines[line - 1]`), so a granit
/// token's line number lands on its line exactly.
pub(crate) fn line_contents(buffer: &str) -> Vec<&str> {
    split_lines_preserve_endings(buffer)
        .map(|(_, content, _)| content)
        .collect()
}

/// CR-aware analog of `str::split_inclusive('\n')`: each line including its trailing
/// YAML 1.2 break, the final piece without one if the buffer is unterminated.
/// Concatenating the pieces reproduces the buffer, so a line index maps 1:1 onto a
/// granit (CR-aware) line number.
pub(crate) fn split_lines_inclusive(buffer: &str) -> impl Iterator<Item = &str> {
    scan_lines(buffer).map(move |(start, _, next_start)| &buffer[start..next_start])
}
