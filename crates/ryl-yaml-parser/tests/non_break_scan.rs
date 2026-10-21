use ryl_yaml_parser::{Input, StrInput};

#[test]
fn ascii_prefix_keeps_unicode_character_and_byte_offsets() {
    let line = "ascii\ttext é🦀\u{85}\u{2028}\u{feff}tail";
    let source = format!("{line}\nnext");
    let mut input = StrInput::new(&source);
    assert_eq!(input.skip_while_non_breakz(), line.chars().count());
    assert_eq!(input.byte_offset(), Some(line.len()));
    assert_eq!(input.look_ch(), '\n');
}

#[test]
fn non_break_scan_leaves_line_breaks_and_invalid_controls_unconsumed() {
    let source = "ab\r\ncd\0ef\u{7f}gh\u{1}ij";
    let mut input = StrInput::new(source);
    assert_eq!(input.skip_while_non_breakz(), 2);
    assert_eq!(input.look_ch(), '\r');
    input.skip();
    assert_eq!(input.skip_while_non_breakz(), 0);
    assert_eq!(input.look_ch(), '\n');
    input.skip();
    assert_eq!(input.skip_while_non_breakz(), 2);
    assert_eq!(input.look_ch(), '\0');
    input.skip();
    assert_eq!(input.skip_while_non_breakz(), 2);
    assert_eq!(input.look_ch(), '\u{7f}');
    input.skip();
    assert_eq!(input.skip_while_non_breakz(), 2);
    assert_eq!(input.look_ch(), '\u{1}');
    input.skip();
    assert_eq!(input.skip_while_non_breakz(), 2);
    assert_eq!(input.byte_offset(), Some(source.len()));
    assert_eq!(input.look_ch(), '\0');
}
