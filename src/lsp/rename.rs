//! Anchor/alias rename. Rewrites every occurrence of the name *within the same YAML
//! document*: a name reused across a `---`/`...` boundary is a distinct anchor (matching
//! `rules::anchors`). Detection reuses granit's scanner tokens, so a literal `&`/`*`
//! inside a scalar is never mistaken for an anchor.

use granit_parser::{Scanner, StrInput, TokenType};
use lsp_types::{Position, PrepareRenameResponse, Range, TextEdit};

use crate::lsp::encoding::{PositionEncoding, position_at, range_contains};
use crate::rules::support::line_syntax::line_contents;
use crate::rules::support::punctuation::{build_line_starts, line_and_column};
use crate::rules::support::span_utils::CharPos;

struct Occurrence {
    name: String,
    /// 0-based char index where the name begins (one char past the `&`/`*` sigil).
    name_start: usize,
    name_len: usize,
    /// Incremented on each `---`/`...` boundary so cross-document names stay distinct.
    document: usize,
}

fn occurrences(text: &str) -> Vec<Occurrence> {
    let mut occurrences = Vec::new();
    let mut document = 0usize;
    for token in Scanner::new(StrInput::new(text)) {
        let name = match token.1 {
            TokenType::DocumentStart | TokenType::DocumentEnd => {
                document += 1;
                continue;
            }
            TokenType::Anchor(name) | TokenType::Alias(name) => name.to_string(),
            _ => continue,
        };
        // granit's Anchor/Alias span starts at the `&`/`*` sigil (verified), hence `+ 1`.
        occurrences.push(Occurrence {
            name_len: name.chars().count(),
            name_start: token.0.start.index() + 1,
            name,
            document,
        });
    }
    occurrences
}

/// The LSP range covering an occurrence's name (single line: names never span a break).
fn name_range(
    occurrence: &Occurrence,
    line_starts: &[CharPos],
    lines: &[&str],
    enc: PositionEncoding,
) -> Range {
    let (line, column) =
        line_and_column(line_starts, CharPos::new(occurrence.name_start));
    Range {
        start: position_at(lines, line, column, enc),
        end: position_at(lines, line, column + occurrence.name_len, enc),
    }
}

fn occurrence_at<'a>(
    occurrences: &'a [Occurrence],
    line_starts: &[CharPos],
    lines: &[&str],
    position: Position,
    enc: PositionEncoding,
) -> Option<&'a Occurrence> {
    occurrences
        .iter()
        .find(|occ| range_contains(name_range(occ, line_starts, lines, enc), position))
}

/// The name range + placeholder when `position` is on an anchor/alias, else `None`.
#[must_use]
pub fn prepare_rename(
    text: &str,
    position: Position,
    enc: PositionEncoding,
) -> Option<PrepareRenameResponse> {
    let char_indices: Vec<(usize, char)> = text.char_indices().collect();
    let line_starts = build_line_starts(&char_indices);
    let lines = line_contents(text);
    let occurrences = occurrences(text);
    let target = occurrence_at(&occurrences, &line_starts, &lines, position, enc)?;
    Some(PrepareRenameResponse::RangeWithPlaceholder {
        range: name_range(target, &line_starts, &lines, enc),
        placeholder: target.name.clone(),
    })
}

/// The edits renaming the anchor/alias at `position` to `new_name` across its document.
/// `Ok(None)` when `position` is not on an anchor/alias.
///
/// # Errors
/// When `new_name` is empty or contains a control character, whitespace, or a YAML flow
/// indicator (`,[]{}`), which `ns-anchor-char` forbids (a `:` is spec-legal and allowed),
/// or when it collides with another anchor in the document.
pub fn rename_edits(
    text: &str,
    position: Position,
    new_name: &str,
    enc: PositionEncoding,
) -> Result<Option<Vec<TextEdit>>, String> {
    let char_indices: Vec<(usize, char)> = text.char_indices().collect();
    let line_starts = build_line_starts(&char_indices);
    let lines = line_contents(text);
    let occurrences = occurrences(text);
    let Some(target) = occurrence_at(&occurrences, &line_starts, &lines, position, enc)
    else {
        return Ok(None);
    };
    validate_name(new_name)?;
    // Renaming onto a name already in this document would silently rebind aliases (each
    // resolves to the nearest preceding anchor of its name), changing the document's meaning.
    if new_name != target.name
        && occurrences
            .iter()
            .any(|occ| occ.document == target.document && occ.name == new_name)
    {
        return Err(format!(
            "cannot rename to {new_name:?}: an anchor or alias with that name already \
             exists in this document"
        ));
    }
    let edits = occurrences
        .iter()
        .filter(|occ| occ.name == target.name && occ.document == target.document)
        .map(|occ| {
            TextEdit::new(
                name_range(occ, &line_starts, &lines, enc),
                new_name.to_string(),
            )
        })
        .collect();
    Ok(Some(edits))
}

/// Reject a name that is not a valid `ns-anchor-char*` (YAML 1.2.2 §6.9.2: a non-space
/// printable char excluding the flow indicators `,[]{}`). `:` is left allowed: it is
/// spec-legal (granit/the reference parser read `&foo:bar` as `foo:bar`), though
/// `anchors` may then flag it as ambiguous.
fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("anchor name must not be empty".to_string());
    }
    if let Some(bad) = name.chars().find(|ch| {
        ch.is_control()
            || ch.is_whitespace()
            || matches!(ch, ',' | '[' | ']' | '{' | '}')
    }) {
        return Err(format!(
            "invalid anchor name: {bad:?} is not allowed (no control characters, \
             whitespace, or flow indicators , [ ] {{ }})"
        ));
    }
    Ok(())
}
