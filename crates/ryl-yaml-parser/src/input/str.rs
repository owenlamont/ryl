use crate::{
    char_traits::is_breakz,
    error::ErrorKind,
    input::{BorrowedInput, Input, SkipTabs, WhitespaceResult},
};
use alloc::string::String;

/// A parser input backed by a `&str`.
#[allow(clippy::module_name_repetitions)]
pub struct StrInput<'a> {
    /// The full, original input string.
    ///
    /// This is kept to support O(1) byte-offset capture and zero-copy slicing via the optional
    /// [`Input::byte_offset`] / [`Input::slice_bytes`] APIs.
    original: &'a str,
    /// The remaining input slice.
    ///
    /// This is a moving window into [`Self::original`]. All consuming operations advance this
    /// slice.
    buffer: &'a str,
    /// The number of characters we have looked ahead.
    ///
    /// This tracks how many characters the parser asked us to look ahead for so we can return the
    /// correct value in [`Self::buflen`].
    lookahead: usize,
}

impl<'a> StrInput<'a> {
    /// Create a new [`StrInput`] over the given string slice.
    #[must_use]
    pub fn new(input: &'a str) -> Self {
        Self {
            original: input,
            buffer: input,
            lookahead: 0,
        }
    }

    /// Return the number of bytes consumed from the original input.
    ///
    /// This is an O(1) operation derived from the invariant that [`Self::buffer`] is always a
    /// suffix of [`Self::original`].
    #[inline]
    #[must_use]
    fn consumed_bytes(&self) -> usize {
        self.original.len() - self.buffer.len()
    }

    /// Find the end of the next plain-scalar chunk in the current input buffer.
    ///
    /// Returns its byte length and character count. This keeps the existing batched scanner in one
    /// place so callers can either materialize the chunk or retain it as a borrowed source slice.
    fn plain_scalar_chunk_len(&self, flow_level_gt_0: bool) -> (usize, usize) {
        let bytes = self.buffer.as_bytes();
        let mut byte_pos = 0;
        let mut chars_consumed = 0;

        while byte_pos < bytes.len() {
            let byte = bytes[byte_pos];
            if byte < 0x80 {
                let character = byte as char;
                if crate::char_traits::is_blank_or_breakz(character)
                    || flow_level_gt_0 && crate::char_traits::is_flow(character)
                {
                    break;
                }
                if character == ':' {
                    let next_byte = bytes.get(byte_pos + 1).copied().unwrap_or(0);
                    // A non-ASCII character cannot be blank, breakz, or a flow indicator.
                    let stops_scalar = next_byte < 0x80
                        && (crate::char_traits::is_blank_or_breakz(next_byte as char)
                            || flow_level_gt_0 && crate::char_traits::is_flow(next_byte as char));
                    if stops_scalar {
                        break;
                    }
                }
                byte_pos += 1;
            } else {
                let character = self.buffer[byte_pos..].chars().next().unwrap();
                byte_pos += character.len_utf8();
            }
            chars_consumed += 1;
        }

        (byte_pos, chars_consumed)
    }
}

