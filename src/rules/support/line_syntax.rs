use std::collections::HashSet;

use granit_parser::{Event, Parser, ScalarStyle, Span, SpannedEventReceiver};

/// Whether a line-1 comment, given as its text after the first `#`, is one another
/// tool reads verbatim: a `#!` shebang or a cloud-init `#cloud-config` header.
pub(crate) fn is_magic_first_line_comment(after_hash: &str) -> bool {
    after_hash.trim_start_matches('#').starts_with('!')
        || after_hash.starts_with("cloud-config")
}

/// Whether line 1, without its break, is such a comment starting at column 1.
pub(crate) fn is_magic_first_line(line: &str) -> bool {
    line.strip_prefix('#')
        .is_some_and(is_magic_first_line_comment)
}

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
/// trailing line is dropped so callers don't protect content outside the scalar. Its
/// span starts at the first non-blank body line, or is empty on the next token for a
/// blank-only body, so the blank lines above it are added back up to the header.
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
        blank: Vec<bool>,
    }
    impl<G> Collector<G> {
        fn blank_run_start(&self, line: usize) -> usize {
            (1..line)
                .rev()
                .take_while(|above| self.blank.get(above - 1) == Some(&true))
                .last()
                .unwrap_or(line)
        }
    }
    impl<G: FnMut(ScalarStyle, Span) -> bool> SpannedEventReceiver<'_> for Collector<G> {
        fn on_event(&mut self, event: Event<'_>, span: Span) {
            if let Event::Scalar(value, style, _, _) = event
                && (self.filter)(style, span)
            {
                // A blank-only body running to the end of input is spanned from its header.
                let start =
                    if matches!(style, ScalarStyle::Literal | ScalarStyle::Folded)
                        && (span.is_empty() || value.contains(|ch| ch != '\n'))
                    {
                        self.blank_run_start(span.start.line())
                    } else {
                        span.start.line()
                    };
                let end = span.end.line();
                let last = if span.end.col() == 0 { end - 1 } else { end };
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
        blank: split_lines_preserve_endings(buffer)
            .map(|(_, line, _)| line.trim_matches([' ', '\t']).is_empty())
            .collect(),
    };
    parser.load(&mut collector, true).ok()?;
    Some(collector.protected)
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
