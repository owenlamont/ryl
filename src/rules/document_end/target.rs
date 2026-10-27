use super::{Config, FORBIDDEN_MESSAGE, MISSING_MESSAGE, Scan, Violation, input};

pub(super) fn scan(
    boundaries: impl Iterator<Item = input::Boundary>,
    cfg: Config,
) -> Scan {
    let mut result = Scan::default();
    let mut pending = false;
    for boundary in boundaries {
        if pending {
            let line = if matches!(boundary.kind, input::Kind::StreamEnd) {
                result.unended_at_stream_end = true;
                boundary.line.saturating_sub(1).max(1)
            } else {
                result.unended_before.push(boundary.line);
                boundary.line
            };
            pending = false;
            result.violations.push(Violation {
                line,
                column: 1,
                message: MISSING_MESSAGE.to_string(),
            });
        } else if let input::Kind::End { explicit } = boundary.kind {
            pending = !explicit && cfg.requires_marker();
            if explicit && !cfg.requires_marker() {
                result.violations.push(Violation {
                    line: boundary.line,
                    column: boundary.column + 1,
                    message: FORBIDDEN_MESSAGE.to_string(),
                });
            }
        }
    }
    result
}

pub(super) fn boundary(
    text: String,
    initial: &input::Eof<'_>,
    final_newline: bool,
) -> String {
    if final_newline && initial.unterminated() {
        text.strip_prefix(initial.ending())
            .unwrap_or(&text)
            .to_string()
    } else {
        text
    }
}
