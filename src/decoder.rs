use std::char;
use std::env;
use std::path::Path;

use encoding_rs::Encoding;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Endian {
    Big,
    Little,
}

#[derive(Clone, Copy)]
enum EncodingKind {
    Utf8,
    Utf8WithBom,
    Utf16 { endian: Endian, skip_bom: bool },
    Utf32 { endian: Endian, skip_bom: bool },
    Latin1,
    Custom(&'static Encoding),
}

fn normalize_label(label: &str) -> String {
    label.trim().to_ascii_lowercase().replace('_', "-")
}

fn decode_error(kind: &str, detail: impl Into<String>) -> String {
    let detail = detail.into();
    format!("invalid {kind}: {detail}")
}

fn parse_override(bytes: &[u8], label: &str) -> Result<EncodingKind, String> {
    let normalized = normalize_label(label);
    if normalized.is_empty() {
        return Err(decode_error(
            "encoding",
            "YAMLLINT_FILE_ENCODING cannot be empty",
        ));
    }
    match normalized.as_str() {
        "utf-8" => Ok(EncodingKind::Utf8),
        "utf-8-sig" | "utf8-sig" => Ok(EncodingKind::Utf8WithBom),
        "utf-16" => Ok(EncodingKind::Utf16 {
            endian: detect_utf16_endian(bytes).unwrap_or(Endian::Little),
            skip_bom: bytes.starts_with(&[0xFE, 0xFF]) || bytes.starts_with(&[0xFF, 0xFE]),
        }),
        "utf-16le" | "utf-16-le" | "utf16le" => Ok(EncodingKind::Utf16 {
            endian: Endian::Little,
            skip_bom: false,
        }),
        "utf-16be" | "utf-16-be" | "utf16be" => Ok(EncodingKind::Utf16 {
            endian: Endian::Big,
            skip_bom: false,
        }),
        "utf-32" => Ok(EncodingKind::Utf32 {
            endian: detect_utf32_endian(bytes).unwrap_or(Endian::Little),
            skip_bom: bytes.starts_with(&[0x00, 0x00, 0xFE, 0xFF])
                || bytes.starts_with(&[0xFF, 0xFE, 0x00, 0x00]),
        }),
        "utf-32le" | "utf-32-le" | "utf32le" => Ok(EncodingKind::Utf32 {
            endian: Endian::Little,
            skip_bom: false,
        }),
        "utf-32be" | "utf-32-be" | "utf32be" => Ok(EncodingKind::Utf32 {
            endian: Endian::Big,
            skip_bom: false,
        }),
        "latin-1" | "latin1" | "iso-8859-1" | "iso8859-1" => Ok(EncodingKind::Latin1),
        other => Encoding::for_label(other.as_bytes())
            .map(EncodingKind::Custom)
            .ok_or_else(|| decode_error("encoding", format!("unsupported label '{label}'"))),
    }
}

fn detect_utf16_endian(bytes: &[u8]) -> Option<Endian> {
    if bytes.starts_with(&[0xFE, 0xFF]) {
        Some(Endian::Big)
    } else if bytes.starts_with(&[0xFF, 0xFE]) {
        Some(Endian::Little)
    } else if bytes.len() >= 2 && bytes[0] == 0x00 {
        Some(Endian::Big)
    } else if bytes.len() >= 2 && bytes[1] == 0x00 {
        Some(Endian::Little)
    } else {
        None
    }
}

fn detect_utf32_endian(bytes: &[u8]) -> Option<Endian> {
    if bytes.starts_with(&[0x00, 0x00, 0xFE, 0xFF]) {
        Some(Endian::Big)
    } else if bytes.starts_with(&[0xFF, 0xFE, 0x00, 0x00]) {
        Some(Endian::Little)
    } else if bytes.len() >= 4 && bytes[0..3] == [0x00, 0x00, 0x00] {
        Some(Endian::Big)
    } else if bytes.len() >= 4 && bytes[1..4] == [0x00, 0x00, 0x00] {
        Some(Endian::Little)
    } else {
        None
    }
}

fn detect_encoding(bytes: &[u8]) -> Result<EncodingKind, String> {
    let override_label = env::var("YAMLLINT_FILE_ENCODING").map_or(None, |value| {
        eprintln!(
            "YAMLLINT_FILE_ENCODING is meant for temporary workarounds. It may be removed in a future version of yamllint."
        );
        Some(value)
    });
    detect_encoding_with_override(bytes, override_label.as_deref())
}

fn detect_encoding_with_override(
    bytes: &[u8],
    override_label: Option<&str>,
) -> Result<EncodingKind, String> {
    if let Some(label) = override_label {
        return parse_override(bytes, label);
    }

    if bytes.starts_with(&[0x00, 0x00, 0xFE, 0xFF]) {
        return Ok(EncodingKind::Utf32 {
            endian: Endian::Big,
            skip_bom: true,
        });
    }
    if bytes.len() >= 4 && bytes[0..3] == [0x00, 0x00, 0x00] {
        return Ok(EncodingKind::Utf32 {
            endian: Endian::Big,
            skip_bom: false,
        });
    }
    if bytes.starts_with(&[0xFF, 0xFE, 0x00, 0x00]) {
        return Ok(EncodingKind::Utf32 {
            endian: Endian::Little,
            skip_bom: true,
        });
    }
    if bytes.len() >= 4 && bytes[1..4] == [0x00, 0x00, 0x00] {
        return Ok(EncodingKind::Utf32 {
            endian: Endian::Little,
            skip_bom: false,
        });
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return Ok(EncodingKind::Utf16 {
            endian: Endian::Big,
            skip_bom: true,
        });
    }
    if bytes.len() >= 2 && bytes[0] == 0x00 {
        return Ok(EncodingKind::Utf16 {
            endian: Endian::Big,
            skip_bom: false,
        });
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return Ok(EncodingKind::Utf16 {
            endian: Endian::Little,
            skip_bom: true,
        });
    }
    if bytes.len() >= 2 && bytes[1] == 0x00 {
        return Ok(EncodingKind::Utf16 {
            endian: Endian::Little,
            skip_bom: false,
        });
    }
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return Ok(EncodingKind::Utf8WithBom);
    }
    Ok(EncodingKind::Utf8)
}

