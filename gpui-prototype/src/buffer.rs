//! The document text on a rope, with undo and redo. Offsets are byte offsets into LF text, like
//! the rest of the editor; the rope makes edits and line lookups cost O(log n) at any file size.

use std::ops::Range;
use std::time::{Duration, Instant};

use ropey::Rope;
use unicode_segmentation::UnicodeSegmentation;

/// Consecutive typing or deleting within this pause is undone as one step.
const GROUP_PAUSE: Duration = Duration::from_millis(1000);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditKind {
    /// Characters typed at the caret. A new line ends the group.
    Typing,
    /// Backspace or Delete with nothing selected.
    Deleting,
    /// Paste, cut, replace, a deleted selection: always its own step.
    Other,
}

#[derive(Clone, Debug)]
struct Edit {
    at: usize,
    old: String,
    new: String,
}

#[derive(Debug)]
struct Transaction {
    id: u64,
    edits: Vec<Edit>,
    kind: EditKind,
    selection_before: Range<usize>,
    selection_after: Range<usize>,
    last_edit_at: Instant,
}

/// Lines `first..first + old` were replaced by `new` lines. The source list is spliced with this.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineSplice {
    pub first: usize,
    pub old: usize,
    pub new: usize,
}

pub struct Buffer {
    rope: Rope,
    undo: Vec<Transaction>,
    redo: Vec<Transaction>,
    next_id: u64,
    /// Id of the newest undo step when the file was last saved; `None` when the history was empty.
    saved: Option<u64>,
    /// Changes on every edit, undo and redo, so the preview and search know their results are stale.
    version: u64,
}

