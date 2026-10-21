//! Utilities to create a source of input to the parser.
//!
//! [`Input`] must be implemented for the parser to fetch input. Make sure your needs aren't
//! covered by the [`BufferedInput`].

use alloc::string::String;

use crate::error::ErrorKind;

pub(crate) mod buffered;
pub(crate) mod str;

#[allow(clippy::module_name_repetitions)]
pub use buffered::{BufferedInput, FallibleBufferedInput};

/// A trait for inputs that can provide borrowed slices with a specific lifetime.
///
/// This trait enables zero-copy (`Cow::Borrowed`) token values for inputs that keep a stable
/// backing string. The key difference from [`Input::slice_bytes`] is that this method returns
/// a slice with the input's original lifetime `'a`, not tied to `&self`.
///
/// For inputs that support zero-copy (like [`str::StrInput`]), this returns `Some(&'a str)`.
/// For streaming inputs that don't have stable backing storage, this returns `None`.
pub trait BorrowedInput<'a>: Input {
    /// Return a borrowed slice of the underlying source between two byte offsets.
    ///
    /// Unlike [`Input::slice_bytes`], this returns a slice with the input's lifetime `'a`,
    /// allowing the slice to outlive the borrow of `&self`.
    ///
    /// `start` and `end` are byte offsets as returned by [`Input::byte_offset`]. The interval is
    /// half-open: `[start, end)`.
    ///
    /// Returns `None` if the input does not support zero-copy slicing.
    ///
    /// # Panics
    /// Implementations may panic in debug builds if `start` is greater than `end` or `end` is past
    /// the end of the underlying source.
    #[track_caller]
    #[must_use]
    fn slice_borrowed(&self, start: usize, end: usize) -> Option<&'a str>;
}

pub use crate::char_traits::{
    is_alpha, is_blank, is_blank_or_breakz, is_break, is_breakz, is_digit, is_flow, is_z,
};

/// Interface for a source of characters.
///
/// Hiding the input's implementation behind this trait allows input-specific optimizations, such
/// as using `str` methods instead of manually transferring one `char` at a time to a buffer.
/// Implementations with stable backing storage can also return borrowed `&str` slices and avoid
/// allocating token values.
///
/// # Scalar scanning hooks
///
/// [`Self::fetch_block_scalar_line`] and [`Self::take_quoted_scalar_ascii_chunk`] allow bulk
/// processing of block and quoted scalar content. Both provide defaults: custom inputs need
/// only override them when they can offer a more efficient implementation.
///
/// Overrides must consume from the logical stream front, including any characters already
/// buffered for lookahead, and keep [`Self::byte_offset`] accurate if it is supported. They
/// must work without a preceding [`Self::lookahead`] call; callers refresh lookahead before
/// inspecting subsequent characters. Source errors must remain available through
/// [`Self::take_source_error`], and a terminal source error must prevent further source reads.
/// The scanner, not the input hook, updates line/column markers and interprets YAML syntax.
pub trait Input {
    /// A hint to the input source that we will need to read `count` characters.
    ///
    /// If the input is exhausted, `\0` can be used to pad the last characters and later returned.
    /// The characters must not be consumed, but may be placed in an internal buffer.
    ///
    /// This method may be a no-op if buffering yields no performance improvement.
    ///
    /// Implementers of [`Input`] must _not_ expose a lookahead window larger than
    /// [`Input::bufmaxlen`]. They may retain a larger window requested by an earlier call; callers
    /// should use [`Input::buflen`] to observe the currently available window.
    fn lookahead(&mut self, count: usize);

    /// Return the number of characters in the active lookahead window.
    ///
    /// This is the number of characters that the input promises can be read through [`peek`] and
    /// [`peek_nth`] after prior [`lookahead`] calls. It is not necessarily the number of source
    /// characters remaining: inputs may keep the window available after consuming characters and
    /// may pad positions past EOF with `\0`.
    ///
    /// [`lookahead`]: Input::lookahead
    /// [`peek`]: Input::peek
    /// [`peek_nth`]: Input::peek_nth
    #[must_use]
    fn buflen(&self) -> usize;

    /// Return the maximum number of characters this input can buffer for lookahead.
    #[must_use]
    fn bufmaxlen(&self) -> usize;

    /// Return whether the active lookahead window is empty.
    ///
    /// This is equivalent to `self.buflen() == 0`. It does not mean the underlying source is
    /// exhausted: after a previous [`lookahead`] call, an input may keep a non-empty lookahead
    /// window available even after all source characters have been consumed, with positions past
    /// EOF observed as `\0`.
    ///
    /// [`lookahead`]: Input::lookahead
    #[inline]
    #[must_use]
    fn buf_is_empty(&self) -> bool {
        self.buflen() == 0
    }

