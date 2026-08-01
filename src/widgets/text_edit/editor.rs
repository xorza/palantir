//! One frame's semantic editing session over the host-owned buffer.

use crate::common::hash;
use crate::widgets::text_edit::edit_state::{
    EditDelta, EditKind, EditState, SelectionState, UNDO_LIMIT,
};
use crate::widgets::text_edit::unicode::{
    next_grapheme_boundary, next_word_boundary, prev_grapheme_boundary, prev_word_boundary,
    sanitize_single_line,
};
use std::borrow::Cow;

/// One frame's semantic editing session.
#[derive(Debug)]
pub(super) struct Editor<'a> {
    pub(super) text: &'a mut String,
    pub(super) state: &'a mut EditState,
    pub(super) multiline: bool,
    max_chars: Option<usize>,
    history_checked: bool,
    /// The buffer was mutated this session (typing, delete, paste,
    /// cut, undo/redo). Set by the mutation choke points, so it's
    /// content-accurate — a same-length overwrite still reports.
    pub(super) edited: bool,
}

impl<'a> Editor<'a> {
    pub(super) fn new(
        text: &'a mut String,
        state: &'a mut EditState,
        multiline: bool,
        max_chars: Option<usize>,
    ) -> Self {
        Self {
            text,
            state,
            multiline,
            max_chars,
            history_checked: false,
            edited: false,
        }
    }

    fn selection_state(&self) -> SelectionState {
        SelectionState {
            caret: self.state.caret,
            selection: self.state.selection,
        }
    }

    fn ensure_history_matches(&mut self) {
        if self.history_checked {
            return;
        }
        if self.state.local_edit_pending {
            self.history_checked = true;
            return;
        }
        let current_hash = hash::hash_str(self.text);
        if self
            .state
            .expected_hash
            .is_some_and(|expected| expected != current_hash)
        {
            self.state.undo.clear();
            self.state.redo.clear();
            self.state.last_edit_kind = None;
            self.state.char_count = None;
        }
        self.state.expected_hash = Some(current_hash);
        self.history_checked = true;
    }

    fn mark_local_edit(&mut self) {
        self.state.local_edit_pending = true;
    }

    fn push_delta(&mut self, delta: EditDelta, kind: EditKind) {
        let coalesced = self.state.last_edit_kind == Some(kind)
            && self
                .state
                .undo
                .back_mut()
                .is_some_and(|previous| previous.coalesce(&delta, kind));
        if !coalesced {
            if self.state.undo.len() == UNDO_LIMIT {
                self.state.undo.pop_front();
            }
            self.state.undo.push_back(delta);
        }
        self.state.redo.clear();
        self.state.last_edit_kind = Some(kind);
    }

    fn replace_range(&mut self, range: std::ops::Range<usize>, replacement: &str, kind: EditKind) {
        debug_assert!(self.text.is_char_boundary(range.start));
        debug_assert!(self.text.is_char_boundary(range.end));
        debug_assert!(range.start <= range.end);
        if &self.text[range.clone()] == replacement {
            self.state.caret = range.start + replacement.len();
            self.state.selection = None;
            self.state.last_edit_kind = None;
            return;
        }
        self.ensure_history_matches();
        let before = self.selection_state();
        let removed = self.text[range.clone()].to_owned();
        let removed_chars = self
            .state
            .char_count
            .is_some()
            .then(|| removed.chars().count());
        let inserted_chars = self
            .state
            .char_count
            .is_some()
            .then(|| replacement.chars().count());
        self.text.replace_range(range.clone(), replacement);
        self.state.caret = range.start + replacement.len();
        self.state.selection = None;
        let delta = EditDelta {
            start: range.start,
            removed,
            inserted: replacement.to_owned(),
            before,
            after: self.selection_state(),
        };
        self.push_delta(delta, kind);
        if let Some(count) = &mut self.state.char_count {
            *count = *count - removed_chars.unwrap() + inserted_chars.unwrap();
        }
        self.mark_local_edit();
        self.edited = true;
    }

