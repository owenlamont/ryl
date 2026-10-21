use crate::char_traits::is_breakz;
use crate::error::ErrorKind;
use crate::input::{BorrowedInput, Input};

use arraydeque::ArrayDeque;

/// The size of the [`BufferedInput`] buffer.
///
/// The buffer is statically allocated to avoid conditions for reallocations each time we
/// consume/push a character. As of now, almost all lookaheads are 4 characters maximum, except:
///   - Escape sequences parsing: some escape codes are 8 characters
///   - Scanning indent in scalars: this looks ahead `indent + 2` characters
///
/// This constant must be set to at least 8. When scanning indent in scalars, the lookahead is done
/// in a single call if and only if the indent is `BUFFER_LEN - 2` or less. If the indent is higher
/// than that, the code will fall back to a loop of lookaheads.
const BUFFER_LEN: usize = 16;

/// A wrapper around an [`Iterator`] of [`char`]s with a buffer.
///
/// The YAML scanner often needs some lookahead. With fully allocated buffers such as `String` or
/// `&str`, this is not an issue. However, with streams, we need to have a way of peeking multiple
/// characters at a time and sometimes pushing some back into the stream.
/// Doing this directly with iterator adapters would require pulling in all of `itertools` for one
/// method, so this structure keeps the buffering local.
#[allow(clippy::module_name_repetitions)]
pub struct BufferedInput<T: Iterator<Item = char>> {
    /// The iterator source.
    input: T,
    /// Buffer for the next characters to consume.
    buffer: ArrayDeque<char, BUFFER_LEN>,
    /// Number of front buffer characters that came from the iterator, not EOF padding.
    real_buffered: usize,
    /// Largest active lookahead window requested by the scanner.
    lookahead: usize,
    /// Whether the wrapped iterator has reported EOF.
    source_exhausted: bool,
}

impl<T: Iterator<Item = char>> BufferedInput<T> {
    /// Create a new [`BufferedInput`] over the given character iterator.
    #[must_use]
    pub fn new(input: T) -> Self {
        Self {
            input,
            buffer: ArrayDeque::default(),
            real_buffered: 0,
            lookahead: 0,
            source_exhausted: false,
        }
    }

    fn push_source_or_padding(&mut self) {
        let c = if self.source_exhausted {
            '\0'
        } else if let Some(c) = self.input.next() {
            self.real_buffered += 1;
            c
        } else {
            self.source_exhausted = true;
            '\0'
        };
        self.buffer.push_back(c).unwrap();
    }

    fn fill_lookahead(&mut self) {
        while self.buffer.len() < self.lookahead {
            self.push_source_or_padding();
        }
    }

    fn pop_buffered(&mut self) -> Option<(char, bool)> {
        let c = self.buffer.pop_front()?;
        let is_real = self.real_buffered > 0;
        if is_real {
            self.real_buffered -= 1;
        }
        Some((c, is_real))
    }

    fn read_source_or_eof(&mut self) -> (char, bool) {
        if self.source_exhausted {
            ('\0', false)
        } else if let Some(c) = self.input.next() {
            (c, true)
        } else {
            self.source_exhausted = true;
            ('\0', false)
        }
    }

    fn raw_read_front(&mut self) -> (char, bool) {
        let read = self
            .pop_buffered()
            .unwrap_or_else(|| self.read_source_or_eof());
        self.fill_lookahead();
        read
    }

    fn skip_one(&mut self) -> bool {
        let skipped = match self.pop_buffered() {
            Some((_, true)) => true,
            Some((_, false)) => {
                self.buffer.push_front('\0').unwrap();
                false
            }
            None => self.read_source_or_eof().1,
        };

        if skipped {
            self.fill_lookahead();
        }
        skipped
    }
}

impl<T: Iterator<Item = char>> Input for BufferedInput<T> {
    #[inline]
    fn lookahead(&mut self, count: usize) {
        self.lookahead = self.lookahead.max(count.min(BUFFER_LEN));
        self.fill_lookahead();
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
        self.raw_read_front().0
    }