impl Input for StrInput<'_> {
    #[inline]
    fn lookahead(&mut self, x: usize) {
        // We already have all characters that we need.
        // We cannot add '\0's to the buffer when we reach EOF.
        // Character-retrieving functions return '\0' when they read past EOF.
        self.lookahead = self.lookahead.max(x);
    }

    #[inline]
    fn buflen(&self) -> usize {
        self.lookahead
    }

    #[inline]
    fn bufmaxlen(&self) -> usize {
        BUFFER_LEN
    }

    #[inline]
    fn raw_read_ch(&mut self) -> char {
        let mut chars = self.buffer.chars();
        if let Some(c) = chars.next() {
            self.buffer = chars.as_str();
            c
        } else {
            '\0'
        }
    }

    #[inline]
    fn raw_read_non_breakz_ch(&mut self) -> Option<char> {
        if let Some((c, sub_str)) = split_first_char(self.buffer) {
            if is_breakz(c) {
                None
            } else {
                self.buffer = sub_str;
                Some(c)
            }
        } else {
            None
        }
    }

    #[inline]
    fn skip(&mut self) {
        if !self.buffer.is_empty() {
            let b = self.buffer.as_bytes()[0];
            if b < 0x80 {
                self.buffer = &self.buffer[1..];
            } else {
                let mut chars = self.buffer.chars();
                chars.next();
                self.buffer = chars.as_str();
            }
        }
    }

    #[inline]
    fn skip_n(&mut self, count: usize) {
        let mut chars = self.buffer.chars();
        for _ in 0..count {
            if chars.next().is_none() {
                break;
            }
        }
        self.buffer = chars.as_str();
    }

    #[inline]
    fn peek(&self) -> char {
        if self.buffer.is_empty() {
            return '\0';
        }
        let b = self.buffer.as_bytes()[0];
        if b < 0x80 {
            b as char
        } else {
            self.buffer.chars().next().unwrap()
        }
    }

    #[inline]
    fn peek_nth(&self, n: usize) -> char {
        if n == 0 {
            return self.peek();
        }
        let bytes = self.buffer.as_bytes();
        if n == 1 && bytes.len() >= 2 && bytes[0] < 0x80 && bytes[1] < 0x80 {
            return bytes[1] as char;
        }
        let mut chars = self.buffer.chars();
        for _ in 0..n {
            if chars.next().is_none() {
                return '\0';
            }
        }
        chars.next().unwrap_or('\0')
    }

    #[inline]
    fn byte_offset(&self) -> Option<usize> {
        Some(self.consumed_bytes())
    }

    #[inline]
    fn slice_bytes(&self, start: usize, end: usize) -> Option<&str> {
        debug_assert!(start <= end);
        debug_assert!(end <= self.original.len());
        self.original.get(start..end)
    }

    #[cfg(feature = "comments")]
    #[inline]
    fn may_contain_comments(&self) -> bool {
        self.original.as_bytes().contains(&b'#')
    }

    #[inline]
    fn next_2_are(&self, c1: char, c2: char) -> bool {
        let mut chars = self.buffer.chars();
        chars.next() == Some(c1) && chars.next() == Some(c2)
    }

    #[inline]
    fn next_3_are(&self, c1: char, c2: char, c3: char) -> bool {
        let mut chars = self.buffer.chars();
        chars.next() == Some(c1) && chars.next() == Some(c2) && chars.next() == Some(c3)
    }

    #[inline]
    fn next_is_document_indicator(&self) -> bool {
        if self.buffer.len() < 3 {
            false
        } else {
            // Since all characters we look for are ASCII, we can directly use the byte API of str.
            let bytes = self.buffer.as_bytes();
            (bytes.len() == 3 || matches!(bytes[3], b' ' | b'\t' | 0 | b'\n' | b'\r'))
                && (bytes[0] == b'.' || bytes[0] == b'-')
                && bytes[0] == bytes[1]
                && bytes[1] == bytes[2]
        }
    }

    #[inline]
    fn next_is_document_start(&self) -> bool {
        if self.buffer.len() < 3 {
            false
        } else {
            // Since all characters we look for are ASCII, we can directly use the byte API of str.
            let bytes = self.buffer.as_bytes();
            (bytes.len() == 3 || matches!(bytes[3], b' ' | b'\t' | 0 | b'\n' | b'\r'))
                && bytes[0] == b'-'
                && bytes[1] == b'-'
                && bytes[2] == b'-'
        }
    }

    #[inline]
    fn next_is_document_end(&self) -> bool {
        if self.buffer.len() < 3 {
            false
        } else {
            // Since all characters we look for are ASCII, we can directly use the byte API of str.
            let bytes = self.buffer.as_bytes();
            (bytes.len() == 3 || matches!(bytes[3], b' ' | b'\t' | 0 | b'\n' | b'\r'))
                && bytes[0] == b'.'
                && bytes[1] == b'.'
                && bytes[2] == b'.'
        }
    }

    fn skip_ws_to_eol(
        &mut self,
        skip_tabs: SkipTabs,
    ) -> (usize, Result<WhitespaceResult, ErrorKind>) {
        let mut new_str = self.buffer;
        let mut has_yaml_ws = false;
        let mut encountered_tab = false;

        // Separate loops keep the fast space-only path while still tracking whether tabs were seen.
        if skip_tabs == SkipTabs::Yes {
            loop {
                if let Some(sub_str) = new_str.strip_prefix(' ') {
                    has_yaml_ws = true;
                    new_str = sub_str;
                } else if let Some(sub_str) = new_str.strip_prefix('\t') {
                    encountered_tab = true;
                    new_str = sub_str;
                } else {
                    break;
                }
            }
        } else {
            while let Some(sub_str) = new_str.strip_prefix(' ') {
                has_yaml_ws = true;
                new_str = sub_str;
            }
        }

        // All characters consumed were ASCII. We can use the byte length difference to count the
        // number of whitespace ignored.
        let mut chars_consumed = self.buffer.len() - new_str.len();

        if !new_str.is_empty() && new_str.as_bytes()[0] == b'#' {
            if !encountered_tab && !has_yaml_ws {
                return (chars_consumed, Err(ErrorKind::CommentNotSeparated));
            }

            // Skip remaining characters until we hit a breakz.
            while let Some((c, sub_str)) = split_first_char(new_str) {
                if is_breakz(c) {
                    break;
                }
                new_str = sub_str;
                chars_consumed += 1;
            }
        }

        self.buffer = new_str;

        (
            chars_consumed,
            Ok(WhitespaceResult::new(encountered_tab, has_yaml_ws)),
        )
    }

    fn skip_ws_to_eol_blanks(&mut self, skip_tabs: SkipTabs) -> (usize, WhitespaceResult) {
        let bytes = self.buffer.as_bytes();
        let mut i = 0;
        let mut encountered_tab = false;
        let mut has_yaml_ws = false;

        if skip_tabs == SkipTabs::Yes {
            while i < bytes.len() {
                match bytes[i] {
                    b' ' => {
                        has_yaml_ws = true;
                        i += 1;
                    }
                    b'\t' => {
                        encountered_tab = true;
                        i += 1;
                    }
                    _ => break,
                }
            }
        } else {
            while i < bytes.len() && bytes[i] == b' ' {
                has_yaml_ws = true;
                i += 1;
            }
        }

        self.buffer = &self.buffer[i..];

        (i, WhitespaceResult::new(encountered_tab, has_yaml_ws))
    }

    #[inline]
    fn next_is_blank_or_break(&self) -> bool {
        !self.buffer.is_empty() && matches!(self.buffer.as_bytes()[0], b' ' | b'\t' | b'\n' | b'\r')
    }

    #[inline]
    fn next_is_blank_or_breakz(&self) -> bool {
        self.buffer.is_empty()
            || matches!(self.buffer.as_bytes()[0], b' ' | b'\t' | 0 | b'\n' | b'\r')
    }

    #[inline]
    fn next_is_blank(&self) -> bool {
        !self.buffer.is_empty() && matches!(self.buffer.as_bytes()[0], b' ' | b'\t')
    }

    #[inline]
    fn next_is_break(&self) -> bool {
        !self.buffer.is_empty() && matches!(self.buffer.as_bytes()[0], b'\n' | b'\r')
    }

    #[inline]
    fn next_is_breakz(&self) -> bool {
        self.buffer.is_empty() || matches!(self.buffer.as_bytes()[0], 0 | b'\n' | b'\r')
    }

    #[inline]
    fn next_is_z(&self) -> bool {
        self.buffer.is_empty()
    }

    #[inline]
    fn next_is_flow(&self) -> bool {
        !self.buffer.is_empty()
            && matches!(self.buffer.as_bytes()[0], b',' | b'[' | b']' | b'{' | b'}')
    }

    #[inline]
    fn next_is_digit(&self) -> bool {
        !self.buffer.is_empty() && self.buffer.as_bytes()[0].is_ascii_digit()
    }

    /// Check if the next character is an ASCII alphanumeric, `_`, or `-`.
    ///
    /// This is used as a heuristic for error detection (e.g., when `:` is followed
    /// by tab and then a potential value character). The ASCII-only check is intentional:
    /// it catches common cases like `key:\tvalue` while avoiding false positives for
    /// valid YAML constructs. Unicode value starters (e.g., `äöü`) are not detected,
    /// but such cases will still fail to parse (with a less specific error message).
    #[inline]
    fn next_is_alpha(&self) -> bool {
        !self.buffer.is_empty()
            && matches!(self.buffer.as_bytes()[0], b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'-')
    }

    fn skip_while_non_breakz(&mut self) -> usize {
        let mut byte_pos = 0;
        let mut chars_consumed = 0;

        for (i, c) in self.buffer.char_indices() {
            if is_breakz(c) || !crate::char_traits::is_printable(c) {
                break;
            }
            byte_pos = i + c.len_utf8();
            chars_consumed += 1;
        }

        self.buffer = &self.buffer[byte_pos..];
        chars_consumed
    }

    #[inline]
    fn skip_while_blank(&mut self) -> usize {
        let bytes = self.buffer.as_bytes();

        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b' ' | b'\t' => i += 1,
                _ => break,
            }
        }

        self.buffer = &self.buffer[i..];
        i
    }

    /// Fetch characters matching `is_alpha` (ASCII alphanumeric, `_`, `-`).
    ///
    /// This is used for scanning tag handles (e.g., `!foo!`). Per YAML 1.2 spec,
    /// tag handles use `ns-word-char` which is `[0-9a-zA-Z-]`. Our implementation
    /// is slightly more permissive by also accepting `_`, but this is harmless
    /// and matches common practice. Unicode characters like `ä` or `π` are NOT
    /// valid in tag handles per spec, so the ASCII-only byte-based scanning here
    /// is both correct and efficient.
    fn fetch_while_is_alpha(&mut self, out: &mut String) -> usize {
        let bytes = self.buffer.as_bytes();
        let mut i = 0;

        // All target characters are ASCII, so we can scan bytes directly.
        while i < bytes.len() {
            match bytes[i] {
                b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'-' => i += 1,
                _ => break,
            }
        }

        // All matched characters are ASCII, so we can safely slice and convert.
        out.push_str(&self.buffer[..i]);
        self.buffer = &self.buffer[i..];

        i
    }

    fn fetch_while_is_yaml_non_space(&mut self, out: &mut String) -> usize {
        let mut byte_pos = 0;
        let mut chars_consumed = 0;

        for (i, c) in self.buffer.char_indices() {
            if !crate::char_traits::is_yaml_non_space(c) || crate::char_traits::is_z(c) {
                break;
            }

            byte_pos = i + c.len_utf8();
            chars_consumed += 1;
        }

        out.push_str(&self.buffer[..byte_pos]);
        self.buffer = &self.buffer[byte_pos..];

        chars_consumed
    }

    #[inline]
    fn fetch_block_scalar_line(&mut self, out: &mut String) -> usize {
        let end = self
            .buffer
            .as_bytes()
            .iter()
            .position(|&byte| matches!(byte, b'\r' | b'\n' | 0))
            .unwrap_or(self.buffer.len());

        // All terminators are ASCII, so the line ends on a UTF-8 boundary.
        let (line, remaining) = self.buffer.split_at(end);
        out.push_str(line);
        self.buffer = remaining;
        line.chars().count()
    }

    #[inline]
    fn take_quoted_scalar_ascii_chunk(&mut self, single: bool) -> &str {
        let quote = if single { b'\'' } else { b'"' };
        let end = self
            .buffer
            .as_bytes()
            .iter()
            .position(|&byte| {
                !(b'!'..=b'~').contains(&byte) || byte == quote || (!single && byte == b'\\')
            })
            .unwrap_or(self.buffer.len());

        // The run is entirely ASCII, so its end is a UTF-8 boundary.
        let (chunk, remaining) = self.buffer.split_at(end);
        self.buffer = remaining;
        chunk
    }

    fn fetch_plain_scalar_chunk(
        &mut self,
        out: &mut String,
        _count: usize,
        flow_level_gt_0: bool,
    ) -> (bool, usize) {
        let (byte_pos, chars_consumed) = self.plain_scalar_chunk_len(flow_level_gt_0);
        out.push_str(&self.buffer[..byte_pos]);
        self.buffer = &self.buffer[byte_pos..];
        (true, chars_consumed)
    }

    fn skip_plain_scalar_chunk(&mut self, _count: usize, flow_level_gt_0: bool) -> (bool, usize) {
        let (byte_pos, chars_consumed) = self.plain_scalar_chunk_len(flow_level_gt_0);
        self.buffer = &self.buffer[byte_pos..];
        (true, chars_consumed)
    }
}

