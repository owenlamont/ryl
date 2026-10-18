use crate::text_version::lexical::Kind as LexicalKind;
use granit_parser::{Placement, Span};

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
pub(crate) fn collect_comments_with_analysis(
    input: crate::text_version::Analysis<'_>,
) -> Vec<CommentInfo> {
    input
        .tokens()
        .iter()
        .filter_map(|(span, kind)| match kind {
            LexicalKind::Comment { text, placement } => Some(CommentInfo {
                span: span.into_span(),
                text: text.clone(),
                placement: *placement,
            }),
            _ => None,
        })
        .collect()
}
