#[derive(Clone, Copy)]
pub(super) struct Initial<'a> {
    buffer: &'a str,
}

impl<'a> Initial<'a> {
    pub(super) const fn new(buffer: &'a str) -> Self {
        Self { buffer }
    }

    pub(super) fn following(self, end: usize) -> Option<(usize, usize)> {
        let rest = &self.buffer[end..];
        let gap = rest.len() - rest.trim_start_matches(' ').len();
        rest[gap..].starts_with(':').then_some((end + gap, gap))
    }

    pub(super) fn adjacent(self, end: usize) -> Option<(usize, char)> {
        let rest = &self.buffer[end..];
        let token = next_token(rest);
        let next = token.strip_prefix(':')?.chars().next()?;
        Some((end + rest.len() - token.len(), next))
    }
}

fn next_token(mut rest: &str) -> &str {
    loop {
        rest = rest.trim_start_matches([' ', '\t', '\r', '\n']);
        let Some(comment) = rest.strip_prefix('#') else {
            return rest;
        };
        rest = comment.find(['\r', '\n']).map_or("", |eol| &comment[eol..]);
    }
}
