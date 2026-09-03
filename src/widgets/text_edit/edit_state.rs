//! Semantic state for the host-owned text buffer, plus the undo
//! history it retains. [`Editor`](super::editor::Editor) is the
//! behaviour over this data.

use crate::common::hash;
use crate::text::key::TextShapeKey;
use std::collections::VecDeque;
use std::num::NonZeroU64;

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
    /// Byte the pointer started the in-flight drag-select from. `None`
    /// when no drag is latched. Here rather than beside the pointer
    /// state because it is a byte offset into the same host buffer as
    /// `caret` and `selection`, and [`Self::normalize`] has to repair
    /// all three together or a drag that outlives a host edit selects
    /// from a stale boundary.
    pub(super) drag_anchor: Option<usize>,
    pub(super) undo: VecDeque<EditDelta>,
    pub(super) redo: Vec<EditDelta>,
    /// Kind of the most recent recorded edit, used to coalesce
    /// consecutive same-kind edits (typing chars, deleting chars) into
    /// a single undo unit. `None` after any caret-only motion so the
    /// next edit always opens a fresh group.
    pub(super) last_edit_kind: Option<EditKind>,
    pub(super) expected_hash: Option<NonZeroU64>,
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

/// One edit as its parts, before the history decides whether to keep it.
///
/// The text is borrowed — from the buffer for `removed`, from the caller
/// for `inserted` — so [`EditState::record_edit`] can try to extend the
/// open undo group first and copy nothing at all in the case that matters:
/// a keystroke appended to the group it opened. Only an edit that starts a
/// new group is turned into an owning [`EditDelta`].
#[derive(Clone, Copy, Debug)]
pub(super) struct EditParts<'a> {
    pub(super) start: usize,
    pub(super) removed: &'a str,
    pub(super) inserted: &'a str,
    pub(super) before: SelectionState,
    pub(super) after: SelectionState,
}

/// One undoable buffer edit. Built here — from the borrowed [`EditParts`]
/// an edit arrives as — and replayed by
/// [`Editor`](super::editor::Editor), which is why the fields are
/// `pub(super)`: the data and the stacks that retain it live here, the
/// behaviour that applies one lives there.
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

/// Cap on retained undo entries; [`EditState::record_edit`] drops the
/// oldest past this.
const UNDO_LIMIT: usize = 128;

impl EditDelta {
    /// Take ownership of `parts` — the two `String`s the history keeps.
    fn from_parts(parts: EditParts<'_>) -> Self {
        Self {
            start: parts.start,
            removed: parts.removed.to_owned(),
            inserted: parts.inserted.to_owned(),
            before: parts.before,
            after: parts.after,
        }
    }

    /// Extend this delta with an edit that abuts it, reporting whether it
    /// did. Takes the parts rather than a built [`EditDelta`] so the caller
    /// pays for the strings only when this says no.
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
    /// Fold one edit into the undo history: appended to the open group when
    /// it abuts one of the same kind, otherwise pushed as a new entry with
    /// the oldest dropped past [`UNDO_LIMIT`]. Redo dies either way — the
    /// timeline just forked.
    ///
    /// Takes borrowed parts, so a keystroke that lands in the open group
    /// copies its character into that group's `String` and allocates
    /// nothing. That is the steady state of typing.
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

    /// Wipe the history if `text_hash` is not the buffer it describes,
    /// then adopt it as the expectation.
    ///
    /// **The one statement of "did the host replace the buffer".** The
    /// undo stack holds byte ranges into text the application owns; if
    /// that text changed and we did not change it, every range in the
    /// stack refers to something gone. Both entry points below reach the
    /// rule here — written out at each of them, the two bodies drifted on
    /// which hash they compared.
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

    /// Paint-path entry: the shape probe already hashed this buffer, so
    /// its hash is the one to adopt. Also closes the local-edit window —
    /// whatever we changed this frame has now been seen.
    ///
    /// **`None` observes nothing, and so changes nothing.** A probe whose
    /// face named no usable size never hashed the buffer, so this frame
    /// learned neither the identity to adopt nor that our own edit has
    /// been seen. Adopting an absence would drop a standing expectation
    /// and miss a host replacement that landed while the face was
    /// unusable; closing the window would then blame the *next* frame for
    /// an edit we made ourselves.
    pub(super) fn observe_text_hash(&mut self, text_hash: Option<NonZeroU64>) {
        let Some(text_hash) = text_hash else {
            return;
        };
        self.adopt_text_hash(text_hash);
        self.local_edit_pending = false;
    }

    /// Input-path entry, before an edit is applied: no probe has run for
    /// this frame's buffer yet, so mint the hash.
    ///
    /// A pending local edit means we moved the buffer ourselves and the
    /// hash it settles at is not known until paint, which adopts it —
    /// so there is nothing to reconcile against here.
    pub(super) fn reconcile_before_edit(&mut self, text: &str) {
        if self.local_edit_pending {
            return;
        }
        self.adopt_text_hash(Self::text_hash(text));
    }

    /// The identity of `text`, minted the way the shape probe mints the
    /// hash [`Self::observe_text_hash`] is fed.
    ///
    /// Through [`TextShapeKey::content_hash`] and not a bare `hash_str`:
    /// that rule maps a raw zero to one, so for exactly the content that
    /// hashes to zero a raw hash here would compare unequal against the
    /// probe's for the *same* buffer and silently wipe the undo stack.
    pub(super) fn text_hash(text: &str) -> NonZeroU64 {
        TextShapeKey::content_hash(hash::hash_str(text))
    }

    pub(super) fn sel_range(&self) -> Option<std::ops::Range<usize>> {
        let a = self.selection?;
        Some(a.min(self.caret)..a.max(self.caret))
    }

    fn repair_offset(text: &str, offset: usize) -> usize {
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

    /// The two entry points into the history rule must mint one buffer's
    /// identity identically, because they write and read the same
    /// `expected_hash`: the paint path adopts the probe's, the input path
    /// mints its own, and a disagreement reads as "the host replaced the
    /// buffer" and wipes the undo stack under the user.
    ///
    /// The zero case is why this is a test rather than a comment.
    /// `content_hash` maps a raw zero to one, so a bare `hash_str` on the
    /// input side would differ for exactly the content that hashes to
    /// zero, and for nothing else.
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
        // The mapped case, driven directly: a raw zero is the one input
        // where the two spellings part company.
        assert_eq!(TextShapeKey::content_hash(0).get(), 1);
    }

    /// A frame whose probe hashed nothing must leave the history rule
    /// exactly as it found it, and the expectation it kept must still
    /// catch a replacement on the next frame that does hash — see
    /// [`EditState::observe_text_hash`] for why both halves matter.
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

        // And the expectation it kept is the one that catches the
        // replacement on the next frame that *does* hash.
        state.local_edit_pending = false;
        state.last_edit_kind = Some(EditKind::Typing);
        state.observe_text_hash(Some(EditState::text_hash("replaced by the host")));
        assert!(
            state.last_edit_kind.is_none(),
            "the kept expectation must still catch a host replacement",
        );
    }
}
