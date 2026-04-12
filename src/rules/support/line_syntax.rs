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
    if trimmed.is_empty() {
        return None;
    }

    if trimmed.ends_with("|-")
        || trimmed.ends_with("|+")
        || trimmed.ends_with(">-")
        || trimmed.ends_with(">+")
    {
        Some(trimmed.len().saturating_sub(2))
    } else if trimmed.ends_with('|') || trimmed.ends_with('>') {
        Some(trimmed.len().saturating_sub(1))
    } else {
        None
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
