use std::path::Path;

use ryl::decoder;
use tempfile::tempdir;

#[test]
fn decode_utf8_plain() {
    assert_eq!(decoder::decode_bytes(b"hello").unwrap(), "hello");
}

#[test]
fn decode_utf8_invalid_reports_error() {
    let err = decoder::decode_bytes(&[0x80]).unwrap_err();
    assert!(
        err.contains("invalid utf-8 data"),
        "expected utf-8 error, got: {err}"
    );
}

#[test]
fn decode_utf8_bom_is_stripped() {
    let data = [0xEF, 0xBB, 0xBF, b'h', b'i'];
    assert_eq!(decoder::decode_bytes(&data).unwrap(), "hi");
}

#[test]
fn decode_utf8_sig_without_bom_is_identity() {
    assert_eq!(
        decoder::decode_bytes_with_override(b"hi", Some("utf-8-sig")).unwrap(),
        "hi"
    );
}

#[test]
fn decode_utf16_big_endian_with_bom() {
    let data = [0xFE, 0xFF, 0x00, b'h', 0x00, b'i'];
    assert_eq!(decoder::decode_bytes(&data).unwrap(), "hi");
}

#[test]
fn decode_utf16_little_endian_with_bom() {
    let data = [0xFF, 0xFE, b'h', 0x00, b'i', 0x00];
    assert_eq!(decoder::decode_bytes(&data).unwrap(), "hi");
}

#[test]
fn decode_utf16_big_endian_without_bom() {
    let data = [0x00, b'h', 0x00, b'i'];
    assert_eq!(decoder::decode_bytes(&data).unwrap(), "hi");
}

#[test]
fn decode_utf16_little_endian_without_bom() {
    let data = [b'h', 0x00, b'i', 0x00];
    assert_eq!(decoder::decode_bytes(&data).unwrap(), "hi");
}

#[test]
fn decode_utf16_invalid_length_reports_error() {
    let err = decoder::decode_bytes(&[0xFF, 0xFE, 0x00]).unwrap_err();
    assert!(err.contains("invalid utf-16"), "unexpected error: {err}");
}

#[test]
fn decode_utf32_big_endian_with_bom() {
    let data = [
        0x00, 0x00, 0xFE, 0xFF, 0x00, 0x00, 0x00, b'h', 0x00, 0x00, 0x00, b'i',
    ];
    assert_eq!(decoder::decode_bytes(&data).unwrap(), "hi");
}

#[test]
fn decode_utf32_little_endian_with_bom() {
    let data = [
        0xFF, 0xFE, 0x00, 0x00, b'h', 0x00, 0x00, 0x00, b'i', 0x00, 0x00, 0x00,
    ];
    assert_eq!(decoder::decode_bytes(&data).unwrap(), "hi");
}

#[test]
fn decode_utf32_big_endian_without_bom() {
    let data = [0x00, 0x00, 0x00, b'h', 0x00, 0x00, 0x00, b'i'];
    assert_eq!(decoder::decode_bytes(&data).unwrap(), "hi");
}

#[test]
fn decode_utf32_little_endian_without_bom() {
    let data = [b'h', 0x00, 0x00, 0x00, b'i', 0x00, 0x00, 0x00];
    assert_eq!(decoder::decode_bytes(&data).unwrap(), "hi");
}

#[test]
fn decode_utf32_invalid_length_reports_error() {
    let data = [0x00, 0x00, 0x00, b'h', 0x00];
    let err = decoder::decode_bytes(&data).unwrap_err();
    assert!(err.contains("invalid utf-32"), "unexpected error: {err}");
}

#[test]
fn decode_utf32_invalid_scalar_reports_error() {
    let data = [0x00, 0x00, 0xFE, 0xFF, 0x00, 0x11, 0x00, 0x00];
    let err = decoder::decode_bytes(&data).unwrap_err();
    assert!(
        err.contains("invalid scalar value"),
        "unexpected error: {err}"
    );
}

#[test]
fn decode_utf16_invalid_scalar_reports_error() {
    let err = decoder::decode_bytes_with_override(&[0xD8, 0x00], Some("utf-16be")).unwrap_err();
    assert!(err.contains("invalid utf-16"));
}

#[test]
fn decode_with_override_utf16_variants() {
    let big = [0xFE, 0xFF, 0x00, b'h', 0x00, b'i'];
    assert_eq!(
        decoder::decode_bytes_with_override(&big, Some("utf-16")).unwrap(),
        "hi"
    );
    let big_no_bom = [0x00, b'h', 0x00, b'i'];
    assert_eq!(
        decoder::decode_bytes_with_override(&big_no_bom, Some("utf-16")).unwrap(),
        "hi"
    );
    let little_bom = [0xFF, 0xFE, b'h', 0x00];
    assert_eq!(
        decoder::decode_bytes_with_override(&little_bom, Some("UTF_16")).unwrap(),
        "h"
    );
    let little = [b'h', 0x00, b'i', 0x00];
    assert_eq!(
        decoder::decode_bytes_with_override(&little, Some("utf-16le")).unwrap(),
        "hi"
    );
    let little_no_bom = [b'h', 0x00, b'i', 0x00];
    assert_eq!(
        decoder::decode_bytes_with_override(&little_no_bom, Some("utf-16")).unwrap(),
        "hi"
    );
}

#[test]
fn decode_with_override_utf16_fallbacks_to_little_when_ambiguous() {
    let bytes = [0x2D, 0x4E, 0x87, 0x65]; // "中文" in little-endian without BOM or zero hints
    assert_eq!(
        decoder::decode_bytes_with_override(&bytes, Some("utf-16")).unwrap(),
        "中文"
    );
}

