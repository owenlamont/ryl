//! `[format] property-order`: a node's anchor and tag written in one order.

use crate::config_schema::PropertyOrderTarget;
use crate::rules::support::span_utils::{BytePos, marker_byte_offset};
use crate::text_version::Analysis;
use crate::text_version::edits::Edit;
use crate::text_version::lexical::Kind as LexicalKind;

pub(crate) const ID: &str = "property-order";

/// An edit swapping each anchor and tag that sit together on one line in the order
/// `order` rejects, keeping the gap between them.
pub(crate) fn edits(input: Analysis<'_>, order: PropertyOrderTarget) -> Vec<Edit> {
    let text = input.text();
    let mut edits = Vec::new();
    let mut previous: Option<(usize, usize, LexicalKind)> = None;
    for (span, kind) in input.tokens().iter(text) {
        let (start, end) = (
            marker_byte_offset(span.start).get(),
            marker_byte_offset(span.end).get(),
        );
        if matches!(kind, LexicalKind::Anchor | LexicalKind::Tag) {
            if let Some((first, first_end, first_kind)) = previous.take()
                && first_kind != kind
                && !text[first_end..start].contains(['\n', '\r'])
                && rejects(order, first_kind)
            {
                let swapped = format!(
                    "{}{}{}",
                    &text[start..end],
                    &text[first_end..start],
                    &text[first..first_end]
                );
                edits.push((BytePos::new(first), BytePos::new(end), swapped));
            } else {
                previous = Some((start, end, kind));
            }
        } else {
            previous = None;
        }
    }
    edits
}

/// Whether `order` wants the property after one of kind `first`.
const fn rejects(order: PropertyOrderTarget, first: LexicalKind) -> bool {
    matches!(
        (order, first),
        (PropertyOrderTarget::AnchorFirst, LexicalKind::Tag)
            | (PropertyOrderTarget::TagFirst, LexicalKind::Anchor)
    )
}
