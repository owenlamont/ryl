pub(crate) fn leading_whitespace_width(line: &str) -> usize {
    line.chars()
        .take_while(|ch| matches!(ch, ' ' | '\t'))
        .count()
}

pub(crate) fn strip_trailing_comment_preserving_quotes(content: &str) -> &str {
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
            '#' if !in_single && !in_double => {
                return content[..idx].trim_end();
            }
            _ => {}
        }
    }
    content.trim_end()
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
