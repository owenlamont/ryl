//! Diagnostic explanations and escaped annotations from prepared schemas.

use lsp_types::{
    Diagnostic, Hover, HoverContents, MarkupContent, MarkupKind, NumberOrString,
    Position,
};

use crate::config::{SourceKind, YamlLintConfig};
use crate::lsp::encoding::range_contains;
use crate::lsp::encoding::{PositionEncoding, offset_at, position_at};
use crate::rules::support::line_syntax::line_contents;
use std::fmt::Write;
use std::path::Path;

const RULES_URL: &str = "https://ryl-docs.pages.dev/rules/";

/// Build a hover for `position` from the document's `diagnostics`, or `None` when no
/// diagnostic covers it. The hover range is the first match so the editor highlights
/// one token even when several diagnostics overlap.
#[must_use]
pub fn hover(diagnostics: &[Diagnostic], position: Position) -> Option<Hover> {
    with_schema(diagnostics, position, None)
}

pub(super) fn with_schema(
    diagnostics: &[Diagnostic],
    position: Position,
    schema: Option<(String, Option<lsp_types::Range>)>,
) -> Option<Hover> {
    let mut matching = diagnostics
        .iter()
        .filter(|d| range_contains(d.range, position));
    let first = matching.next();
    let mut body = String::new();
    if let Some(first) = first {
        body = section(first);
        for diagnostic in matching {
            body.push_str("\n\n");
            body.push_str(&section(diagnostic));
        }
        body.push_str("\n\n[Rule reference](");
        body.push_str(RULES_URL);
        body.push(')');
    }
    let mut range = first.map(|first| first.range);
    if let Some((schema, schema_range)) = schema {
        if !body.is_empty() {
            body.push_str("\n\n");
        }
        body.push_str(&schema);
        range = range.or(schema_range);
    } else if first.is_none() {
        return None;
    }
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: body,
        }),
        range,
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

pub(super) fn schema(
    text: &str,
    path: &Path,
    config: &YamlLintConfig,
    kind: SourceKind,
    position: Position,
    encoding: PositionEncoding,
) -> Option<(String, Option<lsp_types::Range>)> {
    let association = config
        .schema_for(path)
        .expect("cached schema target has an association");
    let original = text;
    let bom = text.starts_with('\u{feff}');
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut position = position;
    if bom && position.line == 0 {
        position.character = position
            .character
            .checked_sub(position_at(&["\u{feff}"], 1, 2, encoding).character)?;
    }
    let mut local = position;
    let region;
    let body = if kind == SourceKind::Markdown {
        if crate::markdown_embed::markdown_has_unsupported_cr(text) {
            return None;
        }
        region = crate::markdown_embed::extract_regions(
            text,
            crate::markdown_embed::MarkdownSources {
                front_matter: config.markdown_front_matter(),
                fenced_blocks: config.markdown_fenced_blocks(),
            },
        )
        .into_iter()
        .find(|region| {
            let offset = offset_at(text, position, encoding);
            region.raw_span.contains(&offset)
        })?;
        local.line -=
            u32::try_from(region.line_offset).expect("region offset fits LSP line");
        let raw = line_contents(text)
            [usize::try_from(position.line).expect("LSP line fits usize")];
        let content_lines = line_contents(&region.content);
        let content =
            content_lines[usize::try_from(local.line).expect("LSP line fits usize")];
        let raw_len = raw.chars().count();
        let content_len = content.chars().count();
        let stripped = raw_len.saturating_sub(content_len);
        local.character = local
            .character
            .checked_sub(position_at(&[raw], 1, stripped + 1, encoding).character)?
            .saturating_add(
                u32::try_from(content_len.saturating_sub(raw_len))
                    .expect("synthetic indentation fits LSP column"),
            );
        &region.content
    } else {
        text
    };
    let offset = offset_at(body, local, encoding);
    let line = usize::try_from(local.line).expect("LSP line fits usize");
    let column = body[..offset]
        .rsplit(['\n', '\r'])
        .next()
        .expect("splitting yields at least one segment")
        .chars()
        .count();
    let (annotations, span) =
        crate::rules::schema::hover::at(association, body, (line + 1, column))?;
    let mut value = String::new();
    for annotation in annotations.into_iter().flatten() {
        if !value.is_empty() {
            value.push_str("\n\n");
        }
        let bounded: String = annotation.chars().take(4096).collect();
        for ch in crate::cli_support::sanitize_control(&bounded).chars() {
            if ch.is_ascii_punctuation() {
                write!(value, "&#{};", u32::from(ch))
                    .expect("writing to a String is infallible");
            } else {
                value.push(ch);
            }
        }
    }
    let range = (kind == SourceKind::Yaml).then(|| {
        let lines = line_contents(original);
        lsp_types::Range::new(
            position_at(
                &lines,
                span.start.line(),
                span.start.col() + 1 + usize::from(bom && span.start.line() == 1),
                encoding,
            ),
            position_at(
                &lines,
                span.end.line(),
                span.end.col() + 1 + usize::from(bom && span.end.line() == 1),
                encoding,
            ),
        )
    });
    Some((value, range))
}