    #[inline]
    fn raw_read_non_breakz_ch(&mut self) -> Option<char> {
        if let Some(c) = self.buffer.front().copied() {
            if is_breakz(c) {
                None
            } else {
                Some(self.raw_read_front().0)
            }
        } else {
            let (c, is_real) = self.read_source_or_eof();
            if !is_real {
                None
            } else if is_breakz(c) {
                self.buffer.push_back(c).unwrap();
                self.real_buffered += 1;
                None
            } else {
                self.fill_lookahead();
                Some(c)
            }
        }
    }

    #[inline]
    fn skip(&mut self) {
        self.skip_one();
    }

    #[inline]
    fn skip_n(&mut self, count: usize) {
        for _ in 0..count {
            if !self.skip_one() {
                break;
            }
        }
    }

    #[inline]
    fn peek(&self) -> char {
        self.buffer.front().copied().unwrap_or('\0')
    }

    #[inline]
    fn peek_nth(&self, n: usize) -> char {
        self.buffer.get(n).copied().unwrap_or('\0')
    }

    #[inline]
    fn next_is_z(&self) -> bool {
        self.source_exhausted && self.real_buffered == 0
    }
}

/// `BufferedInput` does not support zero-copy slicing since it's a streaming input
/// without stable backing storage.
impl<T: Iterator<Item = char>> BorrowedInput<'static> for BufferedInput<T> {
    #[inline]
    fn slice_borrowed(&self, _start: usize, _end: usize) -> Option<&'static str> {
        None
    }
}

/// Adapter that exposes successful items to [`BufferedInput`] and latches the first source error.
struct FallibleChars<T: Iterator<Item = Result<char, ErrorKind>>> {
    input: T,
    error: Option<ErrorKind>,
    finished: bool,
}

impl<T: Iterator<Item = Result<char, ErrorKind>>> FallibleChars<T> {
    fn new(input: T) -> Self {
        Self {
            input,
            error: None,
            finished: false,
        }
    }
}

impl<T: Iterator<Item = Result<char, ErrorKind>>> Iterator for FallibleChars<T> {
    type Item = char;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }

        match self.input.next() {
            Some(Ok(c)) => Some(c),
            Some(Err(error)) => {
                self.error = Some(error);
                self.finished = true;
                None
            }
            None => {
                self.finished = true;
                None
            }
        }
    }
}

/// A buffered wrapper around a fallible iterator of characters.
///
/// The iterator uses its normal `None` return value for clean end-of-input and returns source
/// failures as `Some(Err(error))`, where `error` is an [`ErrorKind`]. The first error is latched,
/// parsing becomes terminal, and the underlying iterator is never polled again.
#[allow(clippy::module_name_repetitions)]
pub struct FallibleBufferedInput<T: Iterator<Item = Result<char, ErrorKind>>> {
    inner: BufferedInput<FallibleChars<T>>,
}

impl<T: Iterator<Item = Result<char, ErrorKind>>> FallibleBufferedInput<T> {
    /// Create a buffered input over a fallible character iterator.
    #[must_use]
    pub fn new(input: T) -> Self {
        Self {
            inner: BufferedInput::new(FallibleChars::new(input)),
        }
    }
}

impl<T: Iterator<Item = Result<char, ErrorKind>>> Input for FallibleBufferedInput<T> {
    #[inline]
    fn lookahead(&mut self, count: usize) {
        self.inner.lookahead(count);
    }

    #[inline]
    fn buflen(&self) -> usize {
        self.inner.buflen()
    }

    #[inline]
    fn bufmaxlen(&self) -> usize {
        self.inner.bufmaxlen()
    }

    #[inline]
    fn raw_read_ch(&mut self) -> char {
        self.inner.raw_read_ch()
    }

    #[inline]
    fn raw_read_non_breakz_ch(&mut self) -> Option<char> {
        self.inner.raw_read_non_breakz_ch()
    }

    #[inline]
    fn skip(&mut self) {
        self.inner.skip();
    }

    #[inline]
    fn skip_n(&mut self, count: usize) {
        self.inner.skip_n(count);
    }

    #[inline]
    fn peek(&self) -> char {
        self.inner.peek()
    }

    #[inline]
    fn peek_nth(&self, n: usize) -> char {
        self.inner.peek_nth(n)
    }

    #[inline]
    fn next_is_z(&self) -> bool {
        self.inner.next_is_z()
    }

