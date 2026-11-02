# Upstream sources

Pinned snapshots used by Weave Office. Original licenses, notices and asset attribution remain in each source tree. Local UI changes add hosted editor surfaces; Weave Office owns application windows, file tabs, dialogs and lifecycle.

| Source | Repository | Commit |
| --- | --- | --- |
| WordCraft | https://github.com/storytold/wordcraft | 6e0f7e79edabf5904ecb73104e809e6f053d9d27 |
| GridCraft | https://github.com/storytold/gridcraft | a85b1aea3a8a3369581a749d8da041e4db014504 |
| DeckCraft | https://github.com/storytold/deckcraft | 82573331a8f2b87ccb536b9b66d55b90295f2111 |
| PdfCraft | https://github.com/storytold/pdfcraft | eea00a6186546762d11d857666249672bff3fcad |

Upstream source licenses are MIT OR Apache-2.0; individual third-party assets and dependencies retain their respective licenses. ArtCraft branding is not used as Weave Office branding or bundled in its application resources.

## Hosted integration patches

The local `crates/ui-egui/src/app/hosted.rs` adapters retain each editor's document model and editing view while delegating application lifecycle and file operations to Weave Office. Root integration tests exercise these adapters; the upstream standalone applications are not launched by Weave Office.

- WordCraft and DeckCraft consume completed host picture selections before save/close checks. Word file-dialog services, Grid picture services and Deck picture inboxes are connected to the root native chooser by `src/office_files.rs`; results retain their originating document and request identity.
- GridCraft PDF/CSV/HTML export commands and DeckCraft export/template commands are queued to the host even when a caller supplies a path. The host currently supports shared PDF export; unsupported formats produce an error rather than bypassing its write policy.
- GridCraft host-tab deactivation retains cell drafts and their selection. Save still validates and commits the draft; a rejected value must not prevent the host's explicit discard/close transaction.
- WordCraft, GridCraft and DeckCraft accept one host clipboard service for structured copies and system HTML/text. Native clipboard identity and per-editor copy generations prevent stale sessions from restoring or clearing newer copies. WordCraft and DeckCraft publish a read-only preview before cutting, so a refused system write retains the selection and history. GridCraft synchronizes explicit cancellation and queues cross-workbook cuts for a host-coordinated two-session move. Both workbook snapshots are prepared before either live document changes; source cleanup and destination values/styles/internal formulas commit together, with independent single-step undo/redo. Queue identity, drafts, stale snapshots and closed documents are checked before applying. Full-block internal absolute and relative references follow the move; external dependencies, dynamic references, defined names, formula validation, conditional formats, tables/charts/sparklines/pivots are conservatively refused because external workbook links and their dependency rewrites are not implemented. Special paste transformations and insert-cut cells are refused with the cut retained. Save/export drain pending moves before producing output; closing cannot discard a queued move.
- PdfCraft advanced file requests, generated documents and background output use host-owned request and completion contracts. The host protects every open document path before writing exported copies.
- PdfCraft exposes task-completion polling separately from active input and styling. Weave Office polls inactive documents and prevents close/quit from dropping unfinished OCR, optimization, export, action or print results. Closing also settles implicit OCR deferred in Read mode while retaining recognized pages for the save decision.
- PdfCraft page-image context menus exclusively own their popup while open. The generic page menu remains available for text and blank-page clicks; it must not reuse the image popup's widget identities in the same frame.

These patches describe Weave Office's embedded mode and do not change the standalone projects' published parity assessments.
