//! How a text file is stored on disk: its encoding, byte-order mark and line endings.
//!
//! The editor works on a `String` with `\n` line endings. Saving writes the text back in the
//! form it was read in, so opening and saving a file without editing it changes no bytes.

use encoding_rs::Encoding;
use std::path::Path;

/// Files larger than this are refused: the editor keeps the whole text in memory and lays it
/// out at once.
pub const MAX_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineEnding {
    Lf,
    CrLf,
}

impl LineEnding {
    pub fn label(self) -> &'static str {
        match self {
            Self::Lf => "LF",
            Self::CrLf => "CRLF",
        }
    }
}

/// The text encodings the editor reads and writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextEncoding {
    Utf8,
    Utf16Le,
    Utf16Be,
    /// Any single- or multi-byte legacy encoding encoding_rs can write (GBK, Shift_JIS, ...).
    Legacy(&'static Encoding),
}

impl TextEncoding {
    pub fn label(self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::Utf16Le => "UTF-16 LE",
            Self::Utf16Be => "UTF-16 BE",
            Self::Legacy(encoding) => encoding.name(),
        }
    }
}

/// The on-disk form of a text file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Format {
    pub encoding: TextEncoding,
    pub bom: bool,
    pub line_ending: LineEnding,
}

impl Default for Format {
    fn default() -> Self {
        Self {
            encoding: TextEncoding::Utf8,
            bom: false,
            line_ending: if cfg!(windows) {
                LineEnding::CrLf
            } else {
                LineEnding::Lf
            },
        }
    }
}

/// Decode `bytes` into editor text (`\n` line endings) and the format to write it back in.
pub fn decode(bytes: &[u8]) -> Result<(String, Format), String> {
    if bytes.len() > MAX_BYTES {
        return Err(format!(
            "文件太大({} MB),文本编辑器最多打开 {} MB",
            bytes.len() / (1024 * 1024),
            MAX_BYTES / (1024 * 1024)
        ));
    }
    let (encoding, bom, body) = if let Some(rest) = bytes.strip_prefix(b"\xEF\xBB\xBF") {
        (TextEncoding::Utf8, true, rest)
    } else if let Some(rest) = bytes.strip_prefix(b"\xFF\xFE") {
        (TextEncoding::Utf16Le, true, rest)
    } else if let Some(rest) = bytes.strip_prefix(b"\xFE\xFF") {
        (TextEncoding::Utf16Be, true, rest)
    } else {
        (sniff(bytes)?, false, bytes)
    };
    let text = match encoding {
        TextEncoding::Utf8 => String::from_utf8_lossy(body).into_owned(),
        TextEncoding::Utf16Le => encoding_rs::UTF_16LE
            .decode_without_bom_handling(body)
            .0
            .into_owned(),
        TextEncoding::Utf16Be => encoding_rs::UTF_16BE
            .decode_without_bom_handling(body)
            .0
            .into_owned(),
        TextEncoding::Legacy(encoding) => encoding.decode_without_bom_handling(body).0.into_owned(),
    };
    let crlf = text.matches("\r\n").count();
    let lf = text.matches('\n').count();
    let line_ending = if lf == 0 {
        Format::default().line_ending
    } else if crlf * 2 > lf {
        LineEnding::CrLf
    } else {
        LineEnding::Lf
    };
    let text = if crlf > 0 {
        text.replace("\r\n", "\n")
    } else {
        text
    };
    Ok((
        text,
        Format {
            encoding,
            bom,
            line_ending,
        },
    ))
}

/// Whether `bytes` (the start of a file) look like text rather than a binary format.
pub fn looks_like_text(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(8192)];
    if head.starts_with(b"\xFF\xFE") || head.starts_with(b"\xFE\xFF") {
        return true;
    }
    if head.contains(&0) {
        return false;
    }
    // Control characters other than whitespace, backspace, form feed and escape mean binary.
    let control = head
        .iter()
        .filter(|&&b| b < 0x20 && !matches!(b, b'\t' | b'\n' | b'\r' | 0x08 | 0x0C | 0x1B))
        .count();
    control * 100 <= head.len()
}

