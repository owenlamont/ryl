use crate::directives::Directives;
use crate::rules::support::{
    line_syntax::scan_lines,
    span_utils::{BytePos, apply_replacements},
};
use crate::rules::{
    comments,
    line_length::{self, fold::Foldables},
};
use crate::text_version::TextVersion;

pub(super) enum Join {
    Unchanged,
    Changed,
    Refused,
}

pub(super) fn join(source: &mut TextVersion<'_>, directives: &Directives) -> Join {
    let input = source.analysis();
    if crate::fix::ambiguous(input) {
        return Join::Unchanged;
    }
    let mut scalars = Foldables::default();
    for (span, kind) in input.tokens().iter(input.text()) {
        scalars.push(span, &kind);
    }
    let mut edits = Vec::new();
    let starts = input.byte_line_starts();
    for scalar in scalars.finish() {
        let range = scalar.content;
        let first = starts.partition_point(|&at| at <= range.start) - 1;
        let last = starts
            .partition_point(|&at| at < range.end)
            .saturating_sub(1);
        // A block scalar span can consume the next token line's indentation.
        let last = last.saturating_sub(usize::from(
            scalar.style.is_folded()
                && input.text()[starts[last]..range.end]
                    .bytes()
                    .all(|byte| byte == b' '),
        ));
        if (first..=last).any(|line| directives.is_disabled(line_length::ID, line + 1))
            || (!scalar.style.is_folded()
                && comments::directive_tail(
                    input,
                    BytePos::new(range.end + scalar.style.quote_width()),
                )
                .inline())
        {
            continue;
        }
        let text = &input.text()[range.clone()];
        let lines: Vec<_> = scan_lines(text).collect();
        let indent = range.start - starts[first];
        for (index, pair) in lines.windows(2).enumerate() {
            let (left_start, left_end, _) = pair[0];
            let (right_start, right_end, _) = pair[1];
            let left = &text[left_start..left_end];
            let right = &text[right_start..right_end];
            let padding = right.len() - right.trim_start_matches([' ', '\t']).len();
            if left.trim_matches([' ', '\t']).is_empty() || padding == right.len() {
                continue;
            }
            if scalar.style.is_folded() {
                let left_padding = if index == 0 {
                    indent + left.len() - left.trim_start_matches(' ').len()
                } else {
                    left.len() - left.trim_start_matches(' ').len()
                };
                if left_padding != indent
                    || padding != indent
                    || left.trim_start_matches(' ').starts_with('\t')
                    || right.trim_start_matches(' ').starts_with('\t')
                {
                    continue;
                }
            }
            let end = if scalar.style.is_folded() {
                left.len()
            } else {
                trimmed_end(left, scalar.style.is_double())
            };
            let escaped =
                scalar.style.is_double() && backslashes(&left[..end]) % 2 == 1;
            edits.push((
                BytePos::new(range.start + left_start + end - usize::from(escaped)),
                BytePos::new(range.start + right_start + padding),
                if escaped { "" } else { " " }.to_string(),
            ));
        }
    }
    if edits.is_empty() {
        return Join::Unchanged;
    }
    let mut candidate = TextVersion::new(apply_replacements(input.text(), edits));
    let output = candidate.analysis();
    if !output.parsed().valid
        || output.parsed().strict_error().is_some()
        || !crate::fix::format_events_match_with_analysis(input, output)
    {
        return Join::Refused;
    }
    candidate.mark_strict_valid();
    *source = candidate;
    Join::Changed
}

fn trimmed_end(text: &str, double: bool) -> usize {
    let mut end = text.len();
    while end > 0 && matches!(text.as_bytes()[end - 1], b' ' | b'\t') {
        if double && backslashes(&text[..end - 1]) % 2 == 1 {
            break;
        }
        end -= 1;
    }
    end
}

fn backslashes(text: &str) -> usize {
    text.bytes().rev().take_while(|&byte| byte == b'\\').count()
}
