//! Shared interface faces. System fonts are read on the running machine, never
//! copied into the application. Document/cell font families remain editor-owned.

use std::sync::{Arc, OnceLock};

use eframe::egui::{self, FontData, FontDefinitions, FontFamily};

const REGULAR: &str = "weave-ui-regular";
const MEDIUM: &str = "weave-ui-medium";
const SEMIBOLD: &str = "weave-ui-semibold";
const CJK: &str = "weave-ui-cjk";
const CJK_MEDIUM: &str = "weave-ui-cjk-medium";
const FALLBACK: &str = "weave-ui-cjk-fallback";

#[derive(Clone)]
pub struct UiFonts {
    faces: Vec<(&'static str, Arc<FontData>)>,
}

impl UiFonts {
    /// The system lookup and file reads happen once per process, including tests
    /// that create several app instances. Clones share the immutable font bytes.
    pub fn load() -> Self {
        static FONTS: OnceLock<UiFonts> = OnceLock::new();
        FONTS.get_or_init(Self::read).clone()
    }

    fn read() -> Self {
        let latin = [
            (
                REGULAR,
                400.0,
                weave_font_assets::font!("wordcraft/Inter-Regular.ttf").as_slice(),
            ),
            (
                MEDIUM,
                500.0,
                weave_font_assets::font!("wordcraft/Inter-Medium.ttf").as_slice(),
            ),
            (
                SEMIBOLD,
                600.0,
                weave_font_assets::font!("wordcraft/Inter-SemiBold.ttf").as_slice(),
            ),
        ];
        let system_latin = system_latin();
        let mut faces: Vec<_> = latin
            .into_iter()
            .map(|(name, weight, fallback)| {
                let mut data = system_latin
                    .clone()
                    .unwrap_or_else(|| FontData::from_static(fallback));
                if system_latin.is_some() {
                    // SF Pro is variable; use text optical sizing for ribbon labels.
                    data.tweak.coords = egui::epaint::text::VariationCoords::new([
                        (b"wght", weight),
                        (b"opsz", 13.0),
                    ]);
                }
                (name, Arc::new(data))
            })
            .collect();
        if let Some((regular, medium)) = system_cjk() {
            faces.push((CJK, Arc::new(on_baseline(regular))));
            faces.push((CJK_MEDIUM, Arc::new(on_baseline(medium))));
        }
        faces.push((
            FALLBACK,
            Arc::new(on_baseline(FontData::from_static(
                weave_font_assets::font!("craft/fonts/noto-sans-cjk-sc/NotoSansCJKsc-Regular.otf"),
            ))),
        ));
        Self { faces }
    }

    /// Merge only the six interface families used by the shell and its editors.
    /// In particular, Grid's cell families and all dynamically named document
    /// faces must not inherit this policy.
    pub fn apply(&self, fonts: &mut FontDefinitions) {
        for (name, data) in &self.faces {
            fonts.font_data.insert((*name).into(), data.clone());
        }
        for (family, primary, cjk) in [
            (FontFamily::Proportional, REGULAR, CJK),
            (FontFamily::Name("medium".into()), MEDIUM, CJK_MEDIUM),
            (FontFamily::Name("semibold".into()), SEMIBOLD, CJK_MEDIUM),
            (FontFamily::Name("bold".into()), SEMIBOLD, CJK_MEDIUM),
            (FontFamily::Name("ui".into()), REGULAR, CJK),
            (FontFamily::Name("ui-bold".into()), SEMIBOLD, CJK_MEDIUM),
        ] {
            // Missing named UI families belong to a different editor. Do not
            // create them: their presence is part of that editor's readiness gate.
            let Some(chain) = fonts.families.get_mut(&family) else {
                continue;
            };
            chain.retain(|name| !name.starts_with("weave-ui-") && name != "weave-chinese");
            let mut prefix = vec![primary.to_owned()];
            if fonts.font_data.contains_key(cjk) {
                prefix.push(cjk.to_owned());
            }
            prefix.push(FALLBACK.to_owned());
            chain.splice(0..0, prefix);
        }
    }

