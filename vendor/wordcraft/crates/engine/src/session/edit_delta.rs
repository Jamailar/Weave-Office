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
        before.same_except_paragraph(&self.doc, &at.path).then(|| TextEdit {
            document: self.document_id(),
            after: self.rev(),
            previous,
            path: at.path.clone(),
            previous_paragraph: old.rev,
            removed_words: count_words(&old.text),
            added_words: count_words(&new.text),
        })
    }
}