    fn apply_history(&mut self, delta: &EditDelta, undo: bool) {
        let (remove_len, replacement, selection) = if undo {
            (delta.inserted.len(), delta.removed.as_str(), delta.before)
        } else {
            (delta.removed.len(), delta.inserted.as_str(), delta.after)
        };
        let end = delta.start + remove_len;
        debug_assert!(end <= self.text.len());
        debug_assert!(self.text.is_char_boundary(delta.start));
        debug_assert!(self.text.is_char_boundary(end));
        self.text.replace_range(delta.start..end, replacement);
        self.state.caret = selection.caret;
        self.state.selection = selection.selection;
        if self.state.char_count.is_some() {
            self.state.char_count = Some(self.text.chars().count());
        }
        self.state.last_edit_kind = None;
        self.mark_local_edit();
        self.edited = true;
    }

    /// Portion of `s` that fits after deleting the live selection.
    /// The cap is by character count; the returned prefix remains on
    /// a UTF-8 boundary.
    fn capped_prefix<'s>(&mut self, s: &'s str) -> &'s str {
        match self.max_chars {
            Some(max) => {
                let selected_chars = self
                    .state
                    .sel_range()
                    .map_or(0, |range| self.text[range].chars().count());
                let current_chars = *self
                    .state
                    .char_count
                    .get_or_insert_with(|| self.text.chars().count());
                let chars_after_delete = current_chars - selected_chars;
                let room = max.saturating_sub(chars_after_delete);
                match s.char_indices().nth(room) {
                    Some((byte, _)) => &s[..byte],
                    None => s,
                }
            }
            None => s,
        }
    }

    /// Replace the live selection with `s` under one undo unit of
    /// `kind` — the shared choke point for typing, IME text, newline
    /// insert, and paste.
    pub(super) fn replace_selection(&mut self, s: &str, kind: EditKind) {
        self.ensure_history_matches();
        let fit_len = self.capped_prefix(s).len();
        let fit = &s[..fit_len];
        if self.state.selection.is_none() && fit.is_empty() {
            return;
        }
        let range = self
            .state
            .sel_range()
            .unwrap_or(self.state.caret..self.state.caret);
        self.replace_range(range, fit, kind);
    }

    /// Single-line editors never admit line breaks; multi-line passes
    /// text through untouched.
    pub(super) fn sanitized<'s>(&self, raw: &'s str) -> Cow<'s, str> {
        if self.multiline {
            Cow::Borrowed(raw)
        } else {
            sanitize_single_line(raw)
        }
    }

    /// Paste at the caret, replacing any live selection; line breaks
    /// are sanitized away for single-line editors. No-op on an empty
    /// clipboard.
    pub(super) fn paste(&mut self, raw: &str) {
        let cleaned = self.sanitized(raw);
        if !cleaned.is_empty() {
            self.replace_selection(&cleaned, EditKind::Other);
        }
    }

    /// Delete the live selection as one bulk edit.
    pub(super) fn cut_selection(&mut self) {
        let Some(r) = self.state.sel_range() else {
            return;
        };
        self.replace_range(r, "", EditKind::Other);
    }

    pub(super) fn selected_text(&self) -> Option<&str> {
        self.state.sel_range().map(|range| &self.text[range])
    }

    /// Clear the whole buffer (the context menu's Clear).
    pub(super) fn clear(&mut self) {
        if !self.text.is_empty() {
            self.replace_range(0..self.text.len(), "", EditKind::Other);
        }
    }

    pub(super) fn enforce_single_line(&mut self) {
        if self.multiline {
            return;
        }
        let Cow::Owned(cleaned) = sanitize_single_line(self.text) else {
            return;
        };
        self.ensure_history_matches();
        self.state.undo.clear();
        self.state.redo.clear();
        self.state.last_edit_kind = None;
        *self.text = cleaned;
        self.state.normalize(self.text);
        if self.state.char_count.is_some() {
            self.state.char_count = Some(self.text.chars().count());
        }
        self.mark_local_edit();
        self.edited = true;
    }

    /// Select the whole buffer (collapses to no-selection when empty).
    pub(super) fn select_all(&mut self) {
        self.state.selection = (!self.text.is_empty()).then_some(0);
        self.state.caret = self.text.len();
        self.state.last_edit_kind = None;
    }

    /// Move the caret to `new_caret`, extending the selection if
    /// `extend` is set (latches the anchor on the first extending
    /// move) or collapsing it otherwise. Maintains the "never
    /// `Some(caret)`" invariant. Always ends the current edit-coalesce
    /// group — caret-only motion breaks Typing / Delete runs into
    /// separate undo entries.
    pub(super) fn move_caret(&mut self, new_caret: usize, extend: bool) {
        if extend {
            self.state.selection.get_or_insert(self.state.caret);
        } else {
            self.state.selection = None;
        }
        self.state.caret = new_caret;
        if self.state.selection == Some(self.state.caret) {
            self.state.selection = None;
        }
        self.state.last_edit_kind = None;
    }

    /// No-op on an empty stack.
    pub(super) fn undo(&mut self) {
        self.ensure_history_matches();
        if let Some(delta) = self.state.undo.pop_back() {
            self.apply_history(&delta, true);
            self.state.redo.push(delta);
        }
    }

    /// No-op on an empty stack.
    pub(super) fn redo(&mut self) {
        self.ensure_history_matches();
        if let Some(delta) = self.state.redo.pop() {
            self.apply_history(&delta, false);
            self.state.undo.push_back(delta);
        }
    }

    pub(super) fn insert_char(&mut self, c: char) {
        let mut buf = [0u8; 4];
        self.replace_selection(c.encode_utf8(&mut buf), EditKind::Typing);
    }

    pub(super) fn delete_backward(&mut self) {
        if self.state.selection.is_none() && self.state.caret == 0 {
            return;
        }
        let range = if let Some(range) = self.state.sel_range() {
            range
        } else {
            let prev = prev_grapheme_boundary(self.text, self.state.caret);
            prev..self.state.caret
        };
        self.replace_range(range, "", EditKind::Delete);
    }

    pub(super) fn delete_forward(&mut self) {
        if self.state.selection.is_none() && self.state.caret == self.text.len() {
            return;
        }
        let range = if let Some(range) = self.state.sel_range() {
            range
        } else {
            let next = next_grapheme_boundary(self.text, self.state.caret);
            self.state.caret..next
        };
        self.replace_range(range, "", EditKind::Delete);
    }

    pub(super) fn move_grapheme_left(&mut self, extend: bool) {
        let target = if !extend && let Some(range) = self.state.sel_range() {
            range.start
        } else {
            prev_grapheme_boundary(self.text, self.state.caret)
        };
        self.move_caret(target, extend);
    }

    pub(super) fn move_grapheme_right(&mut self, extend: bool) {
        let target = if !extend && let Some(range) = self.state.sel_range() {
            range.end
        } else {
            next_grapheme_boundary(self.text, self.state.caret)
        };
        self.move_caret(target, extend);
    }

    pub(super) fn move_word_left(&mut self, extend: bool) {
        let target = prev_word_boundary(self.text, self.state.caret);
        self.move_caret(target, extend);
    }

    pub(super) fn move_word_right(&mut self, extend: bool) {
        let target = next_word_boundary(self.text, self.state.caret);
        self.move_caret(target, extend);
    }

    pub(super) fn collapse_selection(&mut self) -> bool {
        if self.state.selection.is_none() {
            return false;
        }
        self.state.selection = None;
        self.state.last_edit_kind = None;
        true
    }
}