    /// Read the next character from the logical input stream and return it directly.
    ///
    /// If an implementation has already fetched characters for lookahead, this consumes the
    /// buffered stream front before reading farther from the underlying source.
    #[must_use]
    fn raw_read_ch(&mut self) -> char;

    /// Read a non-breakz character from the input stream and return it directly.
    ///
    /// If an implementation has already fetched characters for lookahead, this consumes from the
    /// buffered stream front before reading farther from the underlying source.
    ///
    /// If the next character is a breakz, it is either not consumed or placed into the buffer (if
    /// any).
    #[must_use]
    fn raw_read_non_breakz_ch(&mut self) -> Option<char>;

    /// Consume the next character.
    fn skip(&mut self);

    /// Consume the next `count` characters.
    fn skip_n(&mut self, count: usize);

    /// Return the next character, without consuming it.
    ///
    /// Users of the [`Input`] must make sure that the character has been loaded through a prior
    /// call to [`Input::lookahead`]. Implementors of [`Input`] may assume that a valid call to
    /// [`Input::lookahead`] has been made beforehand.
    ///
    /// # Return
    /// If the input source is not exhausted, returns the next character to be fed into the
    /// scanner. Otherwise, returns `\0`.
    #[must_use]
    fn peek(&self) -> char;

    /// Return the `n`-th character in the buffer, without consuming it.
    ///
    /// This function assumes that the `n`-th character in the input has already been fetched through
    /// [`Input::lookahead`].
    #[must_use]
    fn peek_nth(&self, n: usize) -> char;

    /// Return the current byte offset in the underlying source, if available.
    ///
    /// This is an *optional* capability that enables zero-copy (`Cow::Borrowed`) token values
    /// for inputs that keep a stable backing string (notably [`str::StrInput`]).
    ///
    /// The returned value (when `Some`) is the number of bytes that have been consumed so far,
    /// i.e. an offset into the original source string.
    ///
    /// # Correctness contract
    /// Implementations returning `Some(_)` must satisfy all of the following:
    ///
    /// - The offset is a valid UTF-8 boundary in the underlying source.
    /// - The offset is monotonically non-decreasing as characters are consumed.
    /// - The underlying source is stable for the duration of parsing (no reallocation/mutation)
    ///   so that slices returned by [`Input::slice_bytes`] remain valid.
    ///
    /// Inputs that cannot provide stable slicing (e.g. stream/iterator inputs) must return
    /// `None`.
    #[inline]
    #[must_use]
    fn byte_offset(&self) -> Option<usize> {
        None
    }

    /// Return a borrowed slice of the underlying source between two byte offsets.
    ///
    /// This is an *optional* capability used to produce `Cow::Borrowed` values without
    /// allocating.
    ///
    /// `start` and `end` are byte offsets as returned by [`Input::byte_offset`]. The interval is
    /// half-open: `[start, end)`.
    ///
    /// # Correctness contract
    /// Implementations returning `Some(&str)` must ensure:
    ///
    /// - `start <= end`.
    /// - Both offsets are valid UTF-8 boundaries.
    /// - The returned `&str` is a view into the stable underlying source associated with this
    ///   input.
    ///
    /// Implementations that return `None` from [`Input::byte_offset`] must also return `None`
    /// here.
    ///
    /// # Panics
    /// Implementations may panic in debug builds if `start` is greater than `end` or `end` is past
    /// the end of the underlying source.
    #[track_caller]
    #[inline]
    #[must_use]
    fn slice_bytes(&self, _start: usize, _end: usize) -> Option<&str> {
        None
    }

    /// Return whether this input may contain a `#` character.
    ///
    /// This is a conservative performance hint. Inputs that cannot answer cheaply should return
    /// `true`, which keeps full comment handling enabled.
    #[cfg(feature = "comments")]
    #[inline]
    #[must_use]
    fn may_contain_comments(&self) -> bool {
        true
    }

    /// Take a terminal error reported by the underlying source.
    ///
    /// Infallible inputs use the default implementation. Fallible streaming inputs latch their
    /// first source error and return it here so the scanner can distinguish the failure from clean
    /// end-of-input. Once an implementation reports an error, it must not read from its source
    /// again.
    ///
    /// Source adapters should use an input-related [`ErrorKind`] such as
    /// [`ErrorKind::InputIo`], [`ErrorKind::InputDecoding`], or
    /// [`ErrorKind::InputByteLimitExceeded`].
    #[inline]
    fn take_source_error(&mut self) -> Option<ErrorKind> {
        None
    }

