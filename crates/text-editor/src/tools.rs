//! More text commands, as in VS Code and Obsidian: sorting and de-duplicating lines, changing
//! case, cleaning whitespace, converting indentation, joining lines, and the Markdown inserts
//! for footnotes, callouts, math, images and the properties (front matter) block.
//!
//! Each returns an [`Edit`]; `None` means there is nothing to change.

use crate::edit::{Edit, Selection, line_end, line_start, lines_of};
use std::ops::Range;

/// The lines a whole-line command works on: the selected lines, or every line when nothing is
/// selected.
fn target_lines(text: &str, selection: Selection) -> Range<usize> {
    if selection.is_empty() {
        let end = text.strip_suffix('\n').map_or(text.len(), str::len);
        0..end
    } else {
        lines_of(text, selection)
    }
}

fn replace_lines(range: Range<usize>, lines: Vec<&str>, old: &str) -> Option<Edit> {
    let joined = lines.join("\n");
    (joined != old).then(|| Edit {
        selection: Selection::new(range.start, range.start + joined.len()),
        range,
        text: joined,
    })
}

/// Sort lines in natural order (`item 2` before `item 10`), ignoring case.
pub fn sort_lines(text: &str, selection: Selection, descending: bool) -> Option<Edit> {
    let range = target_lines(text, selection);
    let old = &text[range.clone()];
    let mut lines: Vec<&str> = old.split('\n').collect();
    lines.sort_by(|a, b| natural_cmp(a, b));
    if descending {
        lines.reverse();
    }
    replace_lines(range, lines, old)
}

/// Keep the first of each run of equal lines, wherever they are.
pub fn unique_lines(text: &str, selection: Selection) -> Option<Edit> {
    let range = target_lines(text, selection);
    let old = &text[range.clone()];
    let mut seen = std::collections::HashSet::new();
    let lines: Vec<&str> = old.split('\n').filter(|line| seen.insert(*line)).collect();
    replace_lines(range, lines, old)
}

pub fn reverse_lines(text: &str, selection: Selection) -> Option<Edit> {
    let range = target_lines(text, selection);
    let old = &text[range.clone()];
    let lines: Vec<&str> = old.split('\n').rev().collect();
    replace_lines(range, lines, old)
}

/// Compare with runs of digits as numbers and letters ignoring case; exact text breaks ties.
fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    natural_cmp_folded(a, b).then_with(|| a.cmp(b))
}

