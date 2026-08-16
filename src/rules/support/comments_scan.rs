use granit_parser::{Placement, Scanner, Span, StrInput, TokenType};

/// `text` is granit's raw payload (after the `#`, excluding the break); `span` covers
/// the whole comment including the `#`; `placement` is `Right` for trailing comments.
pub(crate) struct CommentInfo {
    pub(crate) span: Span,
    pub(crate) text: String,
    pub(crate) placement: Placement,
}

/// Scans rather than parses so comments are still reported for documents that fail to
/// parse: the parser stops at its first error, but a parse error (an undefined alias,
/// say) is not a lexical one, so the scanner tokenizes straight past it. Consumers only
/// test for `Right`, so the scanner reporting an own-line comment as `Free` where the
/// parser resolved it to `Above` makes no difference.
pub(crate) fn collect_comments(buffer: &str) -> Vec<CommentInfo> {
    Scanner::new(StrInput::new(buffer))
        .map_while(Result::ok)
        .filter_map(|token| {
            let (span, token_type) = token.into_parts();
            match token_type {
                TokenType::Comment(comment) => Some(CommentInfo {
                    span,
                    placement: comment.placement(),
                    text: comment.into_text().into_owned(),
                }),
                _ => None,
            }
        })
        .collect()
}