#[test]
fn decode_with_override_utf32_variants() {
    let big = [0x00, 0x00, 0x00, b'h', 0x00, 0x00, 0x00, b'i'];
    assert_eq!(
        decoder::decode_bytes_with_override(&big, Some("utf-32")).unwrap(),
        "hi"
    );
    let big_bom = [
        0x00, 0x00, 0xFE, 0xFF, 0x00, 0x00, 0x00, b'h', 0x00, 0x00, 0x00, b'i',
    ];
    assert_eq!(
        decoder::decode_bytes_with_override(&big_bom, Some("utf-32")).unwrap(),
        "hi"
    );
    let little = [b'h', 0x00, 0x00, 0x00, b'i', 0x00, 0x00, 0x00];
    assert_eq!(
        decoder::decode_bytes_with_override(&little, Some("utf-32le")).unwrap(),
        "hi"
    );
    assert_eq!(
        decoder::decode_bytes_with_override(&little, Some("utf-32")).unwrap(),
        "hi"
    );
}

#[test]
fn decode_with_override_utf32_detects_little_without_bom() {
    let bytes = [0x2D, 0x4E, 0x00, 0x00, 0x87, 0x65, 0x00, 0x00];
    assert_eq!(
        decoder::decode_bytes_with_override(&bytes, Some("utf-32")).unwrap(),
        "中文"
    );
}

#[test]
fn decode_with_override_utf32_missing_hint_errors() {
    let err =
        decoder::decode_bytes_with_override(&[0x01, 0x02, 0x03, 0x04], Some("utf-32")).unwrap_err();
    assert!(err.contains("utf-32"), "unexpected error: {err}");
}

#[test]
fn decode_with_override_utf8_alias() {
    assert_eq!(
        decoder::decode_bytes_with_override(b"hi", Some("utf-8")).unwrap(),
        "hi"
    );
}

#[test]
fn decode_with_override_utf16be_explicit() {
    let data = [0x00, b'h', 0x00, b'i'];
    assert_eq!(
        decoder::decode_bytes_with_override(&data, Some("utf-16be")).unwrap(),
        "hi"
    );
}

#[test]
fn decode_with_override_utf32be_explicit() {
    let data = [0x00, 0x00, 0x00, b'h', 0x00, 0x00, 0x00, b'i'];
    assert_eq!(
        decoder::decode_bytes_with_override(&data, Some("utf-32be")).unwrap(),
        "hi"
    );
}

#[test]
fn decode_with_override_latin1_variants() {
    let bytes = [0xA1, 0x21];
    assert_eq!(
        decoder::decode_bytes_with_override(&bytes, Some("latin-1")).unwrap(),
        "¡!"
    );
    let alias = [0xA1];
    assert_eq!(
        decoder::decode_bytes_with_override(&alias, Some("ISO_8859_1")).unwrap(),
        "¡"
    );
    assert_eq!(
        decoder::decode_bytes_with_override(&alias, Some("latin1")).unwrap(),
        "¡"
    );
}

#[test]
fn decode_with_override_utf8_sig() {
    let data = [0xEF, 0xBB, 0xBF, b'h'];
    assert_eq!(
        decoder::decode_bytes_with_override(&data, Some("utf8-sig")).unwrap(),
        "h"
    );
}

#[test]
fn decode_with_override_utf16_only_bom() {
    let data = [0xFF, 0xFE];
    assert_eq!(
        decoder::decode_bytes_with_override(&data, Some("utf-16")).unwrap(),
        ""
    );
}

#[test]
fn decode_with_override_utf32_only_bom() {
    let data = [0xFF, 0xFE, 0x00, 0x00];
    assert_eq!(
        decoder::decode_bytes_with_override(&data, Some("utf-32")).unwrap(),
        ""
    );
}

#[test]
fn decode_with_override_utf16_empty_input() {
    assert_eq!(
        decoder::decode_bytes_with_override(&[], Some("utf-16le")).unwrap(),
        ""
    );
}

#[test]
fn decode_with_override_utf32_empty_input() {
    assert_eq!(
        decoder::decode_bytes_with_override(&[], Some("utf-32le")).unwrap(),
        ""
    );
}

#[test]
fn decode_with_override_unknown_errors() {
    let err = decoder::decode_bytes_with_override(b"data", Some("unsupported")).unwrap_err();
    assert!(err.contains("unsupported label"));
}

#[test]
fn decode_with_override_empty_label_errors() {
    let err = decoder::decode_bytes_with_override(b"data", Some("  ")).unwrap_err();
    assert!(err.contains("cannot be empty"));
}

#[test]
fn decode_with_custom_encoding() {
    let bytes = b"hello";
    assert_eq!(
        decoder::decode_bytes_with_override(bytes, Some("koi8-r")).unwrap(),
        "hello"
    );
    assert_eq!(
        decoder::decode_bytes_with_override(bytes, Some("KOI8_R")).unwrap(),
        "hello"
    );
}

#[test]
fn decode_with_custom_encoding_errors() {
    let err = decoder::decode_bytes_with_override(&[0x82], Some("shift_jis")).unwrap_err();
    assert!(err.contains("decode error"));
}

#[test]
fn read_file_decodes_utf8() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("sample.yml");
    std::fs::write(&path, "key: value\n").unwrap();
    let content = decoder::read_file(&path).unwrap();
    assert!(content.contains("key: value"));
}

#[test]
fn read_file_invalid_utf8_errors() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("invalid.yml");
    std::fs::write(&path, [0xFF]).unwrap();
    let err = decoder::read_file(&path).unwrap_err();
    assert!(err.contains("invalid utf-8 data"));
}

#[test]
fn read_file_missing_file_errors() {
    let err = decoder::read_file(Path::new("no_such_decoder_file.yml")).unwrap_err();
    assert!(err.contains("failed to read"));
}
