use ryl_yaml_parser::ScalarStyle;

#[derive(Clone, Copy)]
pub(super) struct Initial<'a> {
    pub(super) style: ScalarStyle,
    content: &'a str,
}

impl<'a> Initial<'a> {
    pub(super) const fn new(style: ScalarStyle, content: &'a str) -> Self {
        Self { style, content }
    }

    pub(super) fn any_escape(self) -> bool {
        self.content.contains('\\')
    }

    pub(super) fn non_quote_escape(self) -> bool {
        self.content
            .match_indices('\\')
            .any(|(at, _)| !self.content[at + 1..].starts_with('"'))
    }

    pub(super) fn multiline(self) -> bool {
        self.content.contains('\n') || self.content.contains('\r')
    }

    pub(super) fn escaped_line_ending(self) -> bool {
        self.multiline()
            && (self.content.contains("\\\n") || self.content.contains("\\\r\n"))
    }
}
