//! The undo history: buffer edits grouped as a user thinks of them, and the redo stack an
//! undo builds.

use std::collections::VecDeque;

/// Caret + anchor as one comparable unit, stored either side of an edit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SelectionState {
    pub(super) caret: usize,
    pub(super) selection: Option<usize>,
}

/// One edit as borrowed parts, so [`History::record`] can extend the open group without copying.
#[derive(Clone, Copy, Debug)]
pub(super) struct EditParts<'a> {
    pub(super) start: usize,
    pub(super) removed: &'a str,
    pub(super) inserted: &'a str,
    pub(super) before: SelectionState,
    pub(super) after: SelectionState,
}

/// One undoable buffer edit, replayed by [`Editor`](crate::widgets::text_edit::editor::Editor).
#[derive(Clone, Debug)]
pub(super) struct EditDelta {
    pub(super) start: usize,
    pub(super) removed: String,
    pub(super) inserted: String,
    pub(super) before: SelectionState,
    pub(super) after: SelectionState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EditKind {
    Typing,
    Delete,
    /// Bulk edits (paste, cut, clear, newline insert); never coalesce.
    Other,
}

/// Which way a history step replays a delta.
#[derive(Clone, Copy, Debug)]
pub(super) enum HistoryStep {
    Undo,
    Redo,
}

/// Cap on retained undo entries; the oldest is dropped past it.
const UNDO_LIMIT: usize = 128;

/// The buffer's undo and redo stacks, and the group a next edit may extend.
#[derive(Clone, Default, Debug)]
pub(super) struct History {
    undo: VecDeque<EditDelta>,
    redo: Vec<EditDelta>,
    /// Kind of the last recorded edit, for coalescing; `None` once anything but an edit
    /// of that kind came between.
    last_kind: Option<EditKind>,
}

impl EditDelta {
    /// Own the strings of `parts`.
    fn from_parts(parts: EditParts<'_>) -> Self {
        Self {
            start: parts.start,
            removed: parts.removed.to_owned(),
            inserted: parts.inserted.to_owned(),
            before: parts.before,
            after: parts.after,
        }
    }

    /// Extend this delta with an abutting edit; reports whether it did.
    fn coalesce(&mut self, next: EditParts<'_>, kind: EditKind) -> bool {
        if self.after != next.before {
            return false;
        }
        let merged = match kind {
            EditKind::Typing
                if next.removed.is_empty() && next.start == self.start + self.inserted.len() =>
            {
                self.inserted.push_str(next.inserted);
                true
            }
            EditKind::Delete
                if self.inserted.is_empty()
                    && next.inserted.is_empty()
                    && next.start + next.removed.len() == self.start =>
            {
                self.start = next.start;
                self.removed.insert_str(0, next.removed);
                true
            }
            EditKind::Delete
                if self.inserted.is_empty()
                    && next.inserted.is_empty()
                    && next.start == self.start =>
            {
                self.removed.push_str(next.removed);
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

impl History {
    /// Fold one edit in, coalescing into the open group when it abuts one of the same
    /// kind. Redo is cleared either way.
    pub(super) fn record(&mut self, parts: EditParts<'_>, kind: EditKind) {
        let coalesced = self.last_kind == Some(kind)
            && self
                .undo
                .back_mut()
                .is_some_and(|prev| prev.coalesce(parts, kind));
        if !coalesced {
            if self.undo.len() == UNDO_LIMIT {
                self.undo.pop_front();
            }
            self.undo.push_back(EditDelta::from_parts(parts));
        }
        self.redo.clear();
        self.last_kind = Some(kind);
    }

    /// Close the open group, so the next edit starts its own: caret motion, a selection,
    /// a no-op replace, a history step.
    pub(super) const fn break_group(&mut self) {
        self.last_kind = None;
    }

    /// Forget every edit: the buffer they describe is gone.
    pub(super) fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.last_kind = None;
    }

    /// Take the delta `step` replays: the newest edit for an undo, the newest undone one
    /// for a redo. The caller applies it, then hands it to [`Self::stepped`].
    pub(super) fn take(&mut self, step: HistoryStep) -> Option<EditDelta> {
        match step {
            HistoryStep::Undo => self.undo.pop_back(),
            HistoryStep::Redo => self.redo.pop(),
        }
    }

    /// File a delta [`Self::take`] gave out after it was applied: an undone edit can be
    /// redone, a redone one undone.
    pub(super) fn stepped(&mut self, step: HistoryStep, delta: EditDelta) {
        match step {
            HistoryStep::Undo => self.redo.push(delta),
            // It came off the undo stack, so the stack has room for it.
            HistoryStep::Redo => self.undo.push_back(delta),
        }
        self.last_kind = None;
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::widgets::text_edit::history::{EditKind, History};

    impl History {
        pub(crate) fn undo_len(&self) -> usize {
            self.undo.len()
        }

        pub(crate) fn redo_len(&self) -> usize {
            self.redo.len()
        }

        pub(crate) const fn last_kind(&self) -> Option<EditKind> {
            self.last_kind
        }
    }
}
