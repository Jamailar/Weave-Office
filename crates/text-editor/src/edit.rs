//! Text operations behind the editor's commands, on plain strings so they can be tested
//! without a window. Positions are byte offsets; a selection is `anchor..head` in either order.

use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub anchor: usize,
    pub head: usize,
}

impl Selection {
    pub fn caret(at: usize) -> Self {
        Self {
            anchor: at,
            head: at,
        }
    }
    pub fn new(anchor: usize, head: usize) -> Self {
        Self { anchor, head }
    }
    pub fn range(self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }
    pub fn is_empty(self) -> bool {
        self.anchor == self.head
    }
}

/// Start of the line containing `at`.
pub fn line_start(text: &str, at: usize) -> usize {
    text[..at].rfind('\n').map_or(0, |i| i + 1)
}

/// End of the line containing `at` (before its `\n`).
pub fn line_end(text: &str, at: usize) -> usize {
    text[at..].find('\n').map_or(text.len(), |i| at + i)
}

/// The whole lines a selection touches, as `start..end` (end before the last `\n`). A
/// selection ending at the start of a line does not include that line.
pub fn lines_of(text: &str, selection: Selection) -> Range<usize> {
    let range = selection.range();
    let mut last = range.end;
    if range.end > range.start && text[..range.end].ends_with('\n') {
        last -= 1;
    }
    line_start(text, range.start)..line_end(text, last)
}

/// 1-based line and column (in characters) of `at`.
pub fn line_column(text: &str, at: usize) -> (usize, usize) {
    let line = text[..at].matches('\n').count() + 1;
    let column = text[line_start(text, at)..at].chars().count() + 1;
    (line, column)
}

/// Byte offset of the start of 1-based `line`, clamped to the text.
pub fn offset_of_line(text: &str, line: usize) -> usize {
    if line <= 1 {
        return 0;
    }
    text.match_indices('\n')
        .nth(line - 2)
        .map_or(text.len(), |(i, _)| i + 1)
}

/// The indentation unit a file uses: a tab, or the smallest run of leading spaces seen.
pub fn detect_indent(text: &str) -> String {
    let mut tabs = 0;
    let mut smallest = usize::MAX;
    for line in text.lines().take(2000) {
        if line.starts_with('\t') {
            tabs += 1;
        } else {
            let spaces = line.len() - line.trim_start_matches(' ').len();
            if spaces >= 2
                && spaces < smallest
                && !line.trim().is_empty()
                && !line.trim_start().starts_with('*')
            {
                smallest = spaces;
            }
        }
    }
    if tabs > 0 && smallest == usize::MAX {
        "\t".into()
    } else if smallest == usize::MAX || smallest > 8 {
        "    ".into()
    } else {
        " ".repeat(smallest)
    }
}

/// An edit: replace `range` with `text`, then select `selection` (in the new text).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    pub range: Range<usize>,
    pub text: String,
    pub selection: Selection,
}

impl Edit {
    pub fn apply(&self, text: &mut String) {
        text.replace_range(self.range.clone(), &self.text);
    }
}

/// Rewrite every line the selection touches with `f`, keeping the selection on the same text.
fn map_lines(text: &str, selection: Selection, mut f: impl FnMut(&str, usize) -> String) -> Edit {
    let lines = lines_of(text, selection);
    let mut out = String::new();
    // (old start, old length, new length) of each line.
    let mut spans: Vec<(usize, usize, usize)> = Vec::new();
    let mut start = lines.start;
    for (index, line) in text[lines.clone()].split('\n').enumerate() {
        if index > 0 {
            out.push('\n');
        }
        let new = f(line, index);
        spans.push((start, line.len(), new.len()));
        start += line.len() + 1;
        out.push_str(&new);
    }
    let total = out.len() as isize - lines.len() as isize;
    let map = |at: usize| -> usize {
        if at < lines.start {
            return at;
        }
        let mut new_start = lines.start;
        for &(start, old_len, new_len) in &spans {
            if at <= start + old_len {
                let shift = new_len as isize - old_len as isize;
                let within = ((at - start) as isize + shift).clamp(0, new_len as isize) as usize;
                return new_start + within;
            }
            new_start += new_len + 1;
        }
        (at as isize + total) as usize
    };
    let selection = Selection::new(map(selection.anchor), map(selection.head));
    Edit {
        range: lines,
        text: out,
        selection,
    }
}