    /// Look for the next character and return it.
    ///
    /// The character is not consumed.
    /// Equivalent to calling [`Input::lookahead`] and [`Input::peek`].
    #[inline]
    #[must_use]
    fn look_ch(&mut self) -> char {
        self.lookahead(1);
        self.peek()
    }

    /// Return whether the next character in the input source is equal to `c`.
    ///
    /// This function assumes that the next character in the input has already been fetched through
    /// [`Input::lookahead`].
    #[inline]
    #[must_use]
    fn next_char_is(&self, c: char) -> bool {
        self.peek() == c
    }

    /// Return whether the `n`-th character in the input source is equal to `c`.
    ///
    /// This function assumes that the `n`-th character in the input has already been fetched through
    /// [`Input::lookahead`].
    #[inline]
    #[must_use]
    fn nth_char_is(&self, n: usize, c: char) -> bool {
        self.peek_nth(n) == c
    }

    /// Return whether the next 2 characters in the input source match the given characters.
    ///
    /// This function assumes that the next 2 characters in the input have already been fetched
    /// through [`Input::lookahead`].
    ///
    /// # Panics
    /// Panics if the active lookahead window contains fewer than 2 characters.
    #[track_caller]
    #[inline]
    #[must_use]
    fn next_2_are(&self, c1: char, c2: char) -> bool {
        assert!(self.buflen() >= 2);
        self.peek() == c1 && self.peek_nth(1) == c2
    }

    /// Return whether the next 3 characters in the input source match the given characters.
    ///
    /// This function assumes that the next 3 characters in the input have already been fetched
    /// through [`Input::lookahead`].
    ///
    /// # Panics
    /// Panics if the active lookahead window contains fewer than 3 characters.
    #[track_caller]
    #[inline]
    #[must_use]
    fn next_3_are(&self, c1: char, c2: char, c3: char) -> bool {
        assert!(self.buflen() >= 3);
        self.peek() == c1 && self.peek_nth(1) == c2 && self.peek_nth(2) == c3
    }

    /// Check whether the next characters correspond to a document indicator.
    ///
    /// This function assumes that the next 4 characters in the input have already been fetched
    /// through [`Input::lookahead`].
    ///
    /// # Panics
    /// Panics if the active lookahead window contains fewer than 4 characters.
    #[track_caller]
    #[inline]
    #[must_use]
    fn next_is_document_indicator(&self) -> bool {
        assert!(self.buflen() >= 4);
        is_blank_or_breakz(self.peek_nth(3))
            && (self.next_3_are('.', '.', '.') || self.next_3_are('-', '-', '-'))
    }

    /// Check whether the next characters correspond to a start of document.
    ///
    /// This function assumes that the next 4 characters in the input have already been fetched
    /// through [`Input::lookahead`].
    ///
    /// # Panics
    /// Panics if the active lookahead window contains fewer than 4 characters.
    #[track_caller]
    #[inline]
    #[must_use]
    fn next_is_document_start(&self) -> bool {
        assert!(self.buflen() >= 4);
        self.next_3_are('-', '-', '-') && is_blank_or_breakz(self.peek_nth(3))
    }

    /// Check whether the next characters correspond to an end of document.
    ///
    /// This function assumes that the next 4 characters in the input have already been fetched
    /// through [`Input::lookahead`].
    ///
    /// # Panics
    /// Panics if the active lookahead window contains fewer than 4 characters.
    #[track_caller]
    #[inline]
    #[must_use]
    fn next_is_document_end(&self) -> bool {
        assert!(self.buflen() >= 4);
        self.next_3_are('.', '.', '.') && is_blank_or_breakz(self.peek_nth(3))
    }