impl<'a> BorrowedInput<'a> for StrInput<'a> {
    #[inline]
    fn slice_borrowed(&self, start: usize, end: usize) -> Option<&'a str> {
        debug_assert!(start <= end);
        debug_assert!(end <= self.original.len());
        self.original.get(start..end)
    }
}

/// The buffer size we return to the scanner.
///
/// This does not correspond to any allocated buffer size. In practice, the scanner may request any
/// character in the virtual buffer: characters inside the input are returned as-is, and positions
/// past EOF return `\0`.
///
/// The number of characters we are asked to retrieve in [`lookahead`] depends on the buffer size
/// of the input. Our buffer here is virtually unlimited, but the scanner cannot work with that. It
/// may allocate buffers of its own of the size we return in [`bufmaxlen`] (so we can't return
/// [`usize::MAX`]). We can't always return the number of characters left either, as the scanner
/// expects [`buflen`] to return the same value that was given to [`lookahead`] right after its
/// call.
///
/// This creates a complex situation where [`bufmaxlen`] influences what value [`lookahead`] is
/// called with, which in turn dictates what [`buflen`] returns. In order to avoid breaking any
/// function, we return this constant in [`bufmaxlen`] which, since the input is processed one line
/// at a time, should fit what we expect to be a good balance between memory consumption and what
/// we expect the maximum line length to be.
///
/// [`lookahead`]: `StrInput::lookahead`
/// [`bufmaxlen`]: `StrInput::bufmaxlen`
/// [`buflen`]: `StrInput::buflen`
const BUFFER_LEN: usize = 128;

