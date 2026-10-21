use crate::text_version::lexical::Kind as LexicalKind;
use ryl_yaml_parser::{Placement, Span};

/// `text` is the parser's raw payload (after the `#`, excluding the break); `span`
/// covers the whole comment including the `#`; `placement` is `Right` for trailing
/// comments.
#[derive(Clone)]
pub(crate) struct CommentInfo {
    pub(crate) span: Span,
    pub(crate) text: String,
    pub(crate) placement: Placement,
}

/// Reuses certified strict events; otherwise scans past parse errors such as undefined
/// aliases, preserving trailing-comment placement without requiring a complete parse.
pub(crate) fn collect_comments_with_analysis(
    input: crate::text_version::Analysis<'_>,
) -> Vec<CommentInfo> {
    if let Some(facts) = input.layout_facts() {
        return facts.comments.clone();
    }
    if input.strict_valid() {
        return input
            .event_iter()
            .filter_map(|(event, span)| match event {
                ryl_yaml_parser::Event::Comment(text, placement) => Some(CommentInfo {
                    span,
                    text: text.into_owned(),
                    placement,
                }),
                _ => None,
            })
            .collect();
    }
    input
        .tokens()
        .iter(input.text())
        .filter_map(|(span, kind)| match kind {
            LexicalKind::Comment { text, placement } => Some(CommentInfo {
                span,
                text: text.to_string(),
                placement,
            }),
            _ => None,
        })
        .collect()
}