    /// Skip YAML whitespace up to the end of the current line.
    ///
    /// Inline comments are consumed only after at least one preceding YAML whitespace character.
    ///
    /// # Return
    /// Return a tuple with the number of characters that were consumed and the result of skipping
    /// whitespace. The number of characters returned can be used to advance the index and column,
    /// since no end-of-line character will be consumed.
    /// The [`WhitespaceResult`] reports whether tabs and spaces were consumed.
    ///
    /// # Errors
    /// Returns [`ErrorKind::CommentNotSeparated`] if a comment is encountered without preceding
    /// whitespace. In that event, the first tuple element contains the number of characters
    /// consumed prior to reaching the `#`.
    fn skip_ws_to_eol(
        &mut self,
        skip_tabs: SkipTabs,
    ) -> (usize, Result<WhitespaceResult, ErrorKind>) {
        let mut encountered_tab = false;
        let mut has_yaml_ws = false;
        let mut chars_consumed = 0;
        loop {
            match self.look_ch() {
                ' ' => {
                    has_yaml_ws = true;
                    self.skip();
                }
                '\t' if skip_tabs != SkipTabs::No => {
                    encountered_tab = true;
                    self.skip();
                }
                // YAML comments must be preceded by whitespace.
                '#' if !encountered_tab && !has_yaml_ws => {
                    return (chars_consumed, Err(ErrorKind::CommentNotSeparated));
                }
                '#' => {
                    self.skip(); // Skip over '#'
                    while !is_breakz(self.look_ch()) {
                        self.skip();
                        chars_consumed += 1;
                    }
                }
                _ => break,
            }
            chars_consumed += 1;
        }

        (
            chars_consumed,
            Ok(WhitespaceResult::new(encountered_tab, has_yaml_ws)),
        )
    }

    /// Skip YAML blank characters, stopping before comments, line breaks, or other content.
    ///
    /// This is the comment-aware counterpart to [`Input::skip_ws_to_eol`]: it preserves a
    /// following `#` for the scanner to tokenize while still letting input implementations batch
    /// the common run of spaces and tabs.
    ///
    /// # Return
    /// Returns the number of consumed characters and a [`WhitespaceResult`] describing whether
    /// tabs and spaces were consumed.
    fn skip_ws_to_eol_blanks(&mut self, skip_tabs: SkipTabs) -> (usize, WhitespaceResult) {
        let mut encountered_tab = false;
        let mut has_yaml_ws = false;
        let mut chars_consumed = 0;

        loop {
            match self.look_ch() {
                ' ' => {
                    has_yaml_ws = true;
                    chars_consumed += 1;
                    self.skip();
                }
                '\t' if skip_tabs != SkipTabs::No => {
                    encountered_tab = true;
                    chars_consumed += 1;
                    self.skip();
                }
                _ => break,
            }
        }

        (
            chars_consumed,
            WhitespaceResult::new(encountered_tab, has_yaml_ws),
        )
    }

    /// Check whether the next characters may be part of a plain scalar.
    ///
    /// This function assumes we are not given a blankz character.
    #[allow(clippy::inline_always)]
    #[inline(always)]
    #[must_use]
    fn next_can_be_plain_scalar(&self, in_flow: bool) -> bool {
        let nc = self.peek_nth(1);
        match self.peek() {
            // indicators can end a plain scalar, see 7.3.3. Plain Style
            ':' if is_blank_or_breakz(nc) || (in_flow && is_flow(nc)) => false,
            c if in_flow && is_flow(c) => false,
            _ => true,
        }
    }

    /// Check whether the next character is [a blank] or [a break].
    ///
    /// The character must have previously been fetched through [`lookahead`]
    ///
    /// # Return
    /// Returns true if the character is [a blank] or [a break], false otherwise.
    ///
    /// [`lookahead`]: Input::lookahead
    /// [a blank]: is_blank
    /// [a break]: is_break
    #[inline]
    #[must_use]
    fn next_is_blank_or_break(&self) -> bool {
        is_blank(self.peek()) || is_break(self.peek())
    }

    /// Check whether the next character is [a blank] or [a breakz].
    ///
    /// The character must have previously been fetched through [`lookahead`]
    ///
    /// # Return
    /// Returns true if the character is [a blank] or [a break], false otherwise.
    ///
    /// [`lookahead`]: Input::lookahead
    /// [a blank]: is_blank
    /// [a breakz]: is_breakz
    #[inline]
    #[must_use]
    fn next_is_blank_or_breakz(&self) -> bool {
        is_blank(self.peek()) || is_breakz(self.peek())
    }

    /// Check whether the next character is [a blank].
    ///
    /// The character must have previously been fetched through [`lookahead`]
    ///
    /// # Return
    /// Returns true if the character is [a blank], false otherwise.
    ///
    /// [`lookahead`]: Input::lookahead
    /// [a blank]: is_blank
    #[inline]
    #[must_use]
    fn next_is_blank(&self) -> bool {
        is_blank(self.peek())
    }

    /// Check whether the next character is [a break].
    ///
    /// The character must have previously been fetched through [`lookahead`]
    ///
    /// # Return
    /// Returns true if the character is [a break], false otherwise.
    ///
    /// [`lookahead`]: Input::lookahead
    /// [a break]: is_break
    #[inline]
    #[must_use]
    fn next_is_break(&self) -> bool {
        is_break(self.peek())
    }

