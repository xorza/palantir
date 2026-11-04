//! Semantic state for the host-owned text buffer: caret, selection, undo history, and the
//! identity that tells a local edit from a host replacement.

use crate::widgets::text_edit::history::History;
use std::ops;
use unicode_segmentation::GraphemeCursor;

/// Semantic state for the host-owned text buffer.
#[derive(Clone, Default, Debug)]
pub(super) struct EditState {
    pub(super) caret: usize,
    /// Selection anchor; never `Some(caret)`, so "selection live" is one `is_some()`.
    pub(super) selection: Option<usize>,
    /// Byte the in-flight drag-select started from. Lives here so `normalize`
    /// repairs it with `caret` and `selection`.
    pub(super) drag_anchor: Option<usize>,
    /// The press that focused the field selects everything on its release, unless it
    /// selected something itself (a drag, a double click).
    pub(super) select_all_on_release: bool,
    /// The x a run of vertical moves aims at, so a pass through a short line does not drag the
    /// column left, as every editor keeps a goal column (cosmic-text's `Cursor::x_opt`, a
    /// browser's `<textarea>`).
    pub(super) goal: Option<GoalColumn>,
    pub(super) history: History,
    /// The buffer as the history last saw it, kept equal to it through every local edit by the
    /// same splice, so a mismatch is exactly a host replacement. Refused: a hash of the buffer,
    /// whose collision kept an undo history across a replacement with no sign; an edit
    /// generation, which needs a buffer type of palantir's in place of the host's `String`. An
    /// exact copy is what egui's `Undoer` compares too.
    pub(super) seen: String,
    /// The buffer's characters, counted once a cap asks and kept exact by each edit's
    /// delta; `None` until then and after a host replacement.
    pub(super) char_count: Option<usize>,
}

/// The x a vertical move aims at, and the caret it left: a caret anything else moved, or an edit,
/// ends the run.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct GoalColumn {
    pub(super) x: f32,
    pub(super) caret: usize,
}

impl EditState {
    /// The goal x of the run of vertical moves the caret is in, or `None` outside one.
    pub(super) fn goal_x(&self) -> Option<f32> {
        self.goal
            .filter(|goal| goal.caret == self.caret)
            .map(|goal| goal.x)
    }

    /// Before an edit: wipe the history if the host replaced the buffer since the history last
    /// saw it, then see the buffer as it is.
    pub(super) fn reconcile(&mut self, text: &str) {
        if self.seen != text {
            self.history.clear();
            self.char_count = None;
            self.seen.clear();
            self.seen.push_str(text);
        }
    }

    /// After a local splice of the buffer: the same splice on what the history saw, so the two
    /// stay equal without a copy of the whole buffer.
    pub(super) fn saw_splice(&mut self, range: ops::Range<usize>, replacement: &str) {
        self.seen.replace_range(range, replacement);
    }

    pub(super) fn selection_range(&self) -> Option<ops::Range<usize>> {
        let a = self.selection?;
        Some(a.min(self.caret)..a.max(self.caret))
    }

    fn repair_offset(text: &str, offset: usize) -> usize {
        let mut offset = offset.min(text.len());
        while !text.is_char_boundary(offset) {
            offset -= 1;
        }
        let mut cursor = GraphemeCursor::new(offset, text.len(), true);
        match cursor.is_boundary(text, 0) {
            Ok(true) => offset,
            _ => cursor.prev_boundary(text, 0).ok().flatten().unwrap_or(0),
        }
    }

    /// Repair persisted byte offsets against the host-owned buffer: clamp to `len`,
    /// walk back to a grapheme start, then collapse an empty selection.
    pub(super) fn normalize(&mut self, text: &str) {
        self.caret = Self::repair_offset(text, self.caret);
        self.selection = self
            .selection
            .map(|offset| Self::repair_offset(text, offset));
        self.drag_anchor = self
            .drag_anchor
            .map(|offset| Self::repair_offset(text, offset));
        if self.selection == Some(self.caret) {
            self.selection = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::widgets::text_edit::edit_state::EditState;

    /// A caret inside a grapheme cluster or code point moves to its start; past the end it clamps.
    #[test]
    fn normalize_repairs_offsets_to_grapheme_starts() {
        for (text, offset, repaired) in [
            ("e\u{301}", 1, 0),
            ("ae\u{301}b", 2, 1),
            ("\u{1f44d}\u{1f3fd}", 4, 0),
            ("é", 1, 0),
            ("abc", 9, 3),
            ("abc", 2, 2),
        ] {
            let mut state = EditState {
                caret: offset,
                ..EditState::default()
            };
            state.normalize(text);
            assert_eq!(state.caret, repaired, "{text:?} at {offset}");
        }
    }
}