fn decode_utf8(bytes: &[u8]) -> Result<String, String> {
    String::from_utf8(bytes.to_vec()).map_err(|err| decode_error("utf-8 data", err.to_string()))
}

fn decode_utf8_bom(bytes: &[u8]) -> Result<String, String> {
    let sliced = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    decode_utf8(sliced)
}

fn decode_utf16(bytes: &[u8], endian: Endian, skip_bom: bool) -> Result<String, String> {
    if bytes.is_empty() {
        return Ok(String::new());
    }
    let data = if skip_bom {
        bytes.get(2..).unwrap_or(&[])
    } else {
        bytes
    };
    if data.len() % 2 != 0 {
        return Err(decode_error(
            "utf-16 data",
            format!("length {} is not even", data.len()),
        ));
    }
    let mut units: Vec<u16> = Vec::with_capacity(data.len() / 2);
    for chunk in data.chunks_exact(2) {
        let value = match endian {
            Endian::Big => u16::from_be_bytes([chunk[0], chunk[1]]),
            Endian::Little => u16::from_le_bytes([chunk[0], chunk[1]]),
        };
        units.push(value);
    }
    String::from_utf16(&units).map_err(|err| decode_error("utf-16 data", err.to_string()))
}

fn decode_utf32(bytes: &[u8], endian: Endian, skip_bom: bool) -> Result<String, String> {
    if bytes.is_empty() {
        return Ok(String::new());
    }
    let data = if skip_bom {
        bytes.get(4..).unwrap_or(&[])
    } else {
        bytes
    };
    if data.len() % 4 != 0 {
        return Err(decode_error(
            "utf-32 data",
            format!("length {} is not divisible by 4", data.len()),
        ));
    }
    let mut out = String::with_capacity(data.len() / 4);
    for chunk in data.chunks_exact(4) {
        let raw = match endian {
            Endian::Big => u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
            Endian::Little => u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
        };
        match char::from_u32(raw) {
            Some(ch) => out.push(ch),
            None => {
                return Err(decode_error(
                    "utf-32 data",
                    format!("invalid scalar value 0x{raw:08X}"),
                ));
            }
        }
    }
    Ok(out)
}

fn decode_latin1(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| char::from_u32(u32::from(b)).unwrap())
        .collect()
}

fn decode_with_custom(bytes: &[u8], encoding: &'static Encoding) -> Result<String, String> {
    let (text, _encoding_used, had_errors) = encoding.decode(bytes);
    if had_errors {
        return Err(decode_error(
            encoding.name(),
            "decode error with replacement required",
        ));
    }
    Ok(text.into_owned())
}

fn decode_with_kind(bytes: &[u8], encoding: EncodingKind) -> Result<String, String> {
    match encoding {
        EncodingKind::Utf8 => decode_utf8(bytes),
        EncodingKind::Utf8WithBom => decode_utf8_bom(bytes),
        EncodingKind::Utf16 { endian, skip_bom } => decode_utf16(bytes, endian, skip_bom),
        EncodingKind::Utf32 { endian, skip_bom } => decode_utf32(bytes, endian, skip_bom),
        EncodingKind::Latin1 => Ok(decode_latin1(bytes)),
        EncodingKind::Custom(enc) => decode_with_custom(bytes, enc),
    }
}

/// Decode raw bytes using yamllint-compatible encoding detection.
///
/// # Errors
/// Returns an error string describing why decoding failed.
pub fn decode_bytes(bytes: &[u8]) -> Result<String, String> {
    let encoding = detect_encoding(bytes)?;
    decode_with_kind(bytes, encoding)
}

/// Decode bytes using an explicit encoding override, bypassing environment lookups.
///
/// # Errors
/// Returns an error string when the override label is unsupported or decoding fails.
pub fn decode_bytes_with_override(
    bytes: &[u8],
    override_label: Option<&str>,
) -> Result<String, String> {
    let encoding = detect_encoding_with_override(bytes, override_label)?;
    decode_with_kind(bytes, encoding)
}

/// Read a file from disk and decode it using yamllint-compatible detection.
///
/// # Errors
/// Returns an error string when the file cannot be read or decoded.
pub fn read_file(path: &Path) -> Result<String, String> {
    let data =
        std::fs::read(path).map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    decode_bytes(&data).map_err(|err| format!("failed to read {}: {err}", path.display()))
}