    /// Check whether the next character is [a breakz].
    ///
    /// The character must have previously been fetched through [`lookahead`]
    ///
    /// # Return
    /// Returns true if the character is [a breakz], false otherwise.
    ///
    /// [`lookahead`]: Input::lookahead
    /// [a breakz]: is_breakz
    #[inline]
    #[must_use]
    fn next_is_breakz(&self) -> bool {
        is_breakz(self.peek())
    }

    /// Check whether the input is at its physical end.
    ///
    /// The default implementation infers end-of-input from the `\0` sentinel returned by
    /// [`Self::peek`]. Inputs that can distinguish a literal NUL from end-of-input should override
    /// this method.
    /// The next position must have previously been fetched through [`Self::lookahead`].
    ///
    /// # Return
    /// Returns true if the input is exhausted, false otherwise.
    #[inline]
    #[must_use]
    fn next_is_z(&self) -> bool {
        is_z(self.peek())
    }

    /// Check whether the next character is [a flow].
    ///
    /// The character must have previously been fetched through [`lookahead`]
    ///
    /// # Return
    /// Returns true if the character is [a flow], false otherwise.
    ///
    /// [`lookahead`]: Input::lookahead
    /// [a flow]: is_flow
    #[inline]
    #[must_use]
    fn next_is_flow(&self) -> bool {
        is_flow(self.peek())
    }

    /// Check whether the next character is [a digit].
    ///
    /// The character must have previously been fetched through [`lookahead`]
    ///
    /// # Return
    /// Returns true if the character is [a digit], false otherwise.
    ///
    /// [`lookahead`]: Input::lookahead
    /// [a digit]: is_digit
    #[inline]
    #[must_use]
    fn next_is_digit(&self) -> bool {
        is_digit(self.peek())
    }

    /// Check whether the next character is [a letter].
    ///
    /// The character must have previously been fetched through [`lookahead`]
    ///
    /// # Return
    /// Returns true if the character is [a letter], false otherwise.
    ///
    /// [`lookahead`]: Input::lookahead
    /// [a letter]: is_alpha
    #[inline]
    #[must_use]
    fn next_is_alpha(&self) -> bool {
        is_alpha(self.peek())
    }

    /// Skip printable characters until a [breakz] or non-printable character is found.
    ///
    /// The stopping character is not consumed.
    ///
    /// # Return
    /// Return the number of characters that were consumed. The number of characters returned can
    /// be used to advance the index and column, since no end-of-line character will be consumed.
    ///
    /// [breakz]: is_breakz
    #[inline]
    fn skip_while_non_breakz(&mut self) -> usize {
        let mut count = 0;
        while {
            let c = self.look_ch();
            !is_breakz(c) && crate::char_traits::is_printable(c)
        } {
            count += 1;
            self.skip();
        }
        count
    }

    /// Skip characters from the input while [blanks] are found.
    ///
    /// The characters are consumed from the input.
    ///
    /// # Return
    /// Return the number of characters that were consumed. The number of characters returned can
    /// be used to advance the index and column, since no end-of-line character will be consumed.
    ///
    /// [blanks]: is_blank
    fn skip_while_blank(&mut self) -> usize {
        let mut n_bytes = 0;
        while is_blank(self.look_ch()) {
            n_bytes += self.peek().len_utf8();
            self.skip();
        }
        n_bytes
    }

    /// Fetch characters from the input while we encounter letters and store them in `out`.
    ///
    /// The characters are consumed from the input.
    ///
    /// # Return
    /// Return the number of characters that were consumed. The number of characters returned can
    /// be used to advance the index and column, since no end-of-line character will be consumed.
    fn fetch_while_is_alpha(&mut self, out: &mut String) -> usize {
        let mut n_bytes = 0;
        while is_alpha(self.look_ch()) {
            let c = self.peek();
            n_bytes += c.len_utf8();
            out.push(c);
            self.skip();
        }
        n_bytes
    }

    /// Fetch characters as long as they satisfy `is_yaml_non_space(c)`.
    ///
    /// The characters are consumed from the input.
    ///
    /// # Return
    /// Return the number of characters that were consumed. The number of characters returned can
    /// be used to advance the index and column, since no end-of-line character will be consumed.
    fn fetch_while_is_yaml_non_space(&mut self, out: &mut String) -> usize {
        let mut chars_consumed = 0;
        loop {
            let c = self.look_ch();
            if !crate::char_traits::is_yaml_non_space(c) || is_z(c) {
                break;
            }
            let c = self.peek();
            out.push(c);
            self.skip();
            chars_consumed += 1;
        }
        chars_consumed
    }

