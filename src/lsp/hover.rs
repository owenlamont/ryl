//! Hover support for `ryl server`. Hovering a position covered by a ryl diagnostic
//! shows that rule's id, the diagnostic message, and a link to the rules reference.
//! ryl reports points, not rich descriptions, and `docs/rules.md` has no per-rule
//! anchors, so the value-add over the inline diagnostic is the click-through link; schema
//! hover stays Red Hat's `yaml-language-server`'s job. This is a pure function of the
//! already-computed diagnostics, so it carries no protocol state.

use lsp_types::{
    Diagnostic, Hover, HoverContents, MarkupContent, MarkupKind, NumberOrString,
    Position,
};

use crate::lsp::encoding::range_contains;

/// The rules reference page; there are no per-rule anchors to deep-link to.
const RULES_URL: &str = "https://ryl-docs.pages.dev/rules/";

/// Build a hover for `position` from the document's `diagnostics`, or `None` when no
/// diagnostic covers it. Multiple overlapping diagnostics are all listed; the hover
/// range is the first match so the editor can highlight the offending token.
#[must_use]
pub fn hover(diagnostics: &[Diagnostic], position: Position) -> Option<Hover> {
    let mut matching = diagnostics
        .iter()
        .filter(|d| range_contains(d.range, position));
    let first = matching.next()?;
    let mut body = section(first);
    for diagnostic in matching {
        body.push_str("\n\n");
        body.push_str(&section(diagnostic));
    }
    body.push_str("\n\n[Rule reference](");
    body.push_str(RULES_URL);
    body.push(')');
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: body,
        }),
        range: Some(first.range),
    })
}

/// One diagnostic rendered as a Markdown heading (`ryl: <rule>`, or just `ryl` for a
/// rule-less syntax diagnostic) followed by its message.
fn section(diagnostic: &Diagnostic) -> String {
    match &diagnostic.code {
        Some(NumberOrString::String(rule)) => {
            format!("**ryl: {rule}**\n\n{}", diagnostic.message)
        }
        _ => format!("**ryl**\n\n{}", diagnostic.message),
    }
}
