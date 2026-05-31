use granit_parser::{Event, Parser, Placement, Span};

/// A source comment located by [`collect_comments`]. `text` is granit's raw payload
/// (everything after the leading `#`, excluding the line break); `span` covers the
/// whole comment including the `#`; `placement` is `Right` for trailing comments.
pub(crate) struct CommentInfo {
    pub(crate) span: Span,
    pub(crate) text: String,
    pub(crate) placement: Placement,
}

/// Collect every comment in `buffer`, recovering past parse errors so comments are
/// still reported for documents that fail to parse cleanly.
pub(crate) fn collect_comments(buffer: &str) -> Vec<CommentInfo> {
    let mut parser = Parser::new_from_str(buffer);
    let mut comments = Vec::new();
    let mut last_err_at: Option<usize> = None;
    while let Some(res) = parser.next_event() {
        match res {
            Ok((Event::Comment(text, placement), span)) => {
                comments.push(CommentInfo {
                    span,
                    text: text.into_owned(),
                    placement,
                });
                last_err_at = None;
            }
            Ok(_) => last_err_at = None,
            Err(e) => {
                let pos = e.marker().index();
                if last_err_at == Some(pos) {
                    break;
                }
                last_err_at = Some(pos);
            }
        }
    }
    comments
}
