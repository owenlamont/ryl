//! LSP position encoding and `file:` URI handling.
//!
//! LSP `Position.character` is a 0-based count of code units in the negotiated
//! [`PositionEncoding`] (UTF-16 by default); ryl reports a 1-based `(line, column)` where
//! the column counts Unicode scalar values (code points), matching yamllint. Converting
//! needs the actual line text, walked CR-aware via [`line_contents`] (the rules' primitive),
//! never a fresh `\n`-only scanner.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use lsp_types::{Position, PositionEncodingKind, Range, Uri};

use crate::rules::support::line_syntax::{line_contents, split_lines_preserve_endings};

/// The position encoding negotiated at `initialize` (LSP default UTF-16 when the client
/// advertises none).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PositionEncoding {
    Utf8,
    Utf16,
    Utf32,
}

impl PositionEncoding {
    fn units(self, ch: char) -> usize {
        match self {
            Self::Utf8 => ch.len_utf8(),
            Self::Utf16 => ch.len_utf16(),
            Self::Utf32 => 1,
        }
    }

    #[must_use]
    pub fn kind(self) -> PositionEncodingKind {
        match self {
            Self::Utf8 => PositionEncodingKind::UTF8,
            Self::Utf16 => PositionEncodingKind::UTF16,
            Self::Utf32 => PositionEncodingKind::UTF32,
        }
    }
}

/// The client's most-preferred encoding (ryl supports all three), or UTF-16 when it
/// advertises none.
#[must_use]
pub fn negotiate(client: Option<&[PositionEncodingKind]>) -> PositionEncoding {
    client
        .unwrap_or(&[])
        .iter()
        .find_map(|kind| match kind.as_str() {
            "utf-8" => Some(PositionEncoding::Utf8),
            "utf-16" => Some(PositionEncoding::Utf16),
            "utf-32" => Some(PositionEncoding::Utf32),
            _ => None,
        })
        .unwrap_or(PositionEncoding::Utf16)
}

/// Code units spanned by the first `char_count` characters of `line`. `take` clamps a
/// count past the line's end to the whole line (the LSP "character past line length
/// defaults to line length" rule), also covering granit's virtual past-end positions for
/// implicit empty scalars.
fn prefix_units(line: &str, char_count: usize, enc: PositionEncoding) -> u32 {
    let units: usize = line.chars().take(char_count).map(|ch| enc.units(ch)).sum();
    u32::try_from(units).unwrap_or(u32::MAX)
}

/// The LSP [`Position`] *before* a 1-based ryl `(line, column)` (column in code points):
/// its 0-based code-unit offset under `enc`.
#[must_use]
pub fn position_at(
    lines: &[&str],
    line_1based: usize,
    column_1based: usize,
    enc: PositionEncoding,
) -> Position {
    let line_idx = line_1based.saturating_sub(1);
    let line = lines.get(line_idx).copied().unwrap_or("");
    let character = prefix_units(line, column_1based.saturating_sub(1), enc);
    Position::new(u32::try_from(line_idx).unwrap_or(u32::MAX), character)
}

/// An LSP range spanning the single character at a 1-based ryl `(line, column)`. ryl
/// reports points, so the range is one char wide (zero-width at or past end-of-line) to
/// give editors a visible squiggle.
#[must_use]
pub fn problem_range(
    lines: &[&str],
    line_1based: usize,
    column_1based: usize,
    enc: PositionEncoding,
) -> Range {
    Range {
        start: position_at(lines, line_1based, column_1based, enc),
        end: position_at(lines, line_1based, column_1based.saturating_add(1), enc),
    }
}

/// Whether `position` lies within `range` (`[start, end)`), with one carve-out: a
/// zero-width range (`start == end`, ryl's end-of-line diagnostics) matches at that point.
/// End-exclusivity keeps a cursor one column *past* a token from counting as on it.
#[must_use]
pub fn range_contains(range: Range, position: Position) -> bool {
    let after_start = position.line > range.start.line
        || (position.line == range.start.line
            && position.character >= range.start.character);
    let before_end = position.line < range.end.line
        || (position.line == range.end.line
            && position.character < range.end.character);
    after_start && (before_end || range.start == range.end && position == range.start)
}

/// The range covering the whole `text`, for a full-document replacement edit. CR-aware:
/// a text ending in a break puts the end at column 0 of the phantom final line (which
/// [`line_contents`] omits), else at the end of the last real line.
#[must_use]
pub fn full_range(text: &str, enc: PositionEncoding) -> Range {
    let lines = line_contents(text);
    let end = if text.ends_with('\n') || text.ends_with('\r') {
        Position::new(u32::try_from(lines.len()).unwrap_or(u32::MAX), 0)
    } else if let Some(last) = lines.last() {
        Position::new(
            u32::try_from(lines.len().saturating_sub(1)).unwrap_or(u32::MAX),
            prefix_units(last, last.chars().count(), enc),
        )
    } else {
        Position::new(0, 0)
    };
    Range {
        start: Position::new(0, 0),
        end,
    }
}

