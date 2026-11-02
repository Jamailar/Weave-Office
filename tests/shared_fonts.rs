//! Shared font bytes must remain the same allocation across independent editors.
use eframe::egui::FontDefinitions;

#[test]
fn all_four_editors_borrow_the_same_multilingual_font_bytes() {
    let grid = gridcraft_ui_egui::theme::pinned_font_definitions();
    let mut matches = 0;
    for word in wordcraft_fonts::CRAFT_FONTS {
        let Some(deck) = deckcraft_fonts::CRAFT_FONTS
            .iter()
            .find(|font| font.family == word.family && font.style == word.style)
        else {
            continue;
        };
        let Some(pdf) = pdfcraft_fonts::CRAFT_FONTS
            .iter()
            .find(|font| font.family == word.family && font.style == word.style)
        else {
            continue;
        };
        let suffix = format!("-{}-{}", word.family, word.style);
        let grid = grid
            .font_data
            .iter()
            .find(|(key, _)| key.starts_with("craft-") && key.ends_with(&suffix))
            .unwrap_or_else(|| panic!("missing Grid face: {suffix}"));
        for (name, bytes) in [
            ("Deck", deck.bytes),
            ("PDF", pdf.bytes),
            ("Grid", grid.1.bytes()),
        ] {
            assert!(
                std::ptr::eq(word.bytes, bytes),
                "{name} embedded a separate copy of {} {}",
                word.family,
                word.style
            );
        }
        assert!(
            skrifa::FontRef::new(word.bytes).is_ok(),
            "{} must still parse",
            word.family
        );
        matches += 1;
    }
    assert!(
        matches >= 10,
        "the multilingual fallback set must remain present"
    );
}

#[test]
fn shell_and_document_fallbacks_share_storage_without_changing_faces() {
    let mut definitions = FontDefinitions::default();
    weave_office::ui_fonts::UiFonts::load().apply(&mut definitions);
    let shell = &definitions.font_data["weave-ui-cjk-fallback"];
    let word = wordcraft_fonts::CRAFT_FONTS
        .iter()
        .find(|font| font.family == "Noto Sans CJK SC")
        .expect("Chinese interface fallback");
    assert!(std::ptr::eq(shell.bytes(), word.bytes));
    assert_eq!(word.bytes.len(), 16_437_364);
    let serif = wordcraft_fonts::CRAFT_FONTS
        .iter()
        .find(|font| font.family == "Noto Serif CJK SC")
        .expect("Chinese document serif remains embedded");
    assert_eq!(serif.bytes.len(), 24_543_080);
    assert!(skrifa::FontRef::new(serif.bytes).is_ok());
    let latin: &[u8] = weave_font_assets::font!("Inter-Regular.ttf");
    assert!(std::ptr::eq(pdfcraft_fonts::INTER_REGULAR, latin));
    assert!(
        wordcraft_fonts::bundled()
            .iter()
            .any(|font| std::ptr::eq(*font, latin))
    );
}

#[test]
fn the_editors_read_the_installed_fonts_through_one_shared_scan() {
    let dirs = weave_system_fonts::font_dirs();
    // Word and Presentation catalog the shared scan's folders, which their databases then take
    // from that scan instead of walking the folders again.
    assert_eq!(wordcraft_fonts::system_font_dirs(), dirs);
    assert_eq!(deckcraft_fonts::system_font_dirs(), dirs);
    let catalog = weave_system_fonts::catalog();
    assert!(std::sync::Arc::ptr_eq(&catalog, &weave_system_fonts::catalog()));
    // Spreadsheet's catalog is built from the same scan: every face it lists is one it found.
    for face in gridcraft_ui_egui::fonts::catalog() {
        assert!(
            catalog.faces.iter().any(|f| f.path == face.path && f.index == face.index),
            "{} #{} is not in the shared scan",
            face.path.display(),
            face.index
        );
    }
}