/// Splits the first character of the given string and returns it along with the rest of the
/// string.
#[inline]
fn split_first_char(s: &str) -> Option<(char, &str)> {
    let mut chars = s.chars();
    let c = chars.next()?;
    Some((c, chars.as_str()))
}

#[cfg(test)]
mod test {
    use alloc::string::String;

    use crate::{
        error::ErrorKind,
        input::{BorrowedInput, Input, SkipTabs, WhitespaceResult},
    };

    use super::StrInput;

    #[test]
    pub fn is_document_start() {
        let input = StrInput::new("---\n");
        assert!(input.next_is_document_start());
        assert!(input.next_is_document_indicator());
        let input = StrInput::new("---");
        assert!(input.next_is_document_start());
        assert!(input.next_is_document_indicator());
        let input = StrInput::new("...\n");
        assert!(!input.next_is_document_start());
        assert!(input.next_is_document_indicator());
        let input = StrInput::new("--- ");
        assert!(input.next_is_document_start());
        assert!(input.next_is_document_indicator());
    }

    #[test]
    pub fn is_document_end() {
        let input = StrInput::new("...\n");
        assert!(input.next_is_document_end());
        assert!(input.next_is_document_indicator());
        let input = StrInput::new("...");
        assert!(input.next_is_document_end());
        assert!(input.next_is_document_indicator());
        let input = StrInput::new("---\n");
        assert!(!input.next_is_document_end());
        assert!(input.next_is_document_indicator());
        let input = StrInput::new("... ");
        assert!(input.next_is_document_end());
        assert!(input.next_is_document_indicator());
    }

