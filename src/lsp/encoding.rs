//! LSP position encoding and `file:` URI handling.
//!
//! LSP `Position.character` counts code units of the negotiated
//! [`PositionEncoding`] (UTF-16 by default), 0-based; ryl reports a 1-based
//! `(line, column)` where the column is a 1-based count of Unicode scalar values
//! (code points), matching yamllint. Converting between the two needs the actual
//! line text, so the column conversion walks the line prefix once. Line splitting
//! is CR-aware via [`line_contents`] (the same primitive the rules use), never a
//! fresh `\n`-only scanner.

use std::path::PathBuf;

use lsp_types::{Position, PositionEncodingKind, Range};

use crate::rules::support::line_syntax::line_contents;

/// The position encoding negotiated at `initialize`. ryl supports all three; the
/// LSP-mandated default when a client advertises none is UTF-16.
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

    /// The wire form sent back to the client in `ServerCapabilities`.
    #[must_use]
    pub fn kind(self) -> PositionEncodingKind {
        match self {
            Self::Utf8 => PositionEncodingKind::UTF8,
            Self::Utf16 => PositionEncodingKind::UTF16,
            Self::Utf32 => PositionEncodingKind::UTF32,
        }
    }
}

/// Pick the encoding to negotiate: the client's most-preferred kind that ryl
/// supports (it supports all three), or UTF-16 when the client advertises none.
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

/// Code units spanned by the first `char_count` characters of `line`. `take`
/// clamps a count past the line's end to the whole line (the LSP "character past
/// line length defaults to line length" rule), which also covers granit's virtual
/// past-end-of-line positions for implicit empty scalars.
fn prefix_units(line: &str, char_count: usize, enc: PositionEncoding) -> u32 {
    let units: usize = line.chars().take(char_count).map(|ch| enc.units(ch)).sum();
    u32::try_from(units).unwrap_or(u32::MAX)
}

/// Convert a 1-based ryl `(line, column)` (column in code points) to an LSP range
/// spanning the single character at that point. ryl reports points, not spans, so
/// the range is one character wide (clamped to the line; zero-width at or past
/// end-of-line) to give editors a visible squiggle.
#[must_use]
pub fn problem_range(
    lines: &[&str],
    line_1based: usize,
    column_1based: usize,
    enc: PositionEncoding,
) -> Range {
    let line_idx = line_1based.saturating_sub(1);
    let line = lines.get(line_idx).copied().unwrap_or("");
    let start = prefix_units(line, column_1based.saturating_sub(1), enc);
    let end = prefix_units(line, column_1based, enc);
    let line_u32 = u32::try_from(line_idx).unwrap_or(u32::MAX);
    Range {
        start: Position::new(line_u32, start),
        end: Position::new(line_u32, end),
    }
}

/// The range covering the whole `text`, used for a full-document replacement edit.
/// CR-aware: when the text ends in a line break the end sits at column 0 of the
/// phantom final line (which [`line_contents`] omits), else at the end of the last
/// real line.
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

/// Best-effort conversion of a `file:` URI to a filesystem path, or `None` for a
/// non-`file` URI (e.g. an untitled buffer). Percent-decodes the path and handles
/// the Windows drive-letter (`/C:/…`) and UNC-host (`//host/…`) forms. Purely
/// lexical, like ryl's `canonical_input`: no symlink or filesystem resolution.
#[must_use]
pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    // URI schemes are case-insensitive (RFC 3986). After `file:`, an optional
    // `//authority` precedes the path; RFC 8089 also allows the authority-less
    // `file:/path` form, which some clients emit.
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
        // file://host/share/… is a UNC path; `//host/share` lets PathBuf treat it
        // as one on Windows and stays a harmless leading-slash path elsewhere.
        return Some(PathBuf::from(format!("//{authority}{decoded}")));
    }
    // file:///C:/x -> C:/x. Stripping a leading slash before a `drive:` prefix is
    // safe on every platform: a real POSIX path whose first segment is `X:` is
    // not something ryl would ever be asked to lint.
    let trimmed = decoded
        .strip_prefix('/')
        .filter(|tail| is_drive_prefixed(tail))
        .map_or(decoded.as_str(), |tail| tail);
    Some(PathBuf::from(trimmed))
}

/// Whether `s` starts with a `X:` Windows drive prefix.
fn is_drive_prefixed(s: &str) -> bool {
    let mut chars = s.chars();
    matches!((chars.next(), chars.next()), (Some(c), Some(':')) if c.is_ascii_alphabetic())
}

/// Decode `%XX` escapes to raw bytes, leaving everything else untouched. A `%`
/// without two following hex digits is kept literally (lenient, like browsers).
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