pub fn indent(text: &str, selection: Selection, unit: &str) -> Edit {
    if selection.is_empty()
        || lines_of(text, selection).start == line_start(text, selection.range().end)
            && !text[selection.range()].contains('\n')
    {
        // A caret or a selection within one line: insert the unit at the caret.
        let at = selection.range().start;
        let range = selection.range();
        let column = text[line_start(text, at)..at].chars().count();
        let insert = if unit == "\t" {
            "\t".to_owned()
        } else {
            " ".repeat(unit.len() - column % unit.len())
        };
        let end = at + insert.len();
        return Edit {
            range,
            text: insert,
            selection: Selection::caret(end),
        };
    }
    map_lines(text, selection, |line, _| {
        if line.is_empty() {
            String::new()
        } else {
            format!("{unit}{line}")
        }
    })
}

pub fn outdent(text: &str, selection: Selection, unit: &str) -> Edit {
    map_lines(text, selection, |line, _| {
        if let Some(rest) = line.strip_prefix('\t') {
            return rest.to_owned();
        }
        let spaces = line.len() - line.trim_start_matches(' ').len();
        line[spaces
            .min(unit.len().max(1))
            .min(if unit == "\t" { 4 } else { unit.len() })..]
            .to_owned()
    })
}

pub fn toggle_comment(text: &str, selection: Selection, marker: &str) -> Edit {
    let lines = lines_of(text, selection);
    let all_commented = text[lines]
        .split('\n')
        .filter(|l| !l.trim().is_empty())
        .all(|l| l.trim_start().starts_with(marker));
    let indent = text[lines_of(text, selection)]
        .split('\n')
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    map_lines(text, selection, |line, _| {
        if line.trim().is_empty() {
            return line.to_owned();
        }
        if all_commented {
            let at = line.len() - line.trim_start().len();
            let rest = &line[at + marker.len()..];
            format!("{}{}", &line[..at], rest.strip_prefix(' ').unwrap_or(rest))
        } else {
            format!("{}{marker} {}", &line[..indent], &line[indent..])
        }
    })
}

/// Move the selected lines up (`-1`) or down (`1`).
pub fn move_lines(text: &str, selection: Selection, direction: i32) -> Option<Edit> {
    let lines = lines_of(text, selection);
    let block = &text[lines.clone()];
    if direction < 0 {
        if lines.start == 0 {
            return None;
        }
        let above = line_start(text, lines.start - 1);
        let previous = &text[above..lines.start - 1];
        let shift = previous.len() + 1;
        Some(Edit {
            range: above..lines.end,
            text: format!("{block}\n{previous}"),
            selection: Selection::new(selection.anchor - shift, selection.head - shift),
        })
    } else {
        if lines.end >= text.len() {
            return None;
        }
        let below_end = line_end(text, lines.end + 1);
        let next = &text[lines.end + 1..below_end];
        let shift = next.len() + 1;
        Some(Edit {
            range: lines.start..below_end,
            text: format!("{next}\n{block}"),
            selection: Selection::new(selection.anchor + shift, selection.head + shift),
        })
    }
}

pub fn duplicate_lines(text: &str, selection: Selection) -> Edit {
    let lines = lines_of(text, selection);
    let block = &text[lines.clone()];
    let shift = block.len() + 1;
    Edit {
        range: lines.end..lines.end,
        text: format!("\n{block}"),
        selection: Selection::new(selection.anchor + shift, selection.head + shift),
    }
}

