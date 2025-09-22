pub const ID: &str = "new-line-at-end-of-file";
pub const MESSAGE: &str = "no new line character at the end of file";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Violation {
    pub line: usize,
    pub column: usize,
}

#[must_use]
pub fn check(buffer: &str) -> Option<Violation> {
    if buffer.is_empty() || buffer.ends_with('\n') {
        return None;
    }

    let line = buffer.lines().count();
    let tail = buffer
        .rsplit_once('\n')
        .map_or(buffer, |(_, trailing)| trailing);
    let column = tail.chars().count() + 1;

    Some(Violation { line, column })
}