/// Byte offset in `text` of an LSP [`Position`] under `enc`: the inverse forward-conversion
/// used to apply incremental edits. CR-aware via [`split_lines_preserve_endings`] (the
/// forward direction's primitive), so the two cannot disagree on where a line begins. A
/// line past the end clamps to `text.len()`; a `character` past the line content clamps to
/// its end; a `character` inside a multi-unit char snaps to the char start, so a malformed
/// mid-surrogate position does not panic.
#[must_use]
pub fn offset_at(text: &str, position: Position, enc: PositionEncoding) -> usize {
    let mut offset = 0usize;
    for (line_idx, content, ending) in split_lines_preserve_endings(text) {
        if u32::try_from(line_idx).unwrap_or(u32::MAX) == position.line {
            return offset + column_byte(content, position.character, enc);
        }
        offset += content.len() + ending.len();
    }
    // `position.line` is at or past the phantom final line: clamp to the end of the text.
    text.len()
}

/// Byte offset within `content` (a break-free line) of the LSP `character` (code units
/// under `enc`). Stops before a char that would overshoot, so a past-content `character`
/// clamps to the end and a mid-char position snaps to the char start.
fn column_byte(content: &str, character: u32, enc: PositionEncoding) -> usize {
    let mut units = 0u32;
    let mut bytes = 0usize;
    for ch in content.chars() {
        let next =
            units.saturating_add(u32::try_from(enc.units(ch)).unwrap_or(u32::MAX));
        if next > character {
            break;
        }
        units = next;
        bytes += ch.len_utf8();
    }
    bytes
}

/// A `file:` URI to a filesystem path, or `None` for a non-`file` URI (e.g. an untitled
/// buffer). Handles the Windows drive-letter (`/C:/...`) and UNC-host (`//host/...`) forms.
/// Purely lexical, like ryl's `canonical_input`: no symlink or filesystem resolution.
#[must_use]
pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    // Scheme is case-insensitive (RFC 3986); after `file:`, an optional `//authority`
    // precedes the path, but RFC 8089's authority-less `file:/path` form is also emitted.
    let rest = uri
        .get(..5)
        .filter(|scheme| scheme.eq_ignore_ascii_case("file:"))
        .map(|_| &uri[5..])?;
    let (authority, path) = match rest.strip_prefix("//") {
        Some(after) => after
            .find('/')
            .map_or((after, ""), |i| (&after[..i], &after[i..])),
        None => ("", rest),
    };
    let decoded = String::from_utf8_lossy(&percent_decode(path)).into_owned();
    if !authority.is_empty() && !authority.eq_ignore_ascii_case("localhost") {
        // file://host/share/... is a UNC path; `//host/share` is one on Windows and a
        // harmless leading-slash path elsewhere.
        return Some(PathBuf::from(format!("//{authority}{decoded}")));
    }
    // file:///C:/x -> C:/x. Stripping the leading slash before a `drive:` prefix is safe
    // on every platform: ryl is never asked to lint a POSIX path whose first segment is `X:`.
    let trimmed = decoded
        .strip_prefix('/')
        .filter(|tail| is_drive_prefixed(tail))
        .map_or(decoded.as_str(), |tail| tail);
    Some(PathBuf::from(trimmed))
}

/// A `file:` URI for `path` (the inverse of [`uri_to_path`]), percent-encoding any byte
/// outside the URI-safe set so it round-trips. A backslash normalises to `/` and a drive
/// path (`C:/...`) gets the `file:///C:/...` leading slash. A non-UTF-8 path is rendered
/// lossily (it cannot round-trip, but ryl targets UTF-8 paths).
///
/// # Panics
/// Never in practice: the result is `file://` + URI-safe ASCII + `%XX` escapes, always a
/// valid `file:` URI.
#[must_use]
pub fn path_to_uri(path: &Path) -> Uri {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let slashed = path.to_string_lossy().replace('\\', "/");
    let mut encoded = String::from("file://");
    if !slashed.starts_with('/') {
        encoded.push('/');
    }
    for byte in slashed.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'.'
            | b'_'
            | b'~'
            | b'/'
            | b':' => encoded.push(byte as char),
            _ => {
                encoded.push('%');
                encoded.push(HEX[(byte >> 4) as usize] as char);
                encoded.push(HEX[(byte & 0x0f) as usize] as char);
            }
        }
    }
    Uri::from_str(&encoded).expect("a percent-encoded file path is a valid file URI")
}

fn is_drive_prefixed(s: &str) -> bool {
    let mut chars = s.chars();
    matches!((chars.next(), chars.next()), (Some(c), Some(':')) if c.is_ascii_alphabetic())
}

/// Decode `%XX` escapes to raw bytes. A `%` without two following hex digits is kept
/// literally (lenient, like browsers).
fn percent_decode(s: &str) -> Vec<u8> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hi) = bytes.get(i + 1).copied().and_then(hex_value)
            && let Some(lo) = bytes.get(i + 2).copied().and_then(hex_value)
        {
            out.push((hi << 4) | lo);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}
