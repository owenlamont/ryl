pub const ID: &str = "trailing-spaces";
pub const MESSAGE: &str = "trailing spaces";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub line: usize,
    pub column: usize,
}

#[must_use]
pub fn check(buffer: &str) -> Vec<Violation> {
    let mut violations = Vec::new();
    let bytes = buffer.as_bytes();
    let mut line_no = 1usize;
    let mut line_start = 0usize;
    let mut idx = 0usize;

    while idx < bytes.len() {
        if bytes[idx] == b'\n' {
            let line_end = if idx > line_start && bytes[idx - 1] == b'\r' {
                idx - 1
            } else {
                idx
            };
            process_line(buffer, line_no, line_start, line_end, &mut violations);
            idx += 1;
            line_start = idx;
            line_no += 1;
        } else {
            idx += 1;
        }
    }

    process_line(buffer, line_no, line_start, bytes.len(), &mut violations);
    violations
}

fn process_line(buffer: &str, line_no: usize, start: usize, end: usize, out: &mut Vec<Violation>) {
    if start == end {
        return;
    }

    let bytes = buffer.as_bytes();
    let mut trim_pos = end;
    while trim_pos > start {
        match bytes[trim_pos - 1] {
            b' ' | b'\t' => trim_pos -= 1,
            _ => break,
        }
    }

    if trim_pos < end {
        let column = buffer[start..trim_pos].chars().count() + 1;
        out.push(Violation {
            line: line_no,
            column,
        });
    }
}