fn sniff(bytes: &[u8]) -> Result<TextEncoding, String> {
    if !looks_like_text(bytes) {
        return Err("这不是文本文件,无法用文本编辑器打开".into());
    }
    if std::str::from_utf8(bytes).is_ok() {
        return Ok(TextEncoding::Utf8);
    }
    let mut detector = chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Deny);
    detector.feed(bytes, true);
    let guess = detector.guess(None, chardetng::Utf8Detection::Allow);
    Ok(if guess == encoding_rs::UTF_8 {
        TextEncoding::Utf8
    } else {
        TextEncoding::Legacy(guess)
    })
}

/// Encode editor text in `format`. Characters the encoding cannot represent are an error, so
/// saving never silently replaces them.
pub fn encode(text: &str, format: Format) -> Result<Vec<u8>, String> {
    let owned;
    let text = if format.line_ending == LineEnding::CrLf {
        owned = text.replace('\n', "\r\n");
        owned.as_str()
    } else {
        text
    };
    let mut out = Vec::with_capacity(text.len() + 3);
    match format.encoding {
        TextEncoding::Utf8 => {
            if format.bom {
                out.extend_from_slice(b"\xEF\xBB\xBF");
            }
            out.extend_from_slice(text.as_bytes());
        }
        TextEncoding::Utf16Le | TextEncoding::Utf16Be => {
            let little = format.encoding == TextEncoding::Utf16Le;
            if format.bom {
                out.extend_from_slice(if little { b"\xFF\xFE" } else { b"\xFE\xFF" });
            }
            for unit in text.encode_utf16() {
                out.extend_from_slice(&if little {
                    unit.to_le_bytes()
                } else {
                    unit.to_be_bytes()
                });
            }
        }
        TextEncoding::Legacy(encoding) => {
            let (bytes, _, unmappable) = encoding.encode(text);
            if unmappable {
                return Err(format!(
                    "文本中有 {} 编码无法表示的字符,请改用 UTF-8 保存",
                    encoding.name()
                ));
            }
            out.extend_from_slice(&bytes);
        }
    }
    Ok(out)
}

/// Write `bytes` to `path` through a temporary file in the same folder, so a failed save
/// never leaves a half-written file behind.
pub fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .map_err(|error| format!("无法写入 {}:{error}", path.display()))?;
    temporary
        .write_all(bytes)
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|error| format!("无法写入 {}:{error}", path.display()))?;
    if let Ok(metadata) = std::fs::metadata(path) {
        let _ = std::fs::set_permissions(temporary.path(), metadata.permissions());
    }
    temporary
        .persist(path)
        .map_err(|error| format!("无法保存 {}:{}", path.display(), error.error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_every_supported_form_byte_for_byte() {
        let samples: Vec<Vec<u8>> = vec![
            b"plain\nlines\n".to_vec(),
            b"windows\r\nlines\r\n".to_vec(),
            b"\xEF\xBB\xBFbom\n".to_vec(),
            [
                b"\xFF\xFE".as_slice(),
                &"中文\n"
                    .encode_utf16()
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<_>>(),
            ]
            .concat(),
            encoding_rs::GBK
                .encode("这是一段比较长的中文文本,用来检测编码。\n")
                .0
                .into_owned(),
            Vec::new(),
        ];
        for bytes in samples {
            let (text, format) = decode(&bytes).unwrap();
            assert!(!text.contains('\r'));
            assert_eq!(encode(&text, format).unwrap(), bytes, "{format:?}");
        }
    }

    #[test]
    fn detects_legacy_chinese_and_refuses_binary() {
        let gbk = encoding_rs::GBK.encode("你好,世界。这是一个测试文件。\n").0;
        let (text, format) = decode(&gbk).unwrap();
        assert!(text.starts_with("你好"));
        assert!(matches!(format.encoding, TextEncoding::Legacy(_)));
        assert!(decode(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR").is_err());
        assert!(!looks_like_text(b"PK\x03\x04\0\0"));
        assert!(looks_like_text("# 标题\n".as_bytes()));
    }

    #[test]
    fn legacy_encoding_refuses_unmappable_text() {
        let format = Format {
            encoding: TextEncoding::Legacy(encoding_rs::WINDOWS_1252),
            bom: false,
            line_ending: LineEnding::Lf,
        };
        assert!(encode("中文", format).is_err());
        assert_eq!(encode("abc\n", format).unwrap(), b"abc\n");
    }
}
