use std::char;
use std::env;
use std::path::Path;

use encoding_rs::Encoding;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Endian {
    Big,
    Little,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FileEncoding {
    Utf8,
    Utf8WithBom,
    Utf16 { endian: Endian, skip_bom: bool },
    Utf32 { endian: Endian, skip_bom: bool },
    Latin1,
    Custom(&'static Encoding),
}

impl FileEncoding {
    #[must_use]
    fn encode(&self, content: &str) -> Vec<u8> {
        match *self {
            Self::Utf8 => content.as_bytes().to_vec(),
            Self::Utf8WithBom => {
                let mut out = Vec::with_capacity(content.len() + 3);
                out.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
                out.extend_from_slice(content.as_bytes());
                out
            }
            Self::Utf16 { endian, skip_bom } => {
                let mut out = Vec::with_capacity(content.len() * 2 + 2);
                if skip_bom {
                    match endian {
                        Endian::Big => out.extend_from_slice(&[0xFE, 0xFF]),
                        Endian::Little => out.extend_from_slice(&[0xFF, 0xFE]),
                    }
                }
                for ch in content.encode_utf16() {
                    match endian {
                        Endian::Big => out.extend_from_slice(&ch.to_be_bytes()),
                        Endian::Little => out.extend_from_slice(&ch.to_le_bytes()),
                    }
                }
                out
            }
            Self::Utf32 { endian, skip_bom } => {
                let mut out = Vec::with_capacity(content.len() * 4 + 4);
                if skip_bom {
                    match endian {
                        Endian::Big => out.extend_from_slice(&[0x00, 0x00, 0xFE, 0xFF]),
                        Endian::Little => {
                            out.extend_from_slice(&[0xFF, 0xFE, 0x00, 0x00]);
                        }
                    }
                }
                for ch in content.chars() {
                    let val = ch as u32;
                    match endian {
                        Endian::Big => out.extend_from_slice(&val.to_be_bytes()),
                        Endian::Little => out.extend_from_slice(&val.to_le_bytes()),
                    }
                }
                out
            }
            Self::Latin1 => content.chars().map(|ch| ch as u8).collect(),
            Self::Custom(encoding) => {
                let (bytes, _encoding_used, _had_errors) = encoding.encode(content);
                bytes.into_owned()
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DecodedFile {
    content: String,
    encoding: FileEncoding,
}

impl DecodedFile {
    #[must_use]
    pub(crate) fn content(&self) -> &str {
        &self.content
    }

    #[must_use]
    pub(crate) fn into_content(self) -> String {
        self.content
    }

    pub(crate) fn write(&self, path: &Path, content: &str) -> Result<(), String> {
        std::fs::write(path, self.encoding.encode(content)).map_err(|err| {
            format!("failed to write fixed file {}: {err}", path.display())
        })
    }
}

fn normalize_label(label: &str) -> String {
    label.trim().to_ascii_lowercase().replace('_', "-")
}

fn decode_error(kind: &str, detail: impl Into<String>) -> String {
    let detail = detail.into();
    format!("invalid {kind}: {detail}")
}

fn parse_override(bytes: &[u8], label: &str) -> Result<FileEncoding, String> {
    let normalized = normalize_label(label);
    if normalized.is_empty() {
        return Err(decode_error(
            "encoding",
            "YAMLLINT_FILE_ENCODING cannot be empty",
        ));
    }
    match normalized.as_str() {
        "utf-8" => Ok(FileEncoding::Utf8),
        "utf-8-sig" | "utf8-sig" => Ok(FileEncoding::Utf8WithBom),
        "utf-16" => Ok(FileEncoding::Utf16 {
            endian: detect_utf16_endian(bytes).unwrap_or(Endian::Little),
            skip_bom: bytes.starts_with(&[0xFE, 0xFF])
                || bytes.starts_with(&[0xFF, 0xFE]),
        }),
        "utf-16le" | "utf-16-le" | "utf16le" => Ok(FileEncoding::Utf16 {
            endian: Endian::Little,
            skip_bom: false,
        }),
        "utf-16be" | "utf-16-be" | "utf16be" => Ok(FileEncoding::Utf16 {
            endian: Endian::Big,
            skip_bom: false,
        }),
        "utf-32" => Ok(FileEncoding::Utf32 {
            endian: detect_utf32_endian(bytes).unwrap_or(Endian::Little),
            skip_bom: bytes.starts_with(&[0x00, 0x00, 0xFE, 0xFF])
                || bytes.starts_with(&[0xFF, 0xFE, 0x00, 0x00]),
        }),
        "utf-32le" | "utf-32-le" | "utf32le" => Ok(FileEncoding::Utf32 {
            endian: Endian::Little,
            skip_bom: false,
        }),
        "utf-32be" | "utf-32-be" | "utf32be" => Ok(FileEncoding::Utf32 {
            endian: Endian::Big,
            skip_bom: false,
        }),
        "latin-1" | "latin1" | "iso-8859-1" | "iso8859-1" => Ok(FileEncoding::Latin1),
        other => Encoding::for_label(other.as_bytes())
            .map(FileEncoding::Custom)
            .ok_or_else(|| {
                decode_error("encoding", format!("unsupported label '{label}'"))
            }),
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

fn detect_encoding(bytes: &[u8]) -> Result<FileEncoding, String> {
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
) -> Result<FileEncoding, String> {
    if let Some(label) = override_label {
        return parse_override(bytes, label);
    }

    if bytes.starts_with(&[0x00, 0x00, 0xFE, 0xFF]) {
        return Ok(FileEncoding::Utf32 {
            endian: Endian::Big,
            skip_bom: true,
        });
    }
    if bytes.len() >= 4 && bytes[0..3] == [0x00, 0x00, 0x00] {
        return Ok(FileEncoding::Utf32 {
            endian: Endian::Big,
            skip_bom: false,
        });
    }
    if bytes.starts_with(&[0xFF, 0xFE, 0x00, 0x00]) {
        return Ok(FileEncoding::Utf32 {
            endian: Endian::Little,
            skip_bom: true,
        });
    }
    if bytes.len() >= 4 && bytes[1..4] == [0x00, 0x00, 0x00] {
        return Ok(FileEncoding::Utf32 {
            endian: Endian::Little,
            skip_bom: false,
        });
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return Ok(FileEncoding::Utf16 {
            endian: Endian::Big,
            skip_bom: true,
        });
    }
    if bytes.len() >= 2 && bytes[0] == 0x00 {
        return Ok(FileEncoding::Utf16 {
            endian: Endian::Big,
            skip_bom: false,
        });
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return Ok(FileEncoding::Utf16 {
            endian: Endian::Little,
            skip_bom: true,
        });
    }
    if bytes.len() >= 2 && bytes[1] == 0x00 {
        return Ok(FileEncoding::Utf16 {
            endian: Endian::Little,
            skip_bom: false,
        });
    }
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return Ok(FileEncoding::Utf8WithBom);
    }
    Ok(FileEncoding::Utf8)
}

fn decode_utf8(bytes: &[u8]) -> Result<String, String> {
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|err| decode_error("utf-8 data", err.to_string()))
}

fn decode_utf8_bom(bytes: &[u8]) -> Result<String, String> {
    let sliced = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    decode_utf8(sliced)
}

fn decode_utf16(
    bytes: &[u8],
    endian: Endian,
    skip_bom: bool,
) -> Result<String, String> {
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
    String::from_utf16(&units)
        .map_err(|err| decode_error("utf-16 data", err.to_string()))
}

fn decode_utf32(
    bytes: &[u8],
    endian: Endian,
    skip_bom: bool,
) -> Result<String, String> {
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
            Endian::Little => {
                u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])
            }
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

fn decode_with_custom(
    bytes: &[u8],
    encoding: &'static Encoding,
) -> Result<String, String> {
    let (text, _encoding_used, had_errors) = encoding.decode(bytes);
    if had_errors {
        return Err(decode_error(
            encoding.name(),
            "decode error with replacement required",
        ));
    }
    Ok(text.into_owned())
}

fn decode_with_kind(bytes: &[u8], encoding: FileEncoding) -> Result<String, String> {
    match encoding {
        FileEncoding::Utf8 => decode_utf8(bytes),
        FileEncoding::Utf8WithBom => decode_utf8_bom(bytes),
        FileEncoding::Utf16 { endian, skip_bom } => {
            decode_utf16(bytes, endian, skip_bom)
        }
        FileEncoding::Utf32 { endian, skip_bom } => {
            decode_utf32(bytes, endian, skip_bom)
        }
        FileEncoding::Latin1 => Ok(decode_latin1(bytes)),
        FileEncoding::Custom(enc) => decode_with_custom(bytes, enc),
    }
}

fn decode_bytes_with_encoding(bytes: &[u8]) -> Result<(String, FileEncoding), String> {
    let encoding = detect_encoding(bytes)?;
    decode_with_kind(bytes, encoding).map(|s| (s, encoding))
}

/// Decode raw bytes using yamllint-compatible encoding detection.
///
/// # Errors
/// Returns an error string describing why decoding failed.
pub fn decode_bytes(bytes: &[u8]) -> Result<String, String> {
    decode_bytes_with_encoding(bytes).map(|(content, _)| content)
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
    read_file_lossless(path).map(DecodedFile::into_content)
}

/// Read a file from disk and retain its detected encoding for write-back.
///
/// # Errors
/// Returns an error string when the file cannot be read or decoded.
pub(crate) fn read_file_lossless(path: &Path) -> Result<DecodedFile, String> {
    let data = std::fs::read(path)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    decode_bytes_with_encoding(&data)
        .map(|(content, encoding)| DecodedFile { content, encoding })
        .map_err(|err| format!("failed to read {}: {err}", path.display()))
}