    #[test]
    fn raw_reads_track_byte_offsets_and_eof() {
        let mut input = StrInput::new("aé");

        assert_eq!(input.raw_read_ch(), 'a');
        assert_eq!(input.byte_offset(), Some(1));
        assert_eq!(input.raw_read_ch(), 'é');
        assert_eq!(input.byte_offset(), Some(3));
        assert_eq!(input.raw_read_ch(), '\0');
        assert_eq!(input.byte_offset(), Some(3));
    }

    #[test]
    fn raw_read_non_breakz_stops_before_breakz() {
        let mut input = StrInput::new("a\n");

        assert_eq!(input.raw_read_non_breakz_ch(), Some('a'));
        assert_eq!(input.raw_read_non_breakz_ch(), None);
        assert_eq!(input.peek(), '\n');

        let mut empty = StrInput::new("");
        assert_eq!(empty.raw_read_non_breakz_ch(), None);
    }

    #[test]
    fn skip_handles_ascii_unicode_and_eof() {
        let mut input = StrInput::new("éab");

        input.skip();
        assert_eq!(input.peek(), 'a');

        input.skip_n(8);
        assert_eq!(input.peek(), '\0');

        input.skip();
        assert_eq!(input.peek(), '\0');
    }

    #[test]
    fn peeking_past_end_returns_nul() {
        let ascii = StrInput::new("ab");
        assert_eq!(ascii.peek_nth(1), 'b');
        assert_eq!(ascii.peek_nth(3), '\0');

        let unicode = StrInput::new("éab");
        assert!(unicode.next_3_are('é', 'a', 'b'));
        assert!(!unicode.next_3_are('é', 'a', 'c'));
    }

    #[test]
    fn skip_ws_to_eol_without_tabs_stops_before_tab() {
        let mut input = StrInput::new("  \t# comment\n");

        let (consumed, result) = input.skip_ws_to_eol(SkipTabs::No);

        assert_eq!(consumed, 2);
        let result = result.unwrap();
        assert!(!result.found_tabs());
        assert!(result.has_valid_yaml_ws());
        assert_eq!(input.peek(), '\t');
    }