    /// Append a block scalar's content line to `out`, stopping before CR, LF, NUL, or EOF.
    ///
    /// The caller positions the input after the line's indentation. This method consumes the
    /// entire remaining content line and appends it without clearing existing contents of `out`.
    /// The stopping character is not consumed; in particular, both characters of CRLF remain
    /// unconsumed, even if already buffered for lookahead.
    ///
    /// This copies content verbatim, including tabs and any non-printable characters other than
    /// NUL. Unicode characters such as NEL (`U+0085`), line separator (`U+2028`), and paragraph
    /// separator (`U+2029`) are content, not line terminators here. The scanner remains responsible
    /// for validation, indentation, folding, and chomping; overrides must not discard or replace
    /// invalid content.
    ///
    /// The default uses [`Self::raw_read_non_breakz_ch`], including any buffered lookahead.
    /// Inputs with contiguous storage, such as [`str::StrInput`], can override this to append a
    /// source slice in one operation. No prior lookahead is required; callers refresh lookahead
    /// before inspecting the next character. If a source error interrupts the line, append and
    /// count only the characters consumed before it. Keep the error available through
    /// [`Self::take_source_error`]; the return value alone does not distinguish a source failure
    /// from a normal line ending.
    ///
    /// # Returns
    ///
    /// The number of consumed Unicode scalar values (`char`s), **not UTF-8 bytes**, for advancing
    /// the character index and column. A return value of zero means no content was appended or
    /// consumed, for example when already at a line terminator or EOF.
    ///
    /// # Examples
    ///
    /// ```
    /// use granit_parser::{Input, StrInput};
    ///
    /// let mut input = StrInput::new("é🦀\r\nnext");
    /// let mut output = String::from("prefix:");
    /// assert_eq!(input.fetch_block_scalar_line(&mut output), 2);
    /// assert_eq!(output, "prefix:é🦀");
    /// assert_eq!(input.byte_offset(), Some("é🦀".len()));
    /// input.lookahead(2);
    /// assert_eq!(input.peek(), '\r');
    /// assert_eq!(input.peek_nth(1), '\n');
    /// ```
    fn fetch_block_scalar_line(&mut self, out: &mut String) -> usize {
        // Raw reads consume the logical stream front even when lookahead is still buffered.
        let mut chars_consumed = 0;
        while let Some(character) = self.raw_read_non_breakz_ch() {
            out.push(character);
            chars_consumed += 1;
        }
        chars_consumed
    }

    /// Consume and return an ordinary ASCII run inside a quoted scalar, if supported.
    ///
    /// The caller has already consumed the opening quote. The boolean selects the quote style:
    /// `true` for single quotes, `false` for double quotes. This optional optimization batches
    /// characters needing no YAML escape or folding handling. The default returns an empty slice
    /// without consuming input, leaving character-by-character scanning in place.
    ///
    /// # Override contract
    ///
    /// A non-empty result must contain exactly the consumed source prefix, with no decoding or
    /// substitution. Only bytes in `0x21..=0x7e` may be consumed, excluding the matching quote
    /// (`'` for single quotes, otherwise `"`) and, in double-quoted scalars, backslashes.
    /// Backslashes and double quotes are ordinary content in single-quoted scalars; single quotes
    /// are ordinary content in double-quoted scalars. Whitespace, non-ASCII text, control
    /// characters, escapes, and closing or doubled matching quotes are left for the scanner.
    /// The returned byte length is also the number of consumed characters, and any supported
    /// [`Self::byte_offset`] must advance by that length.
    ///
    /// The run need not be maximal. Returning an empty slice must consume nothing, even if an
    /// eligible run is present; it does **not** indicate EOF or the end of the scalar. No prior
    /// lookahead is required, and already-buffered characters must not be skipped. Callers refresh
    /// lookahead before inspecting the next character. The returned slice is tied to the borrow
    /// of `self`, not to the original source lifetime used by [`BorrowedInput::slice_borrowed`].
    ///
    /// # Examples
    ///
    /// ```
    /// use granit_parser::{BufferedInput, Input, StrInput};
    ///
    /// // Remaining content after a double-quoted scalar's opening quote.
    /// let mut input = StrInput::new(r#"name\n""#);
    /// assert_eq!(input.take_quoted_scalar_ascii_chunk(false), "name");
    /// input.lookahead(1);
    /// assert_eq!(input.peek(), '\\'); // The escape is left for the scanner.
    ///
    /// // Streaming inputs may retain the default and consume nothing.
    /// let mut stream = BufferedInput::new("name".chars());
    /// assert_eq!(stream.take_quoted_scalar_ascii_chunk(false), "");
    /// stream.lookahead(1);
    /// assert_eq!(stream.peek(), 'n');
    /// ```
    #[inline]
    // Overrides return slices borrowed from the input, unlike this no-op default.
    #[allow(clippy::unnecessary_literal_bound)]
    fn take_quoted_scalar_ascii_chunk(&mut self, _single: bool) -> &str {
        // Inputs with contiguous storage, such as StrInput, override this to return a whole run.
        ""
    }

