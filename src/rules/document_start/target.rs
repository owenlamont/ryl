use super::{Config, FORBIDDEN_MESSAGE, MISSING_MESSAGE, Scan, Violation, input};

pub(super) fn scan(starts: impl Iterator<Item = input::Start>, cfg: Config) -> Scan {
    let mut result = Scan::default();
    for start in starts {
        if start.explicit != cfg.requires_marker() {
            let message = if start.explicit {
                FORBIDDEN_MESSAGE
            } else {
                result.implicit_starts.push(start.line);
                MISSING_MESSAGE
            };
            result.violations.push(Violation {
                line: start.line,
                column: if start.explicit { start.column + 1 } else { 1 },
                message: message.to_string(),
            });
        }
    }
    result
}

pub(super) fn first_line(lines: &input::Lines<'_>, keep_magic: bool) -> usize {
    if keep_magic && lines.first().magic() {
        2
    } else {
        1
    }
}

pub(super) fn marker_line(
    lines: &input::Lines<'_>,
    first: usize,
    content: usize,
) -> usize {
    (first..=content)
        .rev()
        .find_map(|line| {
            if lines.end(line).present() {
                Some(line + 1)
            } else {
                lines.bom(line).then_some(line)
            }
        })
        .unwrap_or(first)
}

pub(super) fn attaches_bom(
    line: &crate::rules::indentation::LineGeometry<'_>,
    relative: usize,
) -> bool {
    !line.bom() && relative == '\u{feff}'.len_utf8()
}
