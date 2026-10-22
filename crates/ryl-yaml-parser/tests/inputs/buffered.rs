use ryl_yaml_parser::input::{BorrowedInput, Input};
use ryl_yaml_parser::{BufferedInput, StrInput};

// The fixed capacity of `BufferedInput`'s lookahead buffer.
const BUFFER_LEN: usize = 16;

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