    /// Fetch a chunk of plain scalar characters.
    ///
    /// This optimization method allows the input to batch process characters.
    /// Returns (stopped, `chars_consumed`).
    /// stopped is true if the chunk ended because of a non-plain-scalar character.
    fn fetch_plain_scalar_chunk(
        &mut self,
        out: &mut String,
        count: usize,
        flow_level_gt_0: bool,
    ) -> (bool, usize) {
        let mut chars_consumed = 0;
        for _ in 0..count {
            self.lookahead(1);
            if self.next_is_blank_or_breakz() || !self.next_can_be_plain_scalar(flow_level_gt_0) {
                return (true, chars_consumed);
            }
            out.push(self.peek());
            self.skip();
            chars_consumed += 1;
        }
        (false, chars_consumed)
    }

    /// Consume a chunk of plain-scalar characters without materializing them.
    ///
    /// Stable inputs use this while the scanner retains the source as a borrowed slice and only
    /// promotes it to an owned buffer if YAML folding changes the scalar contents.
    fn skip_plain_scalar_chunk(&mut self, count: usize, flow_level_gt_0: bool) -> (bool, usize) {
        let mut chars_consumed = 0;
        for _ in 0..count {
            self.lookahead(1);
            if self.next_is_blank_or_breakz() || !self.next_can_be_plain_scalar(flow_level_gt_0) {
                return (true, chars_consumed);
            }
            self.skip();
            chars_consumed += 1;
        }
        (false, chars_consumed)
    }
}

/// Behavior to adopt regarding treating tabs as whitespace.
///
/// Although tab is valid YAML whitespace, it does not always behave the same as a space.
/// Whitespace results cannot be used as input policies:
///
/// ```compile_fail,E0308
/// use granit_parser::{input::WhitespaceResult, BufferedInput, Input};
///
/// let mut input = BufferedInput::new(" \tvalue".chars());
/// let previous_result = WhitespaceResult::new(true, true);
/// input.skip_ws_to_eol(previous_result); // Expected SkipTabs, not WhitespaceResult.
/// ```
#[derive(Copy, Clone, Eq, PartialEq)]
pub enum SkipTabs {
    /// Skip all tabs as whitespace.
    Yes,
    /// Don't skip any tab. Return from the function when encountering one.
    No,
}

/// Information about whitespace consumed by an input scanning hook.
///
/// Tabs and spaces are tracked independently. When [`SkipTabs::No`] leaves a tab in the input,
/// that tab is not included in this result. Characters inside a skipped comment do not affect
/// either flag.
///
/// # Examples
///
/// ```
/// use granit_parser::{input::{SkipTabs, WhitespaceResult}, BufferedInput, Input};
///
/// let mut input = BufferedInput::new(" \t#comment".chars());
/// let (consumed, result) = input.skip_ws_to_eol_blanks(SkipTabs::Yes);
/// assert_eq!(consumed, 2);
/// assert_eq!(result, WhitespaceResult::new(true, true));
/// assert!(result.found_tabs());
/// assert!(result.has_valid_yaml_ws());
/// ```
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct WhitespaceResult {
    found_tabs: bool,
    has_valid_yaml_ws: bool,
}

impl WhitespaceResult {
    /// Create whitespace information from independently tracked tabs and spaces.
    ///
    /// `has_valid_yaml_ws` indicates whether at least one space was consumed outside a comment.
    #[must_use]
    pub const fn new(found_tabs: bool, has_valid_yaml_ws: bool) -> Self {
        Self {
            found_tabs,
            has_valid_yaml_ws,
        }
    }

    /// Return whether at least one tab was consumed outside a comment.
    #[must_use]
    pub const fn found_tabs(self) -> bool {
        self.found_tabs
    }

    /// Return whether at least one space was consumed outside a comment.
    #[must_use]
    pub const fn has_valid_yaml_ws(self) -> bool {
        self.has_valid_yaml_ws
    }
}

#[cfg(test)]
mod tests {
    use super::{Input, SkipTabs, WhitespaceResult};
    use crate::error::ErrorKind;

    struct MinimalInput;

    impl Input for MinimalInput {
        fn lookahead(&mut self, _count: usize) {}

