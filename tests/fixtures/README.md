# External-producer interoperability fixtures

Small samples created and saved by a third-party office suite. Producer metadata, author names and vendor-specific theme, tag and namespace labels were neutralised; document content and structure are unchanged.

- `external-origin-pricing.xlsx`: labels, numbers, formulas, date formats and header fonts.
- `external-origin-text.docx`: created from a blank document on 2026-11-01. It contains a centered colored title, mixed Chinese and English text, a bold run, a red run, explicit paragraph spacing, and a shaded 2×2 table. `docProps/app.xml` identifies the producer as `ExternalOffice`.
- `external-origin-slides.pptx`: a blank two-slide presentation. Each slide has one text shape with two paragraphs; the second shape has a solid `accent1` theme fill. The saved XML uses 28/16 pt text on slide 1 and 21/16 pt text on slide 2. Its producer field is `ExternalOffice`.

`tests/external_interop.rs` checks the source files, edits a copy through the hosted editor, saves it, and reopens it in a fresh host. It also inspects DOCX and PPTX ZIP/XML with Python's standard library so the package checks do not depend on WordCraft's or DeckCraft's parser.
