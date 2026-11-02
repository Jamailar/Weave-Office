# Weave Office CLI — guide for AI agents

Create, read and change documents (.docx .odt .rtf .md .txt), spreadsheets (.xlsx .ods .csv),
presentations (.pptx .odp) and PDFs with `weave-office <command>`. No window opens. The
commands drive the same engines as the editors, so files look exactly as they will in the app.

## Rules

1. **Add `--json`** to every call. Success: `{"ok":true,"data":…}`. Failure (exit code 1, or 2
   for a usage error): `{"ok":false,"error":{"code","message","suggestion","candidates"}}`.
   Read `suggestion` and `candidates` and correct yourself; do not guess twice.
2. **Prefer path edits**: `set`, `add` and `remove` change one node by its path and need no
   caret or selection. Fall back to `run`/`batch` with engine commands only for what they
   don't cover (`weave-office help props` lists what they do cover).
3. **Look before you change**: `view <file> outline` lists every node with its path; `get <file>
   <path>` returns one node in full. `set`/`add` answer with the changed node and its path.
4. **Never guess a command id or its parameters**: `commands <kind> <keyword>` searches, and
   `commands <kind> <exact.id>` shows one command's parameters. Kinds: `word`, `sheet`,
   `slides`, `pdf`.
5. **Every call opens the file afresh**: the caret is at the start and nothing is selected.
   Engine commands that depend on each other (select, then format) go into **one `batch`**.
6. A `batch` is all or nothing: if any step fails, the file is left untouched and the error
   lists each step's status under `details.results`. Use `--dry-run` to try without writing,
   `--out other.docx` to write a copy.
7. Paths are 1-based (`/body/p[1]`, `/slide[1]`, `/page[1]`). **Engine parameters keep each
   engine's own convention**: presentation `index`/`at` parameters are 0-based, PDF `page`
   parameters 1-based, spreadsheet cells A1-style.

## Commands

```
weave-office create  <file> [--template NAME] [--force]
weave-office info    <file>
weave-office view    <file> [outline|text|json]
weave-office get     <file> <path>
weave-office set     <file> <path> key=value ...
weave-office add     <file> <parent> --type T [key=value ...] [--after PATH|--before PATH|--index N]
weave-office remove  <file> <path>
weave-office commands <kind|file> [keyword|exact.id]
weave-office run     <file> <command.id> [key=value ...] [key:=JSON ...] [--params '{…}']
weave-office batch   <file> [--input steps.json | --commands '[…]']   (stdin by default)
weave-office export  <file> <output.(pdf|png|csv|html|md|txt|docx|…)> [--page N]
weave-office help    [agent|paths|props|<command>]
```

`key=value` turns numbers and true/false into JSON numbers and booleans; text keys (`text`,
`value`, `format`, `name`, `title`, `color`, `cell`, `range`, `sheet` …) always stay text. Use
`key:=JSON` for lists, objects or to force a type (`text:='"2024"'`, `at:=[72,72]`).

A batch is a JSON list of steps, any mix of `{"set": "<path>", "props": {…}}`,
`{"add": "<parent>", "type": "…", "props": {…}, "after": "<path>"}`, `{"remove": "<path>"}`,
`{"get": "<path>"}` and `{"command": "<id>", "params": {…}}`.

## Paths

| Kind | Path | Node |
|---|---|---|
| document | `/body/p[N]`, `/body/table[N]`, `/body/block[N]` | paragraph (style, text, runs), table, any block |
| spreadsheet | `/Sheet1`, `/Sheet1/B2`, `/Sheet1/A1:C10`, `/sheet[N]` | sheet summary + values, a cell (value, formula, style), a range |
| presentation | `/slide[N]`, `/slide[N]/shape[M]`, `shape[@id=ID]`, `shape[@name=Title 1]` | slide (layout, title, shapes, notes), shape |
| PDF | `/page[N]` | page size, rotation and text |

## Properties for set / add

| Node | Properties |
|---|---|
| paragraph `/body/p[N]` | `text style align bold italic underline strike font size color highlight` |
| `add /body --type` | `paragraph`, `heading` (`level=1..9`), `title`, `table` (`rows cols`), `pagebreak` |
| cell or range `/Sheet1/A1`, `/Sheet1/A1:C3` | `value` (text, number or `=formula`), `values:=[[…]]` (from the top-left cell), `bold italic underline color fill font size format align wrap merge` |
| sheet `/Sheet1` | `name tabColor`; `add / --type sheet name=…`; `remove /Sheet1/A1:B2` clears the cells |
| slide `/slide[N]` | `layout notes hidden`; `add / --type slide layout= title= body=` |
| shape `/slide[N]/shape[M]` | `text` (a newline starts a paragraph) `bold italic underline font size color align fill x y w h name`; `add /slide[N] --type textbox text= x= y= w= h=` |
| PDF page `/page[N]` | `rotate`; `add / --type page`; `add /page[N] --type text text= x= y=` |

Colours are `RRGGBB` or `#RRGGBB`; sizes and positions are in points.

## Recipes

Document:

```sh
weave-office create report.docx
weave-office add report.docx /body --type heading text="Quarterly report" --json
weave-office add report.docx /body --type paragraph text="Revenue grew 25%." --json
weave-office add report.docx /body --type table rows=2 cols=3 --json
weave-office set report.docx '/body/p[2]' bold=true color=C00000 --json
weave-office run report.docx edit.replaceAll text=25% with=27% --json
weave-office view report.docx outline
```

Several changes at once, all or nothing (`steps.json`):

```json
[{"add": "/body", "type": "heading", "props": {"text": "Summary", "level": 2}},
 {"add": "/body", "type": "paragraph", "props": {"text": "Costs fell."}},
 {"remove": "/body/p[3]"}]
```

`weave-office batch report.docx --input steps.json --json`

Engine commands reach everything else; select first, then format, in one batch:
`[{"command":"select.text","params":{"text":"Revenue"}},{"command":"format.bold"}]`.

Spreadsheet:

```sh
weave-office create sales.xlsx
weave-office set sales.xlsx /Sheet1/A1 'values:=[["Region","Q1"],["North",120],["South",95],["Total","=SUM(B2:B3)"]]' --json
weave-office set sales.xlsx /Sheet1/A1:B1 bold=true fill=FFFF00 --json
weave-office set sales.xlsx /Sheet1/B2:B4 format=0.00 --json
weave-office get sales.xlsx /Sheet1/B4 --json        # value 215, formula =SUM(B2:B3)
weave-office export sales.xlsx sales.csv
```

Presentation:

```sh
weave-office create deck.pptx
weave-office set deck.pptx '/slide[1]/shape[1]' text="Annual review" --json
weave-office add deck.pptx / --type slide layout=titleAndContent title="Q4 Report" body="Revenue +25%" --json
weave-office add deck.pptx '/slide[2]' --type textbox text="Source: finance" x=72 y=460 w=400 h=30 --json
weave-office view deck.pptx outline
weave-office export deck.pptx slide2.png --page 2
```

PDF (the open document's `doc` id is filled in for you):

```sh
weave-office run in.pdf text_find query=invoice --json
weave-office set in.pdf '/page[1]' rotate=90 --out rotated.pdf --json
weave-office add in.pdf '/page[1]' --type text text="Approved" x=400 y=40 --json
weave-office export in.pdf page1.png --page 1
weave-office view in.pdf text
```

Check the result with `view <file> outline`, `get`, or `export <file> out.png` and look at
the picture.