fn natural_cmp_folded(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut digits = String::new();
                    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
                        digits.push(c);
                        it.next();
                    }
                    digits
                };
                let (x, y) = (take(&mut a), take(&mut b));
                let (tx, ty) = (x.trim_start_matches('0'), y.trim_start_matches('0'));
                let order = tx.len().cmp(&ty.len()).then_with(|| tx.cmp(ty));
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some(x), Some(y)) => {
                let order = x.to_lowercase().cmp(y.to_lowercase());
                if order != Ordering::Equal {
                    return order;
                }
                a.next();
                b.next();
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Case {
    Upper,
    Lower,
    /// The first letter of every word upper case.
    Title,
}

/// Change the case of the selection, or of the word at the caret.
pub fn change_case(text: &str, selection: Selection, case: Case) -> Option<Edit> {
    let mut range = selection.range();
    if range.is_empty() {
        let word = |c: char| c.is_alphanumeric() || c == '_';
        range.start = text[..range.start]
            .char_indices()
            .rev()
            .take_while(|(_, c)| word(*c))
            .last()
            .map_or(range.start, |(i, _)| i);
        range.end += text[range.end..]
            .char_indices()
            .take_while(|(_, c)| word(*c))
            .last()
            .map_or(0, |(i, c)| i + c.len_utf8());
    }
    let old = &text[range.clone()];
    let new = match case {
        Case::Upper => old.to_uppercase(),
        Case::Lower => old.to_lowercase(),
        Case::Title => {
            let mut out = String::with_capacity(old.len());
            let mut start = true;
            for c in old.chars() {
                if start && c.is_alphabetic() {
                    out.extend(c.to_uppercase());
                } else {
                    out.extend(c.to_lowercase());
                }
                start = !(c.is_alphanumeric() || c == '\'' || c == '’');
            }
            out
        }
    };
    (new != old).then(|| Edit {
        selection: Selection::new(range.start, range.start + new.len()),
        range,
        text: new,
    })
}

/// Map an offset in `old` to `new` when only whitespace inside lines changed: same line, the
/// same column clamped to the new line's length.
fn same_spot(old: &str, new: &str, at: usize) -> usize {
    let line = old[..at].matches('\n').count();
    let column = at - line_start(old, at);
    let start = if line == 0 {
        0
    } else {
        new.match_indices('\n')
            .nth(line - 1)
            .map_or(new.len(), |(i, _)| i + 1)
    };
    (start + column).min(line_end(new, start.min(new.len())))
}

fn whole_text(text: &str, new: String, selection: Selection) -> Option<Edit> {
    (new != text).then(|| Edit {
        selection: Selection::new(
            same_spot(text, &new, selection.anchor),
            same_spot(text, &new, selection.head),
        ),
        range: 0..text.len(),
        text: new,
    })
}

/// Remove spaces and tabs at the ends of lines. In Markdown, two trailing spaces are a line
/// break, so `keep_breaks` leaves exactly-two-space endings alone.
pub fn trim_trailing(text: &str, selection: Selection, keep_breaks: bool) -> Option<Edit> {
    let lines: Vec<String> = text
        .split('\n')
        .map(|line| {
            let (body, cr) = match line.strip_suffix('\r') {
                Some(body) => (body, "\r"),
                None => (line, ""),
            };
            let trimmed = body.trim_end_matches([' ', '\t']);
            let kept = if keep_breaks && !trimmed.is_empty() && &body[trimmed.len()..] == "  " {
                body
            } else {
                trimmed
            };
            format!("{kept}{cr}")
        })
        .collect();
    whole_text(text, lines.join("\n"), selection)
}

/// Convert leading indentation between tabs and runs of `width` spaces.
pub fn convert_indent(
    text: &str,
    selection: Selection,
    to_tabs: bool,
    width: usize,
) -> Option<Edit> {
    let width = width.max(1);
    let new: Vec<String> = text
        .split('\n')
        .map(|line| {
            let lead = line.len() - line.trim_start_matches([' ', '\t']).len();
            let (indent, rest) = line.split_at(lead);
            let mut columns = 0;
            for c in indent.chars() {
                columns = if c == '\t' {
                    (columns / width + 1) * width
                } else {
                    columns + 1
                };
            }
            let indent = if to_tabs {
                format!(
                    "{}{}",
                    "\t".repeat(columns / width),
                    " ".repeat(columns % width)
                )
            } else {
                " ".repeat(columns)
            };
            format!("{indent}{rest}")
        })
        .collect();
    whole_text(text, new.join("\n"), selection)
}

/// Join the selected lines (or this line and the next) with single spaces.
pub fn join_lines(text: &str, selection: Selection) -> Option<Edit> {
    let mut range = lines_of(text, selection);
    if !range.clone().any(|i| text.as_bytes()[i] == b'\n') {
        if range.end >= text.len() {
            return None;
        }
        range.end = line_end(text, range.end + 1);
    }
    let old = &text[range.clone()];
    let mut joined = String::new();
    for (index, line) in old.split('\n').enumerate() {
        let line = if index == 0 {
            line.trim_end()
        } else {
            line.trim()
        };
        if index > 0 && !line.is_empty() && !joined.is_empty() {
            joined.push(' ');
        }
        joined.push_str(line);
    }
    let caret = range.start + joined.len();
    Some(Edit {
        range,
        text: joined,
        selection: Selection::caret(caret),
    })
}

/// Insert a footnote reference `[^n]` at the caret and its definition at the end of the text,
/// with the caret in the definition.
pub fn insert_footnote(text: &str, selection: Selection) -> Edit {
    let mut n = 1;
    while text.contains(&format!("[^{n}]")) {
        n += 1;
    }
    let range = selection.range();
    let reference = format!("[^{n}]");
    let mut new = String::with_capacity(text.len() + 16);
    new.push_str(&text[..range.start]);
    new.push_str(&reference);
    new.push_str(&text[range.end..]);
    let last_line = &new[line_start(&new, new.len())..];
    let tail = if last_line.starts_with("[^") {
        "\n"
    } else if new.ends_with("\n\n") {
        ""
    } else if new.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    new.push_str(tail);
    new.push_str(&format!("[^{n}]: "));
    let caret = new.len();
    Edit {
        range: 0..text.len(),
        text: new,
        selection: Selection::caret(caret),
    }
}

/// Turn the selected lines into an Obsidian callout (`> [!note] Title`), or insert an empty one.
pub fn insert_callout(text: &str, selection: Selection, kind: &str, title: &str) -> Edit {
    let head = format!("> [!{kind}] {title}").trim_end().to_owned();
    if selection.is_empty() {
        let at = selection.head;
        let lead = if line_start(text, at) == at { "" } else { "\n" };
        let block = format!("{lead}{head}\n> ");
        let caret = at + block.len();
        let trail = if text[at..].starts_with('\n') || at == text.len() {
            ""
        } else {
            "\n"
        };
        return Edit {
            range: at..at,
            text: format!("{block}{trail}"),
            selection: Selection::caret(caret),
        };
    }
    let range = lines_of(text, selection);
    let quoted: Vec<String> = text[range.clone()]
        .split('\n')
        .map(|line| {
            if line.is_empty() {
                ">".into()
            } else {
                format!("> {line}")
            }
        })
        .collect();
    let block = format!("{head}\n{}", quoted.join("\n"));
    Edit {
        selection: Selection::caret(range.start + block.len()),
        range,
        text: block,
    }
}

/// `$$` block math on its own lines, with the caret inside.
pub fn insert_math_block(text: &str, selection: Selection) -> Edit {
    let range = selection.range();
    let inner = &text[range.clone()];
    let lead = if line_start(text, range.start) == range.start {
        ""
    } else {
        "\n"
    };
    let block = format!("{lead}$$\n{inner}\n$$\n");
    let caret = range.start + lead.len() + 3 + inner.len();
    Edit {
        range,
        text: block,
        selection: Selection::caret(caret),
    }
}

/// `![alt](target)`: the selection becomes the target, or a placeholder to type over.
pub fn insert_image(text: &str, selection: Selection, placeholder: &str) -> Edit {
    let range = selection.range();
    let selected = &text[range.clone()];
    let target = if selected.is_empty() {
        placeholder
    } else {
        selected
    };
    let image = format!("![]({target})");
    let start = range.start + 4;
    Edit {
        range,
        text: image,
        selection: Selection::new(start, start + target.len()),
    }
}

/// The properties block at the top of a note (`---` YAML `---`). Inserts one with `title`,
/// `tags` and `date` if there is none, and puts the caret in it either way.
pub fn properties(text: &str, title: &str, date: &str) -> Edit {
    if let Some(rest) = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    {
        let open = text.len() - rest.len();
        let end = rest.find("\n---").map_or(open, |i| open + i);
        return Edit {
            range: end..end,
            text: String::new(),
            selection: Selection::caret(end),
        };
    }
    let block = format!("---\ntitle: {title}\ntags: \ndate: {date}\n---\n");
    let caret = block.find("tags: ").map_or(0, |i| i + 6);
    Edit {
        range: 0..0,
        text: block,
        selection: Selection::caret(caret),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str, edit: Option<Edit>) -> String {
        let mut text = text.to_owned();
        if let Some(edit) = edit {
            edit.apply(&mut text);
        }
        text
    }

    #[test]
    fn sorts_naturally_and_keeps_the_final_newline() {
        let text = "item 10\nItem 2\nitem 1\n";
        let all = Selection::caret(0);
        assert_eq!(
            run(text, sort_lines(text, all, false)),
            "item 1\nItem 2\nitem 10\n"
        );
        assert_eq!(
            run(text, sort_lines(text, all, true)),
            "item 10\nItem 2\nitem 1\n"
        );
        // Only the selected lines.
        let text = "c\nb\na\nz\n";
        assert_eq!(
            run(text, sort_lines(text, Selection::new(0, 5), false)),
            "a\nb\nc\nz\n"
        );
        assert!(sort_lines("a\nb", all, false).is_none());
    }

    #[test]
    fn removes_duplicates_and_reverses() {
        let text = "a\nb\na\nc\nb";
        let all = Selection::caret(0);
        assert_eq!(run(text, unique_lines(text, all)), "a\nb\nc");
        assert_eq!(run(text, reverse_lines(text, all)), "b\nc\na\nb\na");
    }

    #[test]
    fn changes_case_of_selection_or_word() {
        let text = "hello wORLD of rust";
        assert_eq!(
            run(text, change_case(text, Selection::caret(8), Case::Upper)),
            "hello WORLD of rust"
        );
        assert_eq!(
            run(
                text,
                change_case(text, Selection::new(0, text.len()), Case::Title)
            ),
            "Hello World Of Rust"
        );
        assert_eq!(
            run(text, change_case(text, Selection::new(6, 11), Case::Lower)),
            "hello world of rust"
        );
        assert!(change_case("abc", Selection::caret(1), Case::Lower).is_none());
    }

    #[test]
    fn trims_trailing_space_but_keeps_markdown_breaks_and_crlf() {
        let text = "a  \nb \t\nc\r\nd \r\n";
        assert_eq!(
            run(text, trim_trailing(text, Selection::caret(0), false)),
            "a\nb\nc\r\nd\r\n"
        );
        assert_eq!(
            run(text, trim_trailing(text, Selection::caret(0), true)),
            "a  \nb\nc\r\nd\r\n"
        );
        // The caret stays on its line.
        let edit = trim_trailing("x   \ny", Selection::caret(3), false).unwrap();
        assert_eq!(edit.selection, Selection::caret(1));
    }

    #[test]
    fn converts_indentation_both_ways() {
        let text = "\tif x:\n\t\ty\n      z";
        let spaces = run(text, convert_indent(text, Selection::caret(0), false, 4));
        assert_eq!(spaces, "    if x:\n        y\n      z");
        assert_eq!(
            run(
                &spaces,
                convert_indent(&spaces, Selection::caret(0), true, 4)
            ),
            "\tif x:\n\t\ty\n\t  z"
        );
    }

    #[test]
    fn joins_lines() {
        let text = "one\n   two\nthree";
        assert_eq!(
            run(text, join_lines(text, Selection::caret(1))),
            "one two\nthree"
        );
        assert_eq!(
            run(text, join_lines(text, Selection::new(0, text.len()))),
            "one two three"
        );
        assert!(join_lines("last", Selection::caret(1)).is_none());
    }

    #[test]
    fn footnotes_number_themselves() {
        let text = "Claim.";
        let edit = insert_footnote(text, Selection::caret(6));
        let once = run(text, Some(edit.clone()));
        assert_eq!(once, "Claim.[^1]\n\n[^1]: ");
        assert_eq!(edit.selection, Selection::caret(once.len()));
        let twice = run(&once, Some(insert_footnote(&once, Selection::caret(0))));
        assert!(twice.starts_with("[^2]Claim.[^1]"));
        assert!(twice.ends_with("[^1]: \n[^2]: "));
    }

    #[test]
    fn callouts_wrap_lines_or_start_empty() {
        let text = "one\n\ntwo";
        assert_eq!(
            run(
                text,
                Some(insert_callout(
                    text,
                    Selection::new(0, text.len()),
                    "tip",
                    ""
                ))
            ),
            "> [!tip]\n> one\n>\n> two"
        );
        let edit = insert_callout("", Selection::caret(0), "note", "标题");
        assert_eq!(edit.text, "> [!note] 标题\n> ");
    }

    #[test]
    fn properties_are_added_once() {
        let edit = properties("# Note", "Note", "2026-11-02");
        let text = run("# Note", Some(edit.clone()));
        assert!(text.starts_with("---\ntitle: Note\ntags: \ndate: 2026-11-02\n---\n# Note"));
        assert_eq!(
            &text[edit.selection.head - 6..edit.selection.head],
            "tags: "
        );
        let again = properties(&text, "Note", "x");
        assert!(again.text.is_empty());
        assert_eq!(
            &text[again.selection.head..again.selection.head + 4],
            "\n---"
        );
    }

    #[test]
    fn images_and_math() {
        let edit = insert_image("a.png", Selection::new(0, 5), "图片");
        assert_eq!(edit.text, "![](a.png)");
        let edit = insert_image("", Selection::caret(0), "图片");
        assert_eq!(&edit.text[4..edit.selection.head], "图片");
        let edit = insert_math_block("x", Selection::caret(1));
        assert_eq!(edit.text, "\n$$\n\n$$\n");
        assert_eq!(edit.selection, Selection::caret(1 + 4));
    }
}
