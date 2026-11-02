# WPS interoperability fixtures

- `wps-origin-pricing.xlsx`: created and saved in WPS Spreadsheet. The test covers labels, numbers, formulas, date formats, and header fonts.
- `wps-origin-text.docx`: created from a blank document and saved in WPS Writer on 2026-11-01. It contains a centered colored title, mixed Chinese and English text, a bold run, a red run, explicit paragraph spacing, and a shaded 2×2 table. `docProps/app.xml` identifies WPS Office as the producer. The baseline screenshot remains outside the repository.
- `wps-origin-slides.pptx`: created from a blank two-slide presentation and saved in WPS on 2026-11-01. Each slide has one text shape with two paragraphs; the second shape has a solid `accent1` theme fill. The saved XML uses 28/16 pt text on slide 1 and 21/16 pt text on slide 2. Its producer field is `WPS 表格`. The two baseline screenshots remain outside the repository.

`tests/wps_interop.rs` checks the source files, edits a copy through the hosted editor, saves it, and reopens it in a fresh host. It also inspects DOCX and PPTX ZIP/XML with Python's standard library so the package checks do not depend on WordCraft's or DeckCraft's parser.