    /// Call after editor font preparation, before laying out any shell text.
    /// One atomic font-set update preserves document faces and avoids submitting
    /// a partial atlas while changing tabs or interface language.
    pub fn install(&self, ctx: &egui::Context) {
        let next = ctx.fonts(|fonts| {
            let definitions = fonts.definitions();
            // Most frames already have the host faces. Avoid cloning hundreds
            // of named document families just to discover that nothing changed.
            let installed = [
                (FontFamily::Proportional, REGULAR),
                (FontFamily::Name("medium".into()), MEDIUM),
                (FontFamily::Name("semibold".into()), SEMIBOLD),
                (FontFamily::Name("bold".into()), SEMIBOLD),
                (FontFamily::Name("ui".into()), REGULAR),
                (FontFamily::Name("ui-bold".into()), SEMIBOLD),
            ]
            .into_iter()
            .all(|(family, primary)| {
                definitions
                    .families
                    .get(&family)
                    .is_none_or(|chain| chain.first().is_some_and(|name| name == primary))
            });
            if installed {
                return None;
            }
            let mut next = definitions.clone();
            self.apply(&mut next);
            (next != *definitions).then_some(next)
        });
        if let Some(next) = next {
            ctx.set_fonts(next);
            ctx.request_repaint();
        }
    }
}

fn on_baseline(mut data: FontData) -> FontData {
    data.tweak.set_baseline_fallback(true);
    data
}

#[cfg(target_os = "macos")]
fn system_latin() -> Option<FontData> {
    use skrifa::MetadataProvider;
    let bytes = std::fs::read("/System/Library/Fonts/SFNS.ttf").ok()?;
    let font = skrifa::FontRef::from_index(&bytes, 0).ok()?;
    font.charmap()
        .has_map()
        .then(|| FontData::from_owned(bytes))
}

#[cfg(not(target_os = "macos"))]
fn system_latin() -> Option<FontData> {
    None
}

#[cfg(target_os = "macos")]
fn system_cjk() -> Option<(FontData, FontData)> {
    use skrifa::{MetadataProvider, raw::FileRef, string::StringId};
    use std::path::PathBuf;
    let mut paths = vec![PathBuf::from("/System/Library/Fonts/PingFang.ttc")];
    // Recent macOS versions put PingFang in a versioned system asset. Search
    // just font asset directories, with no dependency on an installed office app.
    if let Ok(assets) = std::fs::read_dir("/System/Library/AssetsV2") {
        for asset in assets.flatten().filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("com_apple_MobileAsset_Font")
        }) {
            if let Ok(versions) = std::fs::read_dir(asset.path()) {
                paths.extend(
                    versions
                        .flatten()
                        .map(|entry| entry.path().join("AssetData/PingFang.ttc")),
                );
            }
        }
    }
    paths.sort();
    for path in paths {
        // Bound memory for corrupt or unexpected system assets.
        if !std::fs::metadata(&path).is_ok_and(|m| m.len() <= 128 << 20) {
            continue;
        }
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let count = match FileRef::new(&bytes) {
            Ok(FileRef::Font(_)) => 1,
            Ok(FileRef::Collection(collection)) => collection.len(),
            Err(_) => continue,
        };
        let face = |name: &str| {
            (0..count).find(|index| {
                skrifa::FontRef::from_index(&bytes, *index).is_ok_and(|font| {
                    font.charmap().has_map()
                        && font
                            .localized_strings(StringId::POSTSCRIPT_NAME)
                            .any(|s| s.to_string() == name)
                })
            })
        };
        let Some(regular) = face("PingFangSC-Regular") else {
            continue;
        };
        let medium = face("PingFangSC-Medium").unwrap_or(regular);
        let blob: egui::epaint::text::Blob = Arc::new(bytes);
        return Some((
            FontData::from_blob(blob.clone(), regular),
            FontData::from_blob(blob, medium),
        ));
    }
    None
}

#[cfg(not(target_os = "macos"))]
fn system_cjk() -> Option<(FontData, FontData)> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_ui_faces_preserve_document_families_and_fit_compact_controls() {
        let fonts = UiFonts::load();
        let mut definitions = pdfcraft_ui_egui::theme::font_definitions_for(true);
        let pdf_content: Vec<_> = [
            pdfcraft_ui_egui::theme::EDITOR_REGULAR,
            pdfcraft_ui_egui::theme::EDITOR_SEMIBOLD,
        ]
        .map(|name| {
            let family = FontFamily::Name(name.into());
            (family.clone(), definitions.families[&family].clone())
        })
        .into();
        let document_family = FontFamily::Name("document-Arial-bold".into());
        let original = definitions.families[&FontFamily::Proportional].clone();
        definitions
            .families
            .insert(document_family.clone(), original.clone());
        definitions
            .families
            .insert(FontFamily::Name("cell".into()), original.clone());
        fonts.apply(&mut definitions);
        for (family, chain) in pdf_content {
            assert_eq!(
                definitions.families[&family], chain,
                "PDF page content must keep its original font chain"
            );
        }
        assert_eq!(definitions.families[&document_family], original);
        assert_eq!(
            definitions.families[&FontFamily::Name("cell".into())],
            original
        );
        let applied = definitions.clone();
        fonts.apply(&mut definitions);
        assert_eq!(definitions, applied, "font merging must be idempotent");

        let ctx = egui::Context::default();
        ctx.set_fonts(definitions);
        ctx.run_ui(Default::default(), |ui| {
            ui.fonts_mut(|f| {
                let id = egui::FontId::proportional(13.0);
                assert!(f.has_glyphs(&id, "文件 A1 中文 English 123"));
                let label =
                    f.layout_no_wrap("文件 A1 中文 English 123".into(), id, egui::Color32::BLACK);
                assert!(
                    label.size().y <= 20.0,
                    "13 pt mixed UI label must fit a 24 pt control: {:?}",
                    label.size()
                );
            });
        })
        .drop_without_applying_deltas();
    }
}
