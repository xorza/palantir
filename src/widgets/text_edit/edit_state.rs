//! Semantic state for the host-owned text buffer, plus the undo
//! history it retains. [`Editor`](super::editor::Editor) is the
//! behaviour over this data.

use std::collections::VecDeque;

/// Semantic state for the host-owned text buffer.
///
/// `caret` is a byte offset. Widget-driven mutations step grapheme
/// boundaries, while [`Self::normalize`] repairs offsets after the host
/// replaces the buffer between frames.
#[derive(Clone, Default, Debug)]
pub(super) struct EditState {
    pub(super) caret: usize,
    /// Selection anchor. `None` = no selection. Invariant: never
    /// `Some(caret)` — every mutation site collapses an empty selection
    /// to `None` so "selection live" is a single `is_some()` check.
    pub(super) selection: Option<usize>,
    pub(super) undo: VecDeque<EditDelta>,
    pub(super) redo: Vec<EditDelta>,
    /// Kind of the most recent recorded edit, used to coalesce
    /// consecutive same-kind edits (typing chars, deleting chars) into
    /// a single undo unit. `None` after any caret-only motion so the
    /// next edit always opens a fresh group.
    pub(super) last_edit_kind: Option<EditKind>,
    pub(super) expected_hash: Option<u64>,
    pub(super) local_edit_pending: bool,
    pub(super) char_count: Option<usize>,
}

/// Caret + anchor as one comparable unit. An [`EditDelta`] stores the
/// pair either side of its edit so undo/redo restores the selection the
/// user had, and so `coalesce` can check the two deltas actually abut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SelectionState {
    pub(super) caret: usize,
    pub(super) selection: Option<usize>,
}

/// One undoable buffer edit. Fields are `pub(super)` because
/// [`Editor`](super::editor::Editor) is what builds and replays them —
/// the data lives here with the stacks that retain it, the behaviour
/// lives there.
#[derive(Clone, Debug)]
pub(super) struct EditDelta {
    pub(super) start: usize,
    pub(super) removed: String,
    pub(super) inserted: String,
    pub(super) before: SelectionState,
    pub(super) after: SelectionState,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum EditKind {
    Typing,
    Delete,
    /// Bulk edits (paste, cut, clear, newline insert) — never coalesce.
    Other,
}

/// Cap on retained undo entries; `Editor::push_delta` drops the
/// oldest past this.
pub(super) const UNDO_LIMIT: usize = 128;

impl EditDelta {
    pub(super) fn coalesce(&mut self, next: &Self, kind: EditKind) -> bool {
        if self.after != next.before {
            return false;
        }
        let merged = match kind {
            EditKind::Typing
                if next.removed.is_empty() && next.start == self.start + self.inserted.len() =>
            {
                self.inserted.push_str(&next.inserted);
                true
            }
            EditKind::Delete
                if self.inserted.is_empty()
                    && next.inserted.is_empty()
                    && next.start + next.removed.len() == self.start =>
            {
                self.start = next.start;
                self.removed.insert_str(0, &next.removed);
                true
            }
            EditKind::Delete
                if self.inserted.is_empty()
                    && next.inserted.is_empty()
                    && next.start == self.start =>
            {
                self.removed.push_str(&next.removed);
                true
            }
            EditKind::Typing | EditKind::Delete | EditKind::Other => false,
        };
        if merged {
            self.after = next.after;
        }
        merged
    }
}

impl EditState {
    pub(super) fn observe_text_hash(&mut self, text_hash: u64) {
        if !self.local_edit_pending
            && self
                .expected_hash
                .is_some_and(|expected| expected != text_hash)
        {
            self.undo.clear();
            self.redo.clear();
            self.last_edit_kind = None;
            self.char_count = None;
        }
        self.expected_hash = Some(text_hash);
        self.local_edit_pending = false;
    }

    pub(super) fn sel_range(&self) -> Option<std::ops::Range<usize>> {
        let a = self.selection?;
        Some(a.min(self.caret)..a.max(self.caret))
    }

    pub(super) fn repair_offset(text: &str, offset: usize) -> usize {
        let mut offset = offset.min(text.len());
        while !text.is_char_boundary(offset) {
            offset -= 1;
        }
        offset
    }

    /// Repair every persisted byte offset against the current host-owned
    /// buffer. Offsets beyond the end clamp to `len`; offsets inside a
    /// UTF-8 code point walk backward to its start (at most three bytes).
    /// Then collapse an empty selection. Safe both before input, when the
    /// application may have replaced the buffer, and after our mutations.
    pub(super) fn normalize(&mut self, text: &str) {
        self.caret = Self::repair_offset(text, self.caret);
        self.selection = self
            .selection
            .map(|offset| Self::repair_offset(text, offset));
        if self.selection == Some(self.caret) {
            self.selection = None;
        }
    }
}