    #[test]
    fn skip_ws_to_eol_skips_comments_after_whitespace() {
        let mut input = StrInput::new("  # comment\nnext");

        let (consumed, result) = input.skip_ws_to_eol(SkipTabs::Yes);

        assert_eq!(consumed, 11);
        let result = result.unwrap();
        assert!(!result.found_tabs());
        assert!(result.has_valid_yaml_ws());
        assert_eq!(input.peek(), '\n');
    }

    #[test]
    fn skip_ws_to_eol_rejects_unseparated_comment() {
        for policy in [SkipTabs::Yes, SkipTabs::No] {
            let mut input = StrInput::new("#é中\n");

            let (consumed, result) = input.skip_ws_to_eol(policy);

            assert_eq!(consumed, 0);
            assert_eq!(result, Err(ErrorKind::CommentNotSeparated));
            assert_eq!(input.byte_offset(), Some(0));
            assert_eq!(input.peek(), '#');
        }
    }

    #[test]
    fn skip_ws_to_eol_counts_unicode_comments_and_tracks_whitespace_outside_them() {
        for (source, consumed, expected, byte_offset, next) in [
            ("", 0, WhitespaceResult::new(false, false), 0, '\0'),
            (" \t#é中\r\n", 5, WhitespaceResult::new(true, true), 8, '\r'),
            ("\t# é 中\n", 6, WhitespaceResult::new(true, false), 9, '\n'),
            (" #é中", 4, WhitespaceResult::new(false, true), 7, '\0'),
        ] {
            let mut input = StrInput::new(source);
            assert_eq!(
                input.skip_ws_to_eol(SkipTabs::Yes),
                (consumed, Ok(expected))
            );
            assert_eq!(input.byte_offset(), Some(byte_offset));
            assert_eq!(input.peek(), next);
        }
    }

    #[test]
    fn skip_ws_to_eol_blanks_preserves_comments_and_unskipped_tabs() {
        for (source, policy, consumed, found_tabs, found_spaces, next) in [
            ("", SkipTabs::Yes, 0, false, false, '\0'),
            ("  #é中\n", SkipTabs::Yes, 2, false, true, '#'),
            ("\t#é中\n", SkipTabs::Yes, 1, true, false, '#'),
            (" \t#é中\n", SkipTabs::Yes, 2, true, true, '#'),
            (" \t#é中\n", SkipTabs::No, 1, false, true, '\t'),
            ("\t#é中\n", SkipTabs::No, 0, false, false, '\t'),
        ] {
            let mut input = StrInput::new(source);
            assert_eq!(
                input.skip_ws_to_eol_blanks(policy),
                (consumed, WhitespaceResult::new(found_tabs, found_spaces)),
                "source: {source:?}",
            );
            assert_eq!(input.byte_offset(), Some(consumed));
            assert_eq!(input.peek(), next, "source: {source:?}");
        }
    }

    #[test]
    fn fetch_while_is_alpha_is_ascii_only() {
        let mut input = StrInput::new("abc_123-é");
        let mut out = String::new();

        assert_eq!(input.fetch_while_is_alpha(&mut out), 8);
        assert_eq!(out, "abc_123-");
        assert_eq!(input.peek(), 'é');
    }

    #[test]
    fn fetch_plain_scalar_chunk_handles_non_ascii_after_colon() {
        let mut input = StrInput::new("a:é ");
        let mut out = String::new();

        assert_eq!(
            input.fetch_plain_scalar_chunk(&mut out, 16, false),
            (true, 3)
        );
        assert_eq!(out, "a:é");
        assert_eq!(input.peek(), ' ');
    }

    #[test]
    fn fetch_plain_scalar_chunk_stops_at_flow_indicator() {
        let mut input = StrInput::new("abc,def");
        let mut out = String::new();

        assert_eq!(
            input.fetch_plain_scalar_chunk(&mut out, 16, true),
            (true, 3)
        );
        assert_eq!(out, "abc");
        assert_eq!(input.peek(), ',');
    }

    #[test]
    fn borrowed_slices_use_original_input_lifetime() {
        let input = StrInput::new("aéz");

        assert_eq!(BorrowedInput::slice_borrowed(&input, 1, 3), Some("é"));
        assert_eq!(input.slice_bytes(3, 4), Some("z"));
    }
}
