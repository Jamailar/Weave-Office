# winprint (vendored)

Windows printing for `craft-print` (printer list, PrintTicket, and jobs drawn with
`Windows.Data.Pdf` into the XPS print API). Snapshot of
[storytold/winprint.rs](https://github.com/storytold/winprint.rs) at
`5f80b9baf6b58937c401656634e42d33f00ddf49` (winprint 0.2.1, BSD-3-Clause, see `LICENSE.md`),
used through `[patch."https://github.com/storytold/winprint.rs"]` in the root `Cargo.toml`.

## Weave Office change

Upstream calls `PdfCreateRenderer`, the `PT*` PrintTicket functions and `StartXpsPrintJob`
through the `windows` crate, which puts `windows.data.pdf.dll`, `prntvpt.dll` and `xpsprint.dll`
in the executable's import table. Windows loads those before `main`, so on installations without
them (some N/KN, LTSC, Server and trimmed-down systems) the whole app failed to start with
"The code execution cannot proceed because windows.data.pdf.dll was not found".

`src/utils/system_dll.rs` replaces those wrappers with versions that load the DLL from System32
when a print job needs it. Where a DLL is missing only printing fails, with an error such as
"Windows PDF printing (windows.data.pdf.dll) is not available on this system"; the app starts and
Save as PDF still works. `scripts/check-windows-imports.py` (run by the Windows release build)
keeps such optional DLLs out of the import table.

Also left out: the upstream build script and its build dependencies (it only downloads PDFium for
the off-by-default `pdfium` feature), tests, examples and test data.

To update: copy `src/` from the new upstream revision, re-apply the imports from
`crate::utils::system_dll` in `printer/win_pdf.rs`, `printer/xps.rs` and `ticket/{builder,
print_capabilities,print_ticket}.rs`, then build for Windows and run the import check.