pub fn delete_lines(text: &str, selection: Selection) -> Edit {
    let lines = lines_of(text, selection);
    let range = if lines.end < text.len() {
        lines.start..lines.end + 1
    } else if lines.start > 0 {
        lines.start - 1..lines.end
    } else {
        lines.clone()
    };
    let at = range.start;
    Edit {
        range,
        text: String::new(),
        selection: Selection::caret(at),
    }
}

/// The list or quote prefix of a Markdown line that Enter continues: `(prefix, next prefix)`.
fn markdown_continuation(line: &str) -> Option<(String, String)> {
    let indent_len = line.len() - line.trim_start_matches([' ', '\t']).len();
    let indent = &line[..indent_len];
    let rest = &line[indent_len..];
    if let Some(after) = rest
        .strip_prefix("> ")
        .or_else(|| (rest == ">").then_some(""))
    {
        let _ = after;
        let prefix = format!("{indent}> ");
        return Some((
            if rest == ">" {
                format!("{indent}>")
            } else {
                prefix.clone()
            },
            prefix,
        ));
    }
    for bullet in ["- ", "* ", "+ "] {
        if let Some(after) = rest.strip_prefix(bullet) {
            for task in ["[ ] ", "[x] ", "[X] "] {
                if after.starts_with(task) {
                    return Some((
                        format!("{indent}{bullet}{task}"),
                        format!("{indent}{bullet}[ ] "),
                    ));
                }
            }
            return Some((format!("{indent}{bullet}"), format!("{indent}{bullet}")));
        }
    }
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && digits <= 9 {
        let delimiter = rest[digits..].chars().next()?;
        if matches!(delimiter, '.' | ')') && rest[digits + 1..].starts_with(' ') {
            let number: u64 = rest[..digits].parse().ok()?;
            let after = &rest[digits + 2..];
            let task = ["[ ] ", "[x] ", "[X] "]
                .into_iter()
                .find(|t| after.starts_with(t))
                .map_or("", |_| "[ ] ");
            let current_task = ["[ ] ", "[x] ", "[X] "]
                .into_iter()
                .find(|t| after.starts_with(t))
                .unwrap_or("");
            return Some((
                format!("{indent}{number}{delimiter} {current_task}"),
                format!("{indent}{}{delimiter} {task}", number + 1),
            ));
        }
    }
    None
}

/// Enter: a new line that keeps the indentation, continues Markdown lists and quotes, ends an
/// empty list item, and indents after an opening bracket in code.
pub fn newline(text: &str, selection: Selection, markdown: bool, unit: &str) -> Edit {
    let range = selection.range();
    let start = line_start(text, range.start);
    let line = &text[start..line_end(text, range.start)];
    let before = &text[start..range.start];
    if markdown && let Some((prefix, next)) = markdown_continuation(line) {
        if line.trim_end() == prefix.trim_end() && range.start >= start + prefix.trim_end().len() {
            // Enter on an empty item ends the list: remove its marker.
            let indent = &line[..line.len() - line.trim_start().len()];
            let end = line_end(text, range.start);
            return Edit {
                range: start..end.max(range.end),
                text: indent.to_owned(),
                selection: Selection::caret(start + indent.len()),
            };
        }
        if before.len() >= prefix.len() {
            let insert = format!("\n{next}");
            return Edit {
                selection: Selection::caret(range.start + insert.len()),
                range,
                text: insert,
            };
        }
    }
    let indent: String = before
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect();
    let opener = before.trim_end().chars().last();
    let closer = text[range.end..].chars().next();
    if !markdown && matches!(opener, Some('{' | '[' | '(')) {
        let inner = format!("\n{indent}{unit}");
        if matches!(
            (opener, closer),
            (Some('{'), Some('}')) | (Some('['), Some(']')) | (Some('('), Some(')'))
        ) {
            let insert = format!("{inner}\n{indent}");
            return Edit {
                selection: Selection::caret(range.start + inner.len()),
                range,
                text: insert,
            };
        }
        return Edit {
            selection: Selection::caret(range.start + inner.len()),
            range,
            text: inner,
        };
    }
    let insert = format!("\n{indent}");
    Edit {
        selection: Selection::caret(range.start + insert.len()),
        range,
        text: insert,
    }
}