impl Buffer {
    pub fn new(text: &str) -> Self {
        Self { rope: Rope::from_str(text), undo: Vec::new(), redo: Vec::new(), next_id: 1, saved: None, version: 0 }
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn len(&self) -> usize {
        self.rope.len_bytes()
    }

    /// A cheap copy (the rope shares its chunks), for parsing on a background thread.
    pub fn snapshot(&self) -> Rope {
        self.rope.clone()
    }

    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    pub fn slice(&self, range: Range<usize>) -> String {
        let range = self.clamp(range);
        self.rope.byte_slice(range).to_string()
    }

    pub fn line_count(&self) -> usize {
        self.rope.len_lines()
    }

    /// Line `index` without its newline.
    pub fn line(&self, index: usize) -> String {
        let line = self.rope.line(index.min(self.line_count() - 1));
        let text = line.to_string();
        text.strip_suffix('\n').map(str::to_string).unwrap_or(text)
    }

    pub fn line_start(&self, index: usize) -> usize {
        if index >= self.line_count() {
            return self.len();
        }
        self.rope.line_to_byte(index)
    }

    /// Byte range of line `index`, without its newline.
    pub fn line_range(&self, index: usize) -> Range<usize> {
        let start = self.line_start(index);
        start..start + self.line(index).len()
    }

    pub fn line_of(&self, offset: usize) -> usize {
        self.rope.byte_to_line(self.boundary(offset))
    }

    /// The nearest char boundary at or before `offset`. Every offset that slices the text passes through here.
    pub fn boundary(&self, offset: usize) -> usize {
        let offset = offset.min(self.len());
        self.rope.char_to_byte(self.rope.byte_to_char(offset))
    }

    fn clamp(&self, range: Range<usize>) -> Range<usize> {
        let start = self.boundary(range.start);
        start..self.boundary(range.end).max(start)
    }

    pub fn utf16_of(&self, offset: usize) -> usize {
        self.rope.char_to_utf16_cu(self.rope.byte_to_char(self.boundary(offset)))
    }

    pub fn offset_of_utf16(&self, offset: usize) -> usize {
        let offset = offset.min(self.rope.len_utf16_cu());
        self.rope.char_to_byte(self.rope.utf16_cu_to_char(offset))
    }

    /// Graphemes never span a newline in LF text, so stepping needs only the current line.
    pub fn previous_grapheme(&self, offset: usize) -> usize {
        let offset = self.boundary(offset);
        let line = self.line_of(offset);
        let start = self.line_start(line);
        if offset == start {
            return offset.saturating_sub(1);
        }
        let text = self.line(line);
        text.grapheme_indices(true).rev().map(|(index, _)| start + index).find(|index| *index < offset).unwrap_or(start)
    }

    pub fn next_grapheme(&self, offset: usize) -> usize {
        let offset = self.boundary(offset);
        let line = self.line_of(offset);
        let range = self.line_range(line);
        if offset >= range.end {
            return (offset + 1).min(self.len());
        }
        let text = self.line(line);
        text.grapheme_indices(true).map(|(index, _)| range.start + index).find(|index| *index > offset).unwrap_or(range.end)
    }

    /// Up or Down: the same column, counted in graphemes, on the line above or below.
    pub fn vertical_target(&self, offset: usize, delta: isize) -> usize {
        let offset = self.boundary(offset);
        let line = self.line_of(offset);
        if delta < 0 && line == 0 {
            return 0;
        }
        if delta > 0 && line + 1 >= self.line_count() {
            return self.len();
        }
        let start = self.line_start(line);
        let column = self.slice(start..offset).graphemes(true).count();
        let target = if delta < 0 { line - 1 } else { line + 1 };
        let range = self.line_range(target);
        self.line(target).grapheme_indices(true).nth(column).map(|(index, _)| range.start + index).unwrap_or(range.end)
    }

    /// Replaces `range` with `new`. Returns the caret after the edit and the lines that changed.
    pub fn edit(&mut self, range: Range<usize>, new: &str, kind: EditKind, selection_before: Range<usize>) -> (usize, LineSplice) {
        let range = self.clamp(range);
        let edit = Edit { at: range.start, old: self.slice(range.clone()), new: new.to_string() };
        let splice = self.apply(range.start, edit.old.len(), new);
        let caret = range.start + new.len();
        self.redo.clear();
        let now = Instant::now();
        let joins = self.undo.last().is_some_and(|last| joins_group(last, &edit, kind, now));
        match self.undo.last_mut() {
            Some(last) if joins => {
                last.edits.push(edit);
                last.selection_after = caret..caret;
                last.last_edit_at = now;
            }
            _ => {
                let id = self.next_id;
                self.next_id += 1;
                self.undo.push(Transaction {
                    id,
                    edits: vec![edit],
                    kind,
                    selection_before,
                    selection_after: caret..caret,
                    last_edit_at: now,
                });
            }
        }
        (caret, splice)
    }

    /// Ends the current typing group, so the next keystroke starts a new undo step (after a caret move).
    pub fn break_group(&mut self) {
        if let Some(last) = self.undo.last_mut() {
            last.kind = EditKind::Other;
        }
    }

    /// Returns the selection to restore and the lines that changed, in order.
    pub fn undo(&mut self) -> Option<(Range<usize>, Vec<LineSplice>)> {
        let transaction = self.undo.pop()?;
        let splices = transaction.edits.iter().rev().map(|edit| self.apply(edit.at, edit.new.len(), &edit.old)).collect();
        let selection = transaction.selection_before.clone();
        self.redo.push(transaction);
        Some((selection, splices))
    }

    pub fn redo(&mut self) -> Option<(Range<usize>, Vec<LineSplice>)> {
        let mut transaction = self.redo.pop()?;
        let splices = transaction.edits.iter().map(|edit| self.apply(edit.at, edit.old.len(), &edit.new)).collect();
        let selection = transaction.selection_after.clone();
        // A redone step never merges with the next keystroke.
        transaction.kind = EditKind::Other;
        self.undo.push(transaction);
        Some((selection, splices))
    }

    pub fn is_dirty(&self) -> bool {
        self.undo.last().map(|step| step.id) != self.saved
    }

    pub fn mark_saved(&mut self) {
        self.saved = self.undo.last().map(|step| step.id);
        self.break_group();
    }

    fn apply(&mut self, at: usize, old_len: usize, new: &str) -> LineSplice {
        let first = self.rope.byte_to_line(at);
        let old_last = self.rope.byte_to_line(at + old_len);
        let start = self.rope.byte_to_char(at);
        let end = self.rope.byte_to_char(at + old_len);
        self.rope.remove(start..end);
        self.rope.insert(start, new);
        self.version += 1;
        let new_last = self.rope.byte_to_line(at + new.len());
        LineSplice { first, old: old_last - first + 1, new: new_last - first + 1 }
    }
}

fn joins_group(last: &Transaction, edit: &Edit, kind: EditKind, now: Instant) -> bool {
    if last.kind != kind || now.duration_since(last.last_edit_at) > GROUP_PAUSE {
        return false;
    }
    let Some(previous) = last.edits.last() else { return false };
    match kind {
        EditKind::Typing => {
            let appends = edit.old.is_empty() && edit.at == previous.at + previous.new.len();
            // An IME composition replaces the text it inserted a moment ago.
            let recomposes = edit.at == previous.at && edit.old == previous.new;
            (appends || recomposes) && !edit.new.contains('\n')
        }
        // Backspace runs left (ends where the last deletion started); Delete stays put.
        EditKind::Deleting => edit.new.is_empty() && (edit.at + edit.old.len() == previous.at || edit.at == previous.at),
        EditKind::Other => false,
    }
}

/// Every match of `query` in `text`, ASCII letters compared without case. Matches do not overlap.
pub fn find_all(text: &str, query: &str) -> Vec<Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }
    let haystack = text.as_bytes();
    let needle = query.as_bytes();
    let mut matches = Vec::new();
    let mut at = 0;
    while at + needle.len() <= haystack.len() {
        if haystack[at..at + needle.len()].eq_ignore_ascii_case(needle) && text.is_char_boundary(at) && text.is_char_boundary(at + needle.len()) {
            matches.push(at..at + needle.len());
            at += needle.len();
        } else {
            at += 1;
        }
    }
    matches
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_groups_into_one_undo_step_and_a_new_line_starts_another() {
        let mut buffer = Buffer::new("");
        for (at, ch) in ["a", "b", "c"].iter().enumerate() {
            buffer.edit(at..at, ch, EditKind::Typing, at..at);
        }
        buffer.edit(3..3, "\n", EditKind::Typing, 3..3);
        buffer.edit(4..4, "d", EditKind::Typing, 4..4);
        assert_eq!(buffer.text(), "abc\nd");
        buffer.undo();
        assert_eq!(buffer.text(), "abc");
        let (selection, _) = buffer.undo().unwrap();
        assert_eq!(buffer.text(), "");
        assert_eq!(selection, 0..0);
        buffer.redo();
        assert_eq!(buffer.text(), "abc");
    }

    #[test]
    fn an_ime_composition_is_one_undo_step() {
        let mut buffer = Buffer::new("x");
        buffer.edit(1..1, "k", EditKind::Typing, 1..1);
        buffer.edit(1..2, "か", EditKind::Typing, 1..2);
        buffer.edit(1..4, "漢", EditKind::Typing, 1..4);
        assert_eq!(buffer.text(), "x漢");
        buffer.undo();
        assert_eq!(buffer.text(), "x");
    }

    #[test]
    fn backspaces_group_and_a_caret_move_breaks_the_group() {
        let mut buffer = Buffer::new("hello");
        buffer.edit(4..5, "", EditKind::Deleting, 5..5);
        buffer.edit(3..4, "", EditKind::Deleting, 4..4);
        assert_eq!(buffer.text(), "hel");
        buffer.break_group();
        buffer.edit(2..3, "", EditKind::Deleting, 3..3);
        buffer.undo();
        assert_eq!(buffer.text(), "hel");
        buffer.undo();
        assert_eq!(buffer.text(), "hello");
    }

    #[test]
    fn undo_back_to_the_saved_state_is_clean() {
        let mut buffer = Buffer::new("x");
        assert!(!buffer.is_dirty());
        buffer.edit(1..1, "y", EditKind::Typing, 1..1);
        assert!(buffer.is_dirty());
        buffer.mark_saved();
        assert!(!buffer.is_dirty());
        buffer.edit(2..2, "z", EditKind::Typing, 2..2);
        assert!(buffer.is_dirty());
        buffer.undo();
        assert!(!buffer.is_dirty());
        buffer.undo();
        assert!(buffer.is_dirty());
    }

    #[test]
    fn edits_report_the_lines_they_replaced() {
        let mut buffer = Buffer::new("a\nb\nc");
        let (_, splice) = buffer.edit(2..3, "B\nB2", EditKind::Other, 2..3);
        assert_eq!(splice, LineSplice { first: 1, old: 1, new: 2 });
        assert_eq!(buffer.line_count(), 4);
        assert_eq!(buffer.line(2), "B2");
        let (_, splices) = buffer.undo().unwrap();
        assert_eq!(splices, vec![LineSplice { first: 1, old: 2, new: 1 }]);
    }

    #[test]
    fn caret_moves_never_split_a_character() {
        let buffer = Buffer::new("a\néx\nabc");
        assert_eq!(buffer.vertical_target(1, 1), 4);
        assert_eq!(buffer.vertical_target(buffer.len(), -1), 5);
        assert_eq!(buffer.next_grapheme(2), 4);
        assert_eq!(buffer.previous_grapheme(4), 2);
        assert_eq!(buffer.previous_grapheme(2), 1);
        assert_eq!(buffer.boundary(3), 2);
        assert_eq!(buffer.utf16_of(4), 3);
        assert_eq!(buffer.offset_of_utf16(3), 4);
    }

    #[test]
    fn find_all_ignores_ascii_case_and_does_not_overlap() {
        assert_eq!(find_all("Aaa aA", "aa"), vec![0..2, 4..6]);
        // Only ASCII letters fold: "É" is not "é".
        assert_eq!(find_all("café CAFÉ Café", "café"), vec![0..5, 12..17]);
        assert!(find_all("abc", "").is_empty());
    }
}
