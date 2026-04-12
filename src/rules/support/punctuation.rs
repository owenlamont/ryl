use std::ops::Range;

use saphyr_parser::{Event, Parser, Span, SpannedEventReceiver};

pub(crate) fn collect_scalar_ranges(buffer: &str) -> Vec<Range<usize>> {
    let mut parser = Parser::new_from_str(buffer);
    let mut collector = ScalarRangeCollector::new();
    let _ = parser.load(&mut collector, true);
    collector.into_sorted()
}

pub(crate) fn skip_comment(chars: &[(usize, char)], mut idx: usize) -> usize {
    idx += 1;
    while idx < chars.len() {
        let ch = chars[idx].1;
        if ch == '\n' {
            break;
        }
        if ch == '\r' {
            if chars.get(idx + 1).is_some_and(|(_, ch)| *ch == '\n') {
                idx += 1;
            }
            break;
        }
        idx += 1;
    }
    idx
}

pub(crate) fn build_line_starts(buffer: &str) -> Vec<usize> {
    let mut starts = Vec::new();
    starts.push(0);

    let bytes = buffer.as_bytes();
    let mut idx = 0usize;
    while idx < bytes.len() {
        match bytes[idx] {
            b'\n' => {
                starts.push(idx + 1);
                idx += 1;
            }
            b'\r' => {
                if idx + 1 < bytes.len() && bytes[idx + 1] == b'\n' {
                    starts.push(idx + 2);
                    idx += 2;
                } else {
                    starts.push(idx + 1);
                    idx += 1;
                }
            }
            _ => idx += 1,
        }
    }

    starts
}

pub(crate) fn line_and_column(
    line_starts: &[usize],
    byte_idx: usize,
) -> (usize, usize) {
    let mut left = 0usize;
    let mut right = line_starts.len();

    while left + 1 < right {
        let mid = usize::midpoint(left, right);
        if line_starts[mid] <= byte_idx {
            left = mid;
        } else {
            right = mid;
        }
    }

    let line_start = line_starts[left];
    (left + 1, byte_idx - line_start + 1)
}

struct ScalarRangeCollector {
    ranges: Vec<Range<usize>>,
}

impl ScalarRangeCollector {
    const fn new() -> Self {
        Self { ranges: Vec::new() }
    }

    fn push_range(&mut self, span: Span) {
        let start = span.start.index();
        let end = span.end.index();
        if start < end {
            self.ranges.push(start..end);
        }
    }

    fn into_sorted(mut self) -> Vec<Range<usize>> {
        self.ranges.sort_by(|a, b| a.start.cmp(&b.start));
        self.ranges
    }
}

impl SpannedEventReceiver<'_> for ScalarRangeCollector {
    fn on_event(&mut self, ev: Event<'_>, span: Span) {
        if matches!(ev, Event::Scalar(..)) {
            self.push_range(span);
        }
    }
}
