# Weave Office CLI — guide for AI agents

Create, read and change documents (.docx .odt .rtf .md .txt), spreadsheets (.xlsx .ods .csv),
presentations (.pptx .odp) and PDFs with `weave-office <command>`. No window opens. The
commands drive the same engines as the editors, so files look exactly as they will in the app.

## Rules

1. **Add `--json`** to every call. Success: `{"ok":true,"data":…}`. Failure (exit code 1, or 2
   for a usage error): `{"ok":false,"error":{"code","message","suggestion","candidates"}}`.
   Read `suggestion` and `candidates` and correct yourself; do not guess twice.
2. **Look before you change**: `view <file> outline` lists every node with its path; `get <file>
   <path>` returns one node in full.
3. **Never guess a command id or its parameters**: `commands <kind> <keyword>` searches, and
   `commands <kind> <exact.id>` shows one command's parameters. Kinds: `word`, `sheet`,
   `slides`, `pdf`.
4. **Every call opens the file afresh**: the caret is at the start and nothing is selected.
   Steps that depend on each other (move the caret, then type) go into **one `batch`**.
5. A `batch` is all or nothing: if any step fails, the file is left untouched and the error
   lists each step's status under `details.results`. Use `--dry-run` to try without writing,
   `--out other.docx` to write a copy.
6. Paths are 1-based (`/body/p[1]`, `/slide[1]`, `/page[1]`). **Engine parameters keep each
   engine's own convention**: presentation `index`/`at` parameters are 0-based, PDF `page`
   parameters 1-based, spreadsheet cells A1-style.

## Commands

```
weave-office create  <file> [--template NAME] [--force]
weave-office info    <file>
weave-office view    <file> [outline|text|json]
weave-office get     <file> <path>
weave-office commands <kind|file> [keyword|exact.id]
weave-office run     <file> <command.id> [key=value ...] [key:=JSON ...] [--params '{…}']
weave-office batch   <file> [--input steps.json | --commands '[…]']   (stdin by default)
weave-office export  <file> <output.(pdf|png|csv|html|md|txt|docx|…)> [--page N]
weave-office help    [agent|paths|<command>]
```

`key=value` turns numbers and true/false into JSON numbers and booleans; text keys (`text`,
`input`, `name`, `title`, `color`, `cell`, `range`, `sheet` …) always stay text. Use
`key:=JSON` for lists, objects or to force a type (`text:='"2024"'`, `at:=[72,72]`).

A batch is a JSON list. Each step is `{"command": "<id>", "params": {…}}`, or
`{"get": "<path>"}` to read in the middle of a batch.

## Paths

| Kind | Path | Node |
|---|---|---|
| document | `/body/p[N]`, `/body/table[N]`, `/body/block[N]` | paragraph (style, text, runs), table, any block |
| spreadsheet | `/Sheet1`, `/Sheet1/B2`, `/Sheet1/A1:C10`, `/sheet[N]` | sheet summary + values, a cell (value, formula, style), a range |
| presentation | `/slide[N]`, `/slide[N]/shape[M]`, `shape[@id=ID]`, `shape[@name=Title 1]` | slide (layout, title, shapes, notes), shape |
| PDF | `/page[N]` | page size, rotation and text |

## Recipes

Document — heading, two paragraphs and a table at the end:

```sh
weave-office create report.docx
weave-office batch report.docx --json <<'EOF'
[{"command":"text.insert","params":{"text":"Quarterly report"}},
 {"command":"para.heading1"},
 {"command":"caret.docEnd"},{"command":"text.newParagraph"},{"command":"para.normal"},
 {"command":"text.insert","params":{"text":"Revenue grew 25%.\nCosts fell.","paragraphs":true}},
 {"command":"insert.table","params":{"rows":2,"cols":3}}]
EOF
weave-office run report.docx edit.replaceAll text=25% with=27% --json
weave-office view report.docx outline
```

Select-then-format in one batch: `select.text {"text":"Revenue"}` then `format.bold`.

Spreadsheet:

```sh
weave-office create sales.xlsx
weave-office run sales.xlsx range.setValues range=A1 'values:=[["Region","Q1"],["North",120],["South",95],["Total","=SUM(B2:B3)"]]' --json
weave-office run sales.xlsx home.bold range=A1:B1 --json
weave-office get sales.xlsx /Sheet1/B4 --json        # value 215, formula =SUM(B2:B3)
weave-office export sales.xlsx sales.csv
```

Presentation (slide `index`/`at` are 0-based in parameters):

```sh
weave-office create deck.pptx
weave-office run deck.pptx slide.new layout=titleAndContent title="Q4 Report" body="Revenue +25%" --json
weave-office view deck.pptx outline                  # shape ids for text.edit
weave-office batch deck.pptx --commands '[{"command":"text.edit","params":{"id":258}},{"command":"text.insert","params":{"text":"Annual review"}},{"command":"text.exit"}]' --json
weave-office export deck.pptx slide2.png --page 2
```

PDF (the open document's `doc` id is filled in for you):

```sh
weave-office run in.pdf text_find query=invoice --json
weave-office run in.pdf page_rotate 'pages:=[1]' angle=90 --out rotated.pdf --json
weave-office export in.pdf page1.png --page 1
weave-office view in.pdf text
```

Check the result with `view <file> outline`, `get`, or `export <file> out.png` and look at
the picture.