        fn buflen(&self) -> usize {
            0
        }

        fn bufmaxlen(&self) -> usize {
            0
        }

        fn raw_read_ch(&mut self) -> char {
            '\0'
        }

        fn raw_read_non_breakz_ch(&mut self) -> Option<char> {
            None
        }

        fn skip(&mut self) {}

        fn skip_n(&mut self, _count: usize) {}

        fn peek(&self) -> char {
            '\0'
        }

        fn peek_nth(&self, _n: usize) -> char {
            '\0'
        }
    }

    #[test]
    fn default_slice_bytes_returns_none() {
        let mut input = MinimalInput;

        input.lookahead(4);
        assert_eq!(input.buflen(), 0);
        assert_eq!(input.bufmaxlen(), 0);
        assert_eq!(input.raw_read_ch(), '\0');
        assert_eq!(input.raw_read_non_breakz_ch(), None);
        input.skip();
        input.skip_n(2);
        assert_eq!(input.peek(), '\0');
        assert_eq!(input.peek_nth(1), '\0');
        assert_eq!(input.byte_offset(), None);
        assert_eq!(input.slice_bytes(0, 0), None);
    }

    #[test]
    fn default_skip_ws_to_eol_rejects_unseparated_comment() {
        for policy in [SkipTabs::Yes, SkipTabs::No] {
            let mut input = super::buffered::BufferedInput::new("#é中\n".chars());

            let (consumed, result) = input.skip_ws_to_eol(policy);

            assert_eq!(consumed, 0);
            assert_eq!(result, Err(ErrorKind::CommentNotSeparated));
            assert_eq!(input.peek(), '#');
        }
    }

    #[test]
    fn whitespace_result_flags_are_independent() {
        const TAB_ONLY: WhitespaceResult = WhitespaceResult::new(true, false);
        const TAB_FLAGS: (bool, bool) = (TAB_ONLY.found_tabs(), TAB_ONLY.has_valid_yaml_ws());
        assert_eq!(TAB_FLAGS, (true, false));

        for found_tabs in [false, true] {
            for found_spaces in [false, true] {
                let result = WhitespaceResult::new(found_tabs, found_spaces);
                assert_eq!(result.found_tabs(), found_tabs);
                assert_eq!(result.has_valid_yaml_ws(), found_spaces);
            }
        }
    }

    #[test]
    fn default_skip_ws_to_eol_preserves_policy_flags_and_character_counts() {
        for (source, policy, consumed, found_tabs, found_spaces, next) in [
            ("", SkipTabs::Yes, 0, false, false, '\0'),
            ("  value", SkipTabs::Yes, 2, false, true, 'v'),
            ("\tvalue", SkipTabs::Yes, 1, true, false, 'v'),
            (" \t#é中\r\n", SkipTabs::Yes, 5, true, true, '\r'),
            ("\t# é 中\n", SkipTabs::Yes, 6, true, false, '\n'),
            (" #é中", SkipTabs::Yes, 4, false, true, '\0'),
            (" \t#é中\n", SkipTabs::No, 1, false, true, '\t'),
            ("\t#é中\n", SkipTabs::No, 0, false, false, '\t'),
        ] {
            let mut input = super::buffered::BufferedInput::new(source.chars());
            assert_eq!(
                input.skip_ws_to_eol(policy),
                (
                    consumed,
                    Ok(WhitespaceResult::new(found_tabs, found_spaces))
                ),
                "source: {source:?}",
            );
            assert_eq!(input.peek(), next, "source: {source:?}");
        }
    }

    #[test]
    fn default_skip_ws_to_eol_blanks_preserves_comments_and_unskipped_tabs() {
        for (source, policy, consumed, found_tabs, found_spaces, next) in [
            ("", SkipTabs::Yes, 0, false, false, '\0'),
            ("  #é中\n", SkipTabs::Yes, 2, false, true, '#'),
            ("\t#é中\n", SkipTabs::Yes, 1, true, false, '#'),
            (" \t#é中\n", SkipTabs::Yes, 2, true, true, '#'),
            (" \t#é中\n", SkipTabs::No, 1, false, true, '\t'),
            ("\t#é中\n", SkipTabs::No, 0, false, false, '\t'),
        ] {
            let mut input = super::buffered::BufferedInput::new(source.chars());
            assert_eq!(
                input.skip_ws_to_eol_blanks(policy),
                (consumed, WhitespaceResult::new(found_tabs, found_spaces)),
                "source: {source:?}",
            );
            assert_eq!(input.peek(), next, "source: {source:?}");
        }
    }
}
