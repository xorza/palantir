//! Semantic state for the host-owned text buffer, plus its undo history.

use crate::text::probe::TextProbe;
use std::collections::VecDeque;
use std::num::NonZeroU64;
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
    pub(super) undo: VecDeque<EditDelta>,
    pub(super) redo: Vec<EditDelta>,
    /// Kind of the last recorded edit, for coalescing; `None` after caret-only motion.
    pub(super) last_edit_kind: Option<EditKind>,
    pub(super) expected_hash: Option<NonZeroU64>,
    pub(super) local_edit_pending: bool,
    pub(super) char_count: Option<usize>,
}

/// Caret + anchor as one comparable unit, stored either side of an edit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SelectionState {
    pub(super) caret: usize,
    pub(super) selection: Option<usize>,
}

/// One edit as borrowed parts, so `record_edit` can extend the open group without copying.
#[derive(Clone, Copy, Debug)]
pub(super) struct EditParts<'a> {
    pub(super) start: usize,
    pub(super) removed: &'a str,
    pub(super) inserted: &'a str,
    pub(super) before: SelectionState,
    pub(super) after: SelectionState,
}

/// One undoable buffer edit, replayed by [`Editor`](super::editor::Editor).
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
    /// Bulk edits (paste, cut, clear, newline insert); never coalesce.
    Other,
}

/// Cap on retained undo entries; the oldest is dropped past it.
const UNDO_LIMIT: usize = 128;

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

impl EditState {
    /// Fold one edit into the undo history, coalescing into the open group when
    /// it abuts one of the same kind. Redo is cleared either way.
    pub(super) fn record_edit(&mut self, parts: EditParts<'_>, kind: EditKind) {
        let coalesced = self.last_edit_kind == Some(kind)
            && self
                .undo
                .back_mut()
                .is_some_and(|previous| previous.coalesce(parts, kind));
        if !coalesced {
            if self.undo.len() == UNDO_LIMIT {
                self.undo.pop_front();
            }
            self.undo.push_back(EditDelta::from_parts(parts));
        }
        self.redo.clear();
        self.last_edit_kind = Some(kind);
    }

    /// Wipe the history if `text_hash` is not the buffer it describes, then adopt it.
    fn adopt_text_hash(&mut self, text_hash: NonZeroU64) {
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
    }

    /// Paint-path entry: adopt the probe's hash and close the local-edit window.
    ///
    /// `None` changes nothing: the probe never hashed the buffer, so there is
    /// nothing to adopt and our edit has not been seen.
    pub(super) fn observe_text_hash(&mut self, text_hash: Option<NonZeroU64>) {
        let Some(text_hash) = text_hash else {
            return;
        };
        self.adopt_text_hash(text_hash);
        self.local_edit_pending = false;
    }

    /// Input-path entry, before an edit: mint the hash for this frame's buffer.
    ///
    /// Skipped while a local edit is pending; paint adopts the settled hash.
    pub(super) fn reconcile_before_edit(&mut self, text: &str) {
        if self.local_edit_pending {
            return;
        }
        self.adopt_text_hash(Self::text_hash(text));
    }

    /// The identity of `text`, minted as the shape probe mints it.
    pub(super) fn text_hash(text: &str) -> NonZeroU64 {
        TextProbe::hash_of(text)
    }

    pub(super) fn sel_range(&self) -> Option<ops::Range<usize>> {
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
    use crate::common::hash;
    use crate::text::glyph_font::GlyphFont;
    use crate::text::key::TextShapeKey;
    use crate::widgets::text_edit::edit_state::{EditKind, EditState};

    /// Both entry points must mint a buffer's identity alike; a mismatch reads as a
    /// host replacement and wipes undo. `content_hash` maps a raw zero to one, so a
    /// bare `hash_str` would differ only for content hashing to zero.
    #[test]
    fn both_entry_points_mint_one_buffers_identity_alike() {
        for text in ["", "a", "hello world", "\u{1f600} multi\nline"] {
            let probe = TextShapeKey::unbounded(hash::hash_str(text), GlyphFont::new(16.0));
            assert_eq!(
                EditState::text_hash(text),
                probe.text_hash,
                "input path disagrees with the probe for {text:?}",
            );
        }
        assert_eq!(TextShapeKey::content_hash(0).get(), 1);
    }

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

    /// A frame whose probe hashed nothing leaves the history rule untouched, and the
    /// kept expectation still catches a replacement on the next hashed frame.
    #[test]
    fn a_frame_that_hashed_nothing_observes_nothing() {
        let mut state = EditState::default();
        state.reconcile_before_edit("host value");
        let expected = state.expected_hash;
        assert_eq!(
            expected,
            Some(EditState::text_hash("host value")),
            "premise: the input path leaves an expectation to protect",
        );
        state.local_edit_pending = true;

        state.observe_text_hash(None);
        assert_eq!(
            state.expected_hash, expected,
            "an unhashed frame must not overwrite the standing expectation",
        );
        assert!(
            state.local_edit_pending,
            "an unhashed frame has not seen our edit, so the window stays open",
        );

        state.local_edit_pending = false;
        state.last_edit_kind = Some(EditKind::Typing);
        state.observe_text_hash(Some(EditState::text_hash("replaced by the host")));
        assert!(
            state.last_edit_kind.is_none(),
            "the kept expectation must still catch a host replacement",
        );
    }
}
