use super::{Document, Path, Selection, Session, StoryRef};
use wordcraft_doc::{Paragraph, count_words};

/// A verified replacement of text in one body paragraph, between two document revisions.
#[derive(Debug)]
pub struct TextEdit {
    document: u64,
    after: u64,
    pub previous: u64,
    pub path: Path,
    pub previous_paragraph: u64,
    pub(super) changed_bytes: Option<std::ops::Range<usize>>,
    pub removed_words: usize,
    pub added_words: usize,
}

impl Session {
    pub fn text_edit(&self) -> Option<&TextEdit> {
        self.text_edit.as_ref().filter(|e| (e.document, e.after) == (self.document_id(), self.rev()))
    }

    pub(super) fn single_text_edit(&self, before: &Document, previous: u64, selection: &Selection) -> Option<TextEdit> {
        let at = &self.sel.focus;
        if at.story != StoryRef::Body
            || selection.anchor.story != at.story
            || selection.focus.story != at.story
            || selection.anchor.path != at.path
            || selection.focus.path != at.path
        {
            return None;
        }
        let (old, new) = (before.para_at(at)?, self.doc.para_at(at)?);
        let unmarked = |p: &Paragraph| {
            p.objects.is_empty() && p.runs.iter().all(|r| r.props.ins.is_none() && r.props.del.is_none() && r.props.fmt_change.is_none())
        };
        if old.text == new.text || !unmarked(old) || !unmarked(new) || old.props != new.props || old.section != new.section || old.mark != new.mark {
            return None;
        }
        // Autoformat and controls may change other content in the same command.
        if !before.same_except_paragraph(&self.doc, &at.path) {
            return None;
        }
        let prefix = selection.anchor.off.min(selection.focus.off).min(at.off);
        let unchanged = old
            .text
            .get(..prefix)
            .filter(|s| new.text.starts_with(*s))
            .zip(new.text.get(at.off..).filter(|s| old.text.get(prefix..).is_some_and(|tail| tail.ends_with(*s))));
        let (removed_words, added_words) = match unchanged {
            Some((head, tail)) => {
                // One unchanged ASCII letter/digit already qualifies its token as a
                // word. Whitespace terminates it. Either bounds the word-count delta.
                let boundary = |c: char| c.is_ascii_alphanumeric() || c.is_whitespace();
                let left = head.rfind(boundary).unwrap_or(0);
                let right = tail.find(boundary).and_then(|i| Some(i + tail.get(i..)?.chars().next()?.len_utf8())).unwrap_or(tail.len());
                (count_words(old.text.get(left..old.len() - tail.len() + right)?), count_words(new.text.get(left..at.off + right)?))
            }
            None => (count_words(&old.text), count_words(&new.text)),
        };
        let uniform = matches!((old.runs.as_slice(), new.runs.as_slice()), ([a], [b]) if a.props == b.props);
        Some(TextEdit {
            document: self.document_id(),
            after: self.rev(),
            previous,
            path: at.path.clone(),
            previous_paragraph: old.rev,
            changed_bytes: (unchanged.is_some() && uniform && old.len() > wordcraft_layout::para::MAX_PROOF_BYTES).then_some(prefix..at.off),
            removed_words,
            added_words,
        })
    }
}
