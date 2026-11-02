# Shared open fonts

Weave's native build enables `shared-font-assets` in the four consumers. The small
`weave-font-assets` crate owns one named immutable static per distinct font file,
deduplicated by exact bytes, including the shell and existing Word/PDF Latin faces.
The editor manifests retain their original script, size and asset-policy filters;
they borrow shared bytes instead of each expanding `include_bytes!`. Standalone
vendor builds keep their previous optional-input path when this feature is off.
This unifies font storage, not the engines' font databases, discovery or shaping
policies. The bytes remain embedded, so runtime does not read Jamba or the source.

The four engines share the optional [craft-fonts](https://github.com/storytold/craft-fonts) input. The pinned revision is `37827adc41fd165f9a9234ac40f94dbad353aa69`, matching DeckCraft's release workflow. The older PdfCraft/GridCraft/WordCraft release pin (`b076417b5880bd4154bd22f00695b1d28c6bea83`) lacks `fonts/latin-manifest.txt`; DeckCraft's required build needs its Inter Regular/Bold faces. The newer pin retains the default and Simplified Chinese manifests consumed by the other engines. Font binaries live outside this repository.

Word documents also embed one independent Simplified Chinese serif fallback: [Noto Serif CJK SC Regular](https://github.com/notofonts/noto-cjk/blob/f8d157532fbfaeda587e826d4cd5b21a49186f7c/Serif/OTF/SimplifiedChinese/NotoSerifCJKsc-Regular.otf) from the pinned `notofonts/noto-cjk` commit `f8d157532fbfaeda587e826d4cd5b21a49186f7c`. This input does not alter the craft-fonts checkout or the UI font order.

## Prepare and verify

```sh
python3 scripts/prepare-fonts.py
python3 scripts/prepare-fonts.py --verify-only
python3 scripts/prepare-document-fonts.py
python3 scripts/prepare-document-fonts.py --verify-only
```

The script checks out the pinned revision at `/Volumes/Jamba/DevCache/WeaveOffice/inputs/craft-fonts`, then checks the clean Git revision, all three eight-field manifests, every font's SHA-256, and its adjacent OFL-1.1 text. The manifests contain 13 default, 1 Chinese, and 51 Latin faces (65 files; 80,245,408 bytes in this revision). `--dir` selects another directory on the mounted Jamba volume. Existing checkouts are verified, never silently moved to a new revision. The previous checkout is retained externally as `craft-fonts-b076417`.

`prepare-document-fonts.py` places the 24,543,080-byte serif OTF, its [OFL-1.1 license](https://github.com/notofonts/noto-cjk/blob/f8d157532fbfaeda587e826d4cd5b21a49186f7c/Serif/LICENSE), and a copy of the [fixed manifest](../scripts/document-fonts-manifest.txt) in `/Volumes/Jamba/DevCache/WeaveOffice/inputs/weave-document-fonts`. It verifies the manifest byte for byte and verifies SHA-256 for both the font and license. Downloads and temporary staging stay on Jamba. Existing input is only verified, never silently replaced. The font is embedded into the executable during native builds and remains available when the external volume is disconnected.

Weave Office's development and packaging scripts verify both font inputs and set `CRAFT_FONTS_DIR`, `CRAFT_FONTS_REQUIRED=1`, `WEAVE_DOCUMENT_FONTS_DIR`, and `WEAVE_DOCUMENT_FONTS_REQUIRED=1`. Direct native Cargo builds need these variables or equivalent machine-local Cargo configuration; run both preparation scripts with `--verify-only` first. The document serif build script reads the prepared manifest and font only for native targets; the preparation script performs the pinned SHA-256 and license checks. All four engines read `fonts/manifest.txt`. PdfCraft, GridCraft and DeckCraft additionally read `fonts/simplified-chinese-manifest.txt` on native targets; it contains LXGW WenKai Lite for `Hani,Hans` and must stay out of wasm builds. DeckCraft also reads `fonts/latin-manifest.txt` for presentation fonts. Other engines ignore that additional manifest. A missing or invalid required input should fail the build. Do not add craft-fonts as a Cargo dependency; [upstream integration guidance](https://github.com/storytold/craft-fonts/blob/37827adc41fd165f9a9234ac40f94dbad353aa69/docs/integration.md) explains the optional environment input.

Weave Office's shell UI reads SFNS and PingFang from macOS at runtime. `src/ui_fonts.rs` uses embedded Inter and the `CRAFT_FONTS_DIR` Noto Sans CJK SC face as interface fallbacks when those system fonts are unavailable. These faces are applied to interface font families only; document and cell font choices remain editor-owned. Apple system fonts are not copied into the app bundle. The engines retain their own fallback policies: PdfCraft prefers BIZ UDPGothic for Japanese UI text and includes native-only LXGW WenKai Lite for Simplified Chinese fallback. For PDF text, select an embedded face that covers the actual glyphs; a system UI fallback must never be copied into output or embedded in a PDF. Each engine applies its own size and script policy to the manifest's Noto Sans CJK SC face.

The Word Font dialog's sample is document content rather than interface text. It uses the document layout and renderer with the selected Latin and East Asian families, size, color and character effects; it does not install these faces into egui's interface families. In hosted mode it follows the document canvas's font-weight policy, omitting the extra macOS screen darkening. Its fixed 88 pt preview area clips large samples so changing size does not move the dialog buttons. Invalid or non-finite sizes fall back to 12 pt, and preview sizes are bounded to 6–72 pt. Only the current preview texture is retained, with font generation, style, theme, display scale, hosted mode and dimensions in its cache key. Previewing does not edit the open document or its undo history. The font-menu entry previews retain their existing temporary-face loading policy; other galleries retain their existing rendering options.

## Distribution

Only include licences for fonts actually embedded in a package. Copy each corresponding `fonts/<family>/OFL.txt` into the package licences directory with a unique family name, including any consumed Latin manifest faces, and add this sentence to the app's `NOTICE`: “Builds made with `CRAFT_FONTS_DIR` embed fonts listed in craft-fonts' `ATTRIBUTION.md`.” Keep that [attribution file](https://github.com/storytold/craft-fonts/blob/37827adc41fd165f9a9234ac40f94dbad353aa69/ATTRIBUTION.md) or a pinned link available with the release. The source tree's font files and licence texts must not be committed here. PdfCraft's [asset policy](../vendor/pdfcraft/AGENTS.md) permits these open OFL releases, including Noto CJK, while excluding copies installed by proprietary products.

The macOS bundle includes the document serif's OFL text, pinned manifest, and copyright notice under `Contents/Resources/licenses/weave-document-fonts`. Its font bytes are compiled into the executable; the bundle runs without the build input.