/// Wrap the selection in `before`/`after`, or unwrap it when it is already wrapped. With no
/// selection, wrap the word at the caret, or insert the pair with the caret between.
pub fn toggle_wrap(text: &str, selection: Selection, before: &str, after: &str) -> Edit {
    let mut range = selection.range();
    if range.is_empty() {
        let word_start = text[..range.start]
            .char_indices()
            .rev()
            .take_while(|(_, c)| c.is_alphanumeric() || *c == '_')
            .last()
            .map_or(range.start, |(i, _)| i);
        let word_end = range.end
            + text[range.end..]
                .char_indices()
                .take_while(|(_, c)| c.is_alphanumeric() || *c == '_')
                .last()
                .map_or(0, |(i, c)| i + c.len_utf8());
        range = word_start..word_end;
    }
    let inner = &text[range.clone()];
    // Already wrapped outside the selection?
    if range.start >= before.len()
        && text[..range.start].ends_with(before)
        && text[range.end..].starts_with(after)
    {
        let start = range.start - before.len();
        return Edit {
            range: start..range.end + after.len(),
            text: inner.to_owned(),
            selection: Selection::new(start, start + inner.len()),
        };
    }
    if inner.len() >= before.len() + after.len()
        && inner.starts_with(before)
        && inner.ends_with(after)
    {
        let unwrapped = &inner[before.len()..inner.len() - after.len()];
        return Edit {
            range: range.clone(),
            text: unwrapped.to_owned(),
            selection: Selection::new(range.start, range.start + unwrapped.len()),
        };
    }
    let start = range.start + before.len();
    Edit {
        range: range.clone(),
        text: format!("{before}{inner}{after}"),
        selection: if inner.is_empty() {
            Selection::caret(start)
        } else {
            Selection::new(start, start + inner.len())
        },
    }
}

/// Insert a Markdown link around the selection: `[text](url)` with `url` selected.
pub fn insert_link(text: &str, selection: Selection) -> Edit {
    let range = selection.range();
    let label = &text[range.clone()];
    let looks_like_url = label.starts_with("http://") || label.starts_with("https://");
    let (inserted, select) = if looks_like_url {
        let s = format!("[]({label})");
        (s, 1..1)
    } else {
        let s = format!("[{label}](url)");
        let at = label.len() + 3;
        (s, at..at + 3)
    };
    Edit {
        range: range.clone(),
        selection: Selection::new(range.start + select.start, range.start + select.end),
        text: inserted,
    }
}

/// The block prefix of a Markdown line, without indentation: heading, list, task or quote.
fn strip_block_prefix(line: &str) -> (&str, &str) {
    let indent = line.len() - line.trim_start_matches([' ', '\t']).len();
    let rest = &line[indent..];
    let hashes = rest.chars().take_while(|&c| c == '#').count();
    if (1..=6).contains(&hashes) && rest[hashes..].starts_with(' ') {
        return (&line[..indent], &rest[hashes + 1..]);
    }
    if let Some((prefix, _)) = markdown_continuation(line) {
        return (&line[..indent], &line[prefix.len().min(line.len())..]);
    }
    (&line[..indent], rest)
}

/// Kinds of Markdown block a line can be switched to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Block {
    Paragraph,
    Heading(u8),
    Bullet,
    Numbered,
    Task,
    Quote,
}

fn block_of(line: &str) -> Block {
    let rest = line.trim_start_matches([' ', '\t']);
    let hashes = rest.chars().take_while(|&c| c == '#').count();
    if (1..=6).contains(&hashes) && rest[hashes..].starts_with(' ') {
        return Block::Heading(hashes as u8);
    }
    if rest.starts_with('>') {
        return Block::Quote;
    }
    if ["- [", "* [", "+ ["].iter().any(|p| rest.starts_with(p))
        && markdown_continuation(line).is_some_and(|(p, _)| p.ends_with("] "))
    {
        return Block::Task;
    }
    if rest.starts_with("- ") || rest.starts_with("* ") || rest.starts_with("+ ") {
        return Block::Bullet;
    }
    if markdown_continuation(line).is_some() {
        return Block::Numbered;
    }
    Block::Paragraph
}

