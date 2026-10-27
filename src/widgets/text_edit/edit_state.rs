//! Semantic state for the host-owned text buffer: caret, selection, undo history, and the
//! identity that tells a local edit from a host replacement.

use crate::text::probe::TextProbe;
use crate::widgets::text_edit::history::History;
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
    pub(super) history: History,
    pub(super) expected_hash: Option<NonZeroU64>,
    pub(super) local_edit_pending: bool,
    /// The buffer's characters, counted once a cap asks and kept exact by each edit's
    /// delta; `None` until then and after a host replacement.
    pub(super) char_count: Option<usize>,
}

impl EditState {
    /// Wipe the history if `text_hash` is not the buffer it describes, then adopt it.
    fn adopt_text_hash(&mut self, text_hash: NonZeroU64) {
        if !self.local_edit_pending
            && self
                .expected_hash
                .is_some_and(|expected| expected != text_hash)
        {
            self.history.clear();
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
    use crate::common::hash;
    use crate::text::glyph_font::GlyphFont;
    use crate::text::key::TextShapeKey;
    use crate::widgets::text_edit::edit_state::EditState;
    use crate::widgets::text_edit::history::{EditKind, EditParts, SelectionState};

    /// Both entry points must mint a buffer's identity alike; a mismatch reads as a
    /// host replacement and wipes undo. `content_hash` maps a raw zero to one, so a
    /// bare `hash::str` would differ only for content hashing to zero.
    #[test]
    fn both_entry_points_mint_one_buffers_identity_alike() {
        for text in ["", "a", "hello world", "\u{1f600} multi\nline"] {
            let probe = TextShapeKey::unbounded(hash::str(text), GlyphFont::new(16.0));
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
        let at = SelectionState {
            caret: 0,
            selection: None,
        };
        state.history.record(
            EditParts {
                start: 0,
                removed: "",
                inserted: "x",
                before: at,
                after: at,
            },
            EditKind::Typing,
        );
        state.observe_text_hash(Some(EditState::text_hash("replaced by the host")));
        assert_eq!(
            (state.history.undo_len(), state.history.last_kind()),
            (0, None),
            "the kept expectation must still catch a host replacement",
        );
    }
}
