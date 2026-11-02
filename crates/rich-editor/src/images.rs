//! Pictures a document shows: `data:` URIs and files next to it. Web addresses are not
//! fetched (the editor works offline); they show as placeholders with their alt text.

use egui::{ColorImage, TextureHandle, Vec2, vec2};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Pictures wider than this are scaled down for display.
const MAX_TEXTURE: u32 = 2048;

enum Entry {
    Missing,
    Decoded {
        size: Vec2,
        image: Option<ColorImage>,
        texture: Option<TextureHandle>,
    },
}

#[derive(Default)]
pub struct Images {
    base: Option<PathBuf>,
    entries: HashMap<String, Entry>,
}

impl Images {
    /// Relative paths resolve against `base` (the document's folder).
    pub fn set_base(&mut self, base: Option<PathBuf>) {
        if self.base != base {
            self.base = base;
            self.entries.clear();
        }
    }

    fn load(&self, src: &str) -> Entry {
        let Some(bytes) = self.bytes(src) else {
            return Entry::Missing;
        };
        let Ok(decoded) = image::load_from_memory(&bytes) else {
            return Entry::Missing;
        };
        let size = vec2(decoded.width() as f32, decoded.height() as f32);
        let decoded = if decoded.width() > MAX_TEXTURE {
            decoded.resize(MAX_TEXTURE, u32::MAX, image::imageops::FilterType::Triangle)
        } else {
            decoded
        };
        let rgba = decoded.to_rgba8();
        let image = ColorImage::from_rgba_unmultiplied(
            [rgba.width() as usize, rgba.height() as usize],
            rgba.as_raw(),
        );
        Entry::Decoded {
            size,
            image: Some(image),
            texture: None,
        }
    }

    /// The bytes a `src` names, if they are local.
    pub fn bytes(&self, src: &str) -> Option<Vec<u8>> {
        let src = src.trim();
        let lower = src.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("data:") {
            let comma = rest.find(',')?;
            let meta = &rest[..comma];
            let payload = &src[5 + comma + 1..];
            return if meta.ends_with(";base64") {
                base64_decode(payload)
            } else {
                Some(percent_decode(payload).into_bytes())
            };
        }
        if lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("//")
        {
            return None;
        }
        let path = src.strip_prefix("file://").unwrap_or(src);
        let path = percent_decode(path.split(['?', '#']).next().unwrap_or(path));
        let path = Path::new(&path);
        let path = match (&self.base, path.is_absolute()) {
            (Some(base), false) => base.join(path),
            _ => path.to_path_buf(),
        };
        std::fs::read(path).ok()
    }

    /// The texture for `src`, uploading it on first use.
    pub fn texture(&mut self, ctx: &egui::Context, src: &str) -> Option<&TextureHandle> {
        self.size(src)?;
        let Some(Entry::Decoded { image, texture, .. }) = self.entries.get_mut(src) else {
            return None;
        };
        if texture.is_none() {
            let image = image.take()?;
            *texture = Some(ctx.load_texture(
                format!("weave-rich-image-{src:.64}"),
                image,
                egui::TextureOptions::LINEAR,
            ));
        }
        texture.as_ref()
    }

    pub fn size(&mut self, src: &str) -> Option<Vec2> {
        if !self.entries.contains_key(src) {
            let entry = self.load(src);
            self.entries.insert(src.to_string(), entry);
        }
        match self.entries.get(src)? {
            Entry::Decoded { size, .. } => Some(*size),
            Entry::Missing => None,
        }
    }
}

impl crate::layout::ImageSizes for Images {
    fn image_size(&mut self, src: &str) -> Option<Vec2> {
        self.size(src)
    }
}

pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let mut buffer = 0u32;
    let mut bits = 0;
    for c in text.bytes() {
        let value = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            b' ' | b'\n' | b'\r' | b'\t' => continue,
            _ => return None,
        } as u32;
        buffer = buffer << 6 | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    Some(out)
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len() + 1
            && let Some(hex) = text.get(i + 1..i + 3)
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64() {
        assert_eq!(base64_decode("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(base64_decode("aGVs\nbG8").unwrap(), b"hello");
        assert_eq!(base64_decode("a$"), None);
    }

    #[test]
    fn data_uris_and_files() {
        let images = Images::default();
        assert_eq!(images.bytes("data:text/plain,a%20b").unwrap(), b"a b");
        assert_eq!(images.bytes("data:image/png;base64,aGk=").unwrap(), b"hi");
        assert_eq!(images.bytes("https://example.com/a.png"), None);
        let dir = tempfile_dir();
        std::fs::write(dir.join("图 1.png"), b"x").unwrap();
        let mut images = Images::default();
        images.set_base(Some(dir.clone()));
        assert_eq!(images.bytes("图%201.png").unwrap(), b"x");
        assert_eq!(images.bytes("./图 1.png?v=2").unwrap(), b"x");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn decodes_png_size() {
        let mut png = Vec::new();
        image::RgbaImage::new(3, 2)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let mut images = Images::default();
        let src = format!("data:image/png;base64,{}", encode(&png));
        assert_eq!(images.size(&src), Some(vec2(3.0, 2.0)));
        assert_eq!(images.size("missing.png"), None);
    }

    fn tempfile_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("weave-rich-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn encode(bytes: &[u8]) -> String {
        const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let n = (chunk[0] as u32) << 16
                | (*chunk.get(1).unwrap_or(&0) as u32) << 8
                | *chunk.get(2).unwrap_or(&0) as u32;
            out.push(T[(n >> 18) as usize & 63] as char);
            out.push(T[(n >> 12) as usize & 63] as char);
            out.push(if chunk.len() > 1 {
                T[(n >> 6) as usize & 63] as char
            } else {
                '='
            });
            out.push(if chunk.len() > 2 {
                T[n as usize & 63] as char
            } else {
                '='
            });
        }
        out
    }
}