/// Switch every selected line to `block`; switching to the kind a line already is turns it
/// back into a paragraph.
pub fn set_block(text: &str, selection: Selection, block: Block) -> Edit {
    let lines = lines_of(text, selection);
    let all_same = text[lines].split('\n').all(|l| block_of(l) == block);
    let target = if all_same { Block::Paragraph } else { block };
    map_lines(text, selection, |line, index| {
        let (indent, body) = strip_block_prefix(line);
        let prefix = match target {
            Block::Paragraph => String::new(),
            Block::Heading(level) => format!("{} ", "#".repeat(level as usize)),
            Block::Bullet => "- ".into(),
            Block::Numbered => format!("{}. ", index + 1),
            Block::Task => "- [ ] ".into(),
            Block::Quote => "> ".into(),
        };
        format!("{indent}{prefix}{body}")
    })
}

/// Flip the task checkbox whose `[ ]` / `[x]` starts at `marker`.
pub fn toggle_task(text: &str, marker: usize) -> Option<Edit> {
    let inner = marker + 1;
    let current = text.get(inner..inner + 1)?;
    let replacement = if current == " " { "x" } else { " " };
    Some(Edit {
        range: inner..inner + 1,
        text: replacement.into(),
        selection: Selection::caret(inner + 1),
    })
}