    #[inline]
    fn take_source_error(&mut self) -> Option<ErrorKind> {
        self.inner.input.error.take()
    }
}

/// `FallibleBufferedInput` is a streaming input and cannot provide stable borrowed slices.
impl<T: Iterator<Item = Result<char, ErrorKind>>> BorrowedInput<'static>
    for FallibleBufferedInput<T>
{
    #[inline]
    fn slice_borrowed(&self, _start: usize, _end: usize) -> Option<&'static str> {
        None
    }
}

#[cfg(test)]
mod tests {
    use crate::input::str::StrInput;

    use super::*;

    #[test]
    fn lookahead_larger_than_buffer_is_clamped() {
        let mut input = BufferedInput::new("abc".chars());

        input.lookahead(BUFFER_LEN + 8);

        assert_eq!(input.buflen(), BUFFER_LEN);
        assert_eq!(input.peek(), 'a');
        assert_eq!(input.peek_nth(1), 'b');
        assert_eq!(input.peek_nth(2), 'c');
        assert_eq!(input.peek_nth(3), '\0');
    }

    #[test]
    fn raw_reads_use_stream_front_and_report_eof() {
        let mut input = BufferedInput::new("a".chars());

        assert_eq!(input.raw_read_ch(), 'a');
        assert_eq!(input.raw_read_ch(), '\0');

        let mut input = BufferedInput::new("ab".chars());
        input.lookahead(1);
        assert_eq!(input.raw_read_ch(), 'a');
        assert_eq!(input.peek(), 'b');
    }

    #[test]
    fn raw_read_non_breakz_leaves_break_at_stream_front() {
        let mut input = BufferedInput::new("a\n".chars());

        assert_eq!(input.raw_read_non_breakz_ch(), Some('a'));
        assert_eq!(input.raw_read_non_breakz_ch(), None);
        assert_eq!(input.peek(), '\n');
        input.lookahead(1);
        assert_eq!(input.buflen(), 1);
        assert_eq!(input.peek(), '\n');

        let mut empty = BufferedInput::new("".chars());
        assert_eq!(empty.raw_read_non_breakz_ch(), None);
    }

    #[test]
    fn skip_n_consumes_stream_front_and_preserves_lookahead_window() {
        let mut input = BufferedInput::new("abcdef".chars());

        input.lookahead(5);
        input.skip_n(2);

        assert_eq!(input.buflen(), 5);
        assert_eq!(input.peek(), 'c');
        assert_eq!(input.peek_nth(3), 'f');
        assert_eq!(input.peek_nth(4), '\0');
    }

    #[test]
    fn skip_without_lookahead_consumes_like_str_input() {
        let mut buffered = BufferedInput::new("ab".chars());
        buffered.skip();
        buffered.lookahead(1);

        let mut str_input = StrInput::new("ab");
        str_input.skip();
        str_input.lookahead(1);

        assert_eq!(buffered.peek(), str_input.peek());
    }

    #[test]
    fn skip_n_saturates_at_eof_like_str_input() {
        let mut buffered = BufferedInput::new("abc".chars());
        buffered.lookahead(1);
        buffered.skip_n(8);
        buffered.lookahead(1);

        let mut str_input = StrInput::new("abc");
        str_input.lookahead(1);
        str_input.skip_n(8);
        str_input.lookahead(1);

        assert_eq!(buffered.peek(), str_input.peek());
    }

    #[test]
    fn buflen_matches_str_input_lookahead_window_after_consumption() {
        let mut buffered = BufferedInput::new("ab".chars());
        buffered.lookahead(2);
        buffered.skip();
        buffered.skip();

        let mut str_input = StrInput::new("ab");
        str_input.lookahead(2);
        str_input.skip();
        str_input.skip();

        assert_eq!(buffered.buflen(), str_input.buflen());
        assert_eq!(buffered.buf_is_empty(), str_input.buf_is_empty());
        assert_eq!(buffered.peek(), str_input.peek());
    }

    #[test]
    fn streaming_input_never_borrows_slices() {
        let input = BufferedInput::new("abc".chars());

        assert_eq!(BorrowedInput::slice_borrowed(&input, 0, 1), None);
    }
}
