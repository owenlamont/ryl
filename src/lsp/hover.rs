//! Hover for a position covered by a ryl diagnostic: the rule id, message, and a
//! link to the rules reference.

use lsp_types::{
    Diagnostic, Hover, HoverContents, MarkupContent, MarkupKind, NumberOrString,
    Position,
};

use crate::lsp::encoding::range_contains;

const RULES_URL: &str = "https://ryl-docs.pages.dev/rules/";

/// Build a hover for `position` from the document's `diagnostics`, or `None` when no
/// diagnostic covers it. The hover range is the first match so the editor highlights
/// one token even when several diagnostics overlap.
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

fn section(diagnostic: &Diagnostic) -> String {
    match &diagnostic.code {
        Some(NumberOrString::String(rule)) => {
            format!("**ryl: {rule}**\n\n{}", diagnostic.message)
        }
        _ => format!("**ryl**\n\n{}", diagnostic.message),
    }
}