/// Byte ranges of every match of `query`.
pub fn find_all(text: &str, query: &str, match_case: bool, whole_word: bool) -> Vec<Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let boundary = |at: usize| {
        let before = text[..at].chars().next_back();
        before.is_none_or(|c| !(c.is_alphanumeric() || c == '_'))
    };
    let end_boundary = |at: usize| {
        let after = text[at..].chars().next();
        after.is_none_or(|c| !(c.is_alphanumeric() || c == '_'))
    };
    if match_case {
        for (i, m) in text.match_indices(query) {
            if !whole_word || (boundary(i) && end_boundary(i + m.len())) {
                out.push(i..i + m.len());
            }
            if out.len() >= 100_000 {
                break;
            }
        }
        return out;
    }
    // Case-insensitive: compare lowercase per char so byte offsets stay in the original text.
    let needle: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut i = 0;
    while i < chars.len() {
        let mut j = 0;
        let mut k = i;
        let mut lowered = Vec::new();
        while j < needle.len() && k < chars.len() {
            if lowered.is_empty() {
                lowered = chars[k].1.to_lowercase().collect();
                lowered.reverse();
                k += 1;
            }
            if lowered.pop() != Some(needle[j]) {
                break;
            }
            j += 1;
        }
        if j == needle.len() && lowered.is_empty() {
            let start = chars[i].0;
            let end = chars.get(k).map_or(text.len(), |c| c.0);
            if !whole_word || (boundary(start) && end_boundary(end)) {
                out.push(start..end);
                if out.len() >= 100_000 {
                    break;
                }
                i = k.max(i + 1);
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Reformat JSON with `indent` per level (`None` = minify). The document is checked first;
/// key order, number spelling and string escapes are kept exactly.
pub fn format_json(text: &str, indent: Option<&str>) -> Result<String, String> {
    if let Err(error) = serde_json::from_str::<serde_json::Value>(text) {
        return Err(format!(
            "JSON 无效(第 {} 行第 {} 列):{error}",
            error.line(),
            error.column()
        ));
    }
    let mut out = String::with_capacity(text.len() + text.len() / 4);
    let mut depth = 0usize;
    let mut chars = text.chars().peekable();
    let newline = |out: &mut String, depth: usize| {
        if let Some(unit) = indent {
            out.push('\n');
            for _ in 0..depth {
                out.push_str(unit);
            }
        }
    };
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                out.push(c);
                let mut escaped = false;
                for s in chars.by_ref() {
                    out.push(s);
                    if escaped {
                        escaped = false;
                    } else if s == '\\' {
                        escaped = true;
                    } else if s == '"' {
                        break;
                    }
                }
            }
            '{' | '[' => {
                out.push(c);
                // Keep empty containers on one line.
                while chars.peek().is_some_and(|c| c.is_whitespace()) {
                    chars.next();
                }
                if matches!(chars.peek(), Some('}' | ']')) {
                    out.push(chars.next().unwrap_or(' '));
                } else {
                    depth += 1;
                    newline(&mut out, depth);
                }
            }
            '}' | ']' => {
                depth = depth.saturating_sub(1);
                newline(&mut out, depth);
                out.push(c);
            }
            ',' => {
                out.push(c);
                newline(&mut out, depth);
            }
            ':' => {
                out.push(':');
                if indent.is_some() {
                    out.push(' ');
                }
            }
            c if c.is_whitespace() => {}
            c => out.push(c),
        }
    }
    if indent.is_some() && text.ends_with('\n') {
        out.push('\n');
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(
        text: &str,
        sel: Selection,
        edit: impl Fn(&str, Selection) -> Edit,
    ) -> (String, Selection) {
        let mut s = text.to_owned();
        let edit = edit(text, sel);
        edit.apply(&mut s);
        (s, edit.selection)
    }

    #[test]
    fn indent_and_outdent_lines() {
        let (t, sel) = run("a\nb\n", Selection::new(0, 3), |t, s| indent(t, s, "  "));
        assert_eq!(t, "  a\n  b\n");
        assert_eq!(&t[sel.range()], "a\n  b");
        let (t, _) = run(&t, Selection::new(0, t.len()), |t, s| outdent(t, s, "  "));
        assert_eq!(t, "a\nb\n");
        let (t, sel) = run("ab", Selection::caret(1), |t, s| indent(t, s, "    "));
        assert_eq!(t, "a   b");
        assert_eq!(sel, Selection::caret(4));
    }

    #[test]
    fn comments_toggle() {
        let (t, _) = run("  a\n  b", Selection::new(0, 6), |t, s| {
            toggle_comment(t, s, "//")
        });
        assert_eq!(t, "  // a\n  // b");
        let (t, _) = run(&t, Selection::new(0, t.len()), |t, s| {
            toggle_comment(t, s, "//")
        });
        assert_eq!(t, "  a\n  b");
    }

    #[test]
    fn move_duplicate_delete() {
        let mut t = "1\n2\n3".to_owned();
        let e = move_lines(&t, Selection::caret(2), -1).unwrap();
        e.apply(&mut t);
        assert_eq!(t, "2\n1\n3");
        assert_eq!(e.selection, Selection::caret(0));
        let e = move_lines(&t, Selection::caret(0), 1).unwrap();
        e.apply(&mut t);
        assert_eq!(t, "1\n2\n3");
        assert!(move_lines(&t, Selection::caret(4), 1).is_none());
        let (d, _) = run("a\nb", Selection::caret(0), duplicate_lines);
        assert_eq!(d, "a\na\nb");
        let (d, _) = run("a\nb", Selection::caret(3), delete_lines);
        assert_eq!(d, "a");
    }

    #[test]
    fn enter_continues_markdown_lists() {
        let (t, sel) = run("- one", Selection::caret(5), |t, s| {
            newline(t, s, true, "  ")
        });
        assert_eq!(t, "- one\n- ");
        assert_eq!(sel, Selection::caret(8));
        let (t, _) = run(&t, Selection::caret(8), |t, s| newline(t, s, true, "  "));
        assert_eq!(t, "- one\n");
        let (t, _) = run("9. x", Selection::caret(4), |t, s| {
            newline(t, s, true, "  ")
        });
        assert_eq!(t, "9. x\n10. ");
        let (t, _) = run("  - [x] done", Selection::caret(12), |t, s| {
            newline(t, s, true, "  ")
        });
        assert_eq!(t, "  - [x] done\n  - [ ] ");
        let (t, _) = run("> q", Selection::caret(3), |t, s| newline(t, s, true, "  "));
        assert_eq!(t, "> q\n> ");
        let (t, sel) = run("fn x() {}", Selection::caret(8), |t, s| {
            newline(t, s, false, "    ")
        });
        assert_eq!(t, "fn x() {\n    \n}");
        assert_eq!(sel, Selection::caret(13));
    }

    #[test]
    fn wrap_and_unwrap() {
        let (t, sel) = run("a word b", Selection::new(2, 6), |t, s| {
            toggle_wrap(t, s, "**", "**")
        });
        assert_eq!(t, "a **word** b");
        let (t, _) = run(&t, sel, |t, s| toggle_wrap(t, s, "**", "**"));
        assert_eq!(t, "a word b");
        let (t, _) = run("a word b", Selection::caret(4), |t, s| {
            toggle_wrap(t, s, "*", "*")
        });
        assert_eq!(t, "a *word* b");
        let (t, sel) = run("", Selection::caret(0), |t, s| toggle_wrap(t, s, "`", "`"));
        assert_eq!((t.as_str(), sel), ("``", Selection::caret(1)));
        let (t, sel) = run("x", Selection::new(0, 1), insert_link);
        assert_eq!(t, "[x](url)");
        assert_eq!(&t[sel.range()], "url");
    }

    #[test]
    fn blocks_switch_and_toggle_back() {
        let (t, _) = run("a\nb", Selection::new(0, 3), |t, s| {
            set_block(t, s, Block::Numbered)
        });
        assert_eq!(t, "1. a\n2. b");
        let (t, _) = run(&t, Selection::new(0, t.len()), |t, s| {
            set_block(t, s, Block::Heading(2))
        });
        assert_eq!(t, "## a\n## b");
        let (t, _) = run(&t, Selection::new(0, t.len()), |t, s| {
            set_block(t, s, Block::Heading(2))
        });
        assert_eq!(t, "a\nb");
        let (t, _) = run("- x", Selection::caret(0), |t, s| {
            set_block(t, s, Block::Task)
        });
        assert_eq!(t, "- [ ] x");
        let mut t = t;
        toggle_task(&t, 2).unwrap().apply(&mut t);
        assert_eq!(t, "- [x] x");
    }

    #[test]
    fn find_respects_case_and_words() {
        let text = "Foo foo FOO food İx";
        assert_eq!(find_all(text, "foo", true, false), vec![4..7, 12..15]);
        assert_eq!(find_all(text, "foo", false, false).len(), 4);
        assert_eq!(find_all(text, "foo", false, true).len(), 3);
        assert_eq!(find_all("中文中文", "文", true, false), vec![3..6, 9..12]);
        assert!(find_all(text, "", false, false).is_empty());
    }

    #[test]
    fn json_formatting_keeps_order_and_text() {
        let pretty = format_json(
            "{\"b\":1,\"a\":[1, 2.50,{}],\"s\":\"x, {y}\\\"\"}",
            Some("  "),
        )
        .unwrap();
        assert_eq!(
            pretty,
            "{\n  \"b\": 1,\n  \"a\": [\n    1,\n    2.50,\n    {}\n  ],\n  \"s\": \"x, {y}\\\"\"\n}"
        );
        assert_eq!(
            format_json(&pretty, None).unwrap(),
            "{\"b\":1,\"a\":[1,2.50,{}],\"s\":\"x, {y}\\\"\"}"
        );
        assert!(
            format_json("{\"a\":}", None)
                .unwrap_err()
                .contains("第 1 行")
        );
    }

    #[test]
    fn line_helpers() {
        let t = "ab\n中c\n";
        assert_eq!(line_column(t, 6), (2, 2));
        assert_eq!(offset_of_line(t, 2), 3);
        assert_eq!(offset_of_line(t, 9), t.len());
        assert_eq!(detect_indent("a\n  b\n    c"), "  ");
        assert_eq!(detect_indent("a\n\tb"), "\t");
    }
}
