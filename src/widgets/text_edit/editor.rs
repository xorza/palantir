//! One frame's semantic editing session over the host-owned buffer.

use crate::input::key_class::KeyFilter;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widget_core::response::ResponseSnapshot;
use crate::widgets::context_menu::ContextMenu;
use crate::widgets::context_menu::menu_item::MenuItem;
use crate::widgets::context_menu::menu_separator::MenuSeparator;
use crate::widgets::text_edit::action::{ActionAvailability, EditAction};
use crate::widgets::text_edit::edit_state::{
    EditDelta, EditKind, EditParts, EditState, SelectionState,
};
use crate::widgets::text_edit::unicode::{
    next_grapheme_boundary, next_word_boundary, prev_grapheme_boundary, prev_word_boundary,
    sanitize_single_line, word_range_at,
};
use std::borrow::Cow;
use std::ops;

#[derive(Debug)]
pub(super) struct Editor<'a> {
    text: &'a mut String,
    state: &'a mut EditState,
    multiline: bool,
    max_chars: Option<usize>,
    history_checked: bool,
    /// The buffer was mutated this session, set at the mutation choke points so a
    /// same-length overwrite still reports.
    edited: bool,
}

impl<'a> Editor<'a> {
    pub(super) const fn new(
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

    /// Run the default context menu against this session, returning whether it
    /// edited the buffer. Caret motion is not reported: `TextEdit::pass` brackets
    /// this and the keyboard pass in one comparison. One session serves the whole
    /// menu so undo history is reconciled once; `filter` is the field's own, as the
    /// menu drains the same layer-wide stream and owes the same
    /// [`KeyFilter::takes_press`] gate.
    pub(super) fn show_menu(
        &mut self,
        ui: &mut Ui,
        snapshot: &ResponseSnapshot,
        filter: KeyFilter,
    ) -> bool {
        let clipboard = ui.clipboard();
        let mut clicked_action = None;
        ContextMenu::on(snapshot).show(ui, |ui, popup| {
            for &keypress in ui.keyboard_events() {
                if !filter.takes_press(keypress) {
                    continue;
                }
                if let Some(action) = EditAction::from_keypress(keypress) {
                    action.execute(self, &clipboard);
                    if EditAction::MENU.iter().any(|item| item.action == action) {
                        popup.close();
                    }
                }
            }

            let has_selection = self.has_selection();
            let has_text = self.has_text();
            for item in EditAction::MENU {
                if item.separator_before {
                    MenuSeparator::new().show(ui);
                }
                let enabled = match item.availability {
                    ActionAvailability::Always => true,
                    ActionAvailability::Selection => has_selection,
                    ActionAvailability::Text => has_text,
                };
                let mut row = MenuItem::new(item.label).disabled(!enabled);
                if let Some(shortcut) = item.action.shortcut() {
                    row = row.shortcut_hint(shortcut);
                }
                if row.show(ui, popup).clicked() {
                    clicked_action = Some(item.action);
                }
            }
        });
        if let Some(action) = clicked_action {
            action.execute(self, &clipboard);
        }
        self.edited
    }

    pub(super) fn text(&self) -> &str {
        self.text
    }

    pub(super) const fn multiline(&self) -> bool {
        self.multiline
    }

    pub(super) const fn edited(&self) -> bool {
        self.edited
    }

    pub(super) const fn caret(&self) -> usize {
        self.state.caret
    }

    pub(super) fn has_selection(&self) -> bool {
        self.state.sel_range().is_some()
    }

    pub(super) const fn has_text(&self) -> bool {
        !self.text.is_empty()
    }

    pub(super) fn normalize(&mut self) {
        self.state.normalize(self.text);
    }

    /// Place the caret for a pointer press: one click places it and arms the drag,
    /// two take the word, three or more everything. A multi-click leaves the drag
    /// disarmed. With `extend` (Shift) a single press keeps the anchor, so the drag
    /// grows the selection from where it began.
    pub(super) fn press(&mut self, at: usize, clicks: u8, extend: bool) {
        self.state.drag_anchor = None;
        if extend && clicks <= 1 {
            let anchor = self.state.selection.unwrap_or(self.state.caret);
            self.select_range(anchor, at);
            self.state.drag_anchor = Some(anchor);
            return;
        }
        match clicks {
            2 => {
                let word = word_range_at(self.text, at);
                if word.is_empty() {
                    self.arm_drag(at);
                } else {
                    self.select_range(word.start, word.end);
                }
            }
            3.. => self.select_all(),
            _ => self.arm_drag(at),
        }
    }

    /// Grow the pointer selection to `at` from the armed press's anchor; nothing
    /// after a multi-click.
    pub(super) fn drag_to(&mut self, at: usize) {
        if self.state.drag_anchor.is_some() {
            self.move_caret(at, true);
        }
    }

    pub(super) const fn end_drag(&mut self) {
        self.state.drag_anchor = None;
    }

    fn arm_drag(&mut self, at: usize) {
        self.state.drag_anchor = Some(at);
        self.select_range(at, at);
    }

    const fn selection_state(&self) -> SelectionState {
        SelectionState {
            caret: self.state.caret,
            selection: self.state.selection,
        }
    }

    /// Reconcile the history against the buffer once per editor; `history_checked`
    /// latches it, as several entry points open with it.
    fn ensure_history_matches(&mut self) {
        if self.history_checked {
            return;
        }
        self.history_checked = true;
        self.state.reconcile_before_edit(self.text);
    }

    const fn mark_local_edit(&mut self) {
        self.state.local_edit_pending = true;
    }

    fn replace_range(&mut self, range: ops::Range<usize>, replacement: &str, kind: EditKind) {
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
        // Derived, not read back after the splice: the history wants it *before*
        // the buffer moves.
        let after = SelectionState {
            caret: range.start + replacement.len(),
            selection: None,
        };
        let removed = &self.text[range.clone()];
        let counts_chars = self.state.char_count.is_some();
        let removed_chars = counts_chars.then(|| removed.chars().count());
        let inserted_chars = counts_chars.then(|| replacement.chars().count());
        // Recorded first, while `removed` still borrows the live buffer, so typing
        // appends into the open undo group with no allocation.
        self.state.record_edit(
            EditParts {
                start: range.start,
                removed,
                inserted: replacement,
                before,
                after,
            },
            kind,
        );
        self.text.replace_range(range, replacement);
        self.state.caret = after.caret;
        self.state.selection = after.selection;
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

    /// Replace the live selection with `s` under one undo unit of `kind`: the choke
    /// point for typing, IME text, newline insert and paste.
    pub(super) fn replace_selection(&mut self, s: &str, kind: EditKind) {
        self.ensure_history_matches();
        let fit_len = self.capped_prefix(s).len();
        let fit = &s[..fit_len];
        // Input the cap leaves no room for is dropped whole; an empty replacement
        // is a delete, which still clears a selection.
        if fit.is_empty() && (!s.is_empty() || self.state.selection.is_none()) {
            return;
        }
        let range = self
            .state
            .sel_range()
            .unwrap_or(self.state.caret..self.state.caret);
        self.replace_range(range, fit, kind);
    }

    pub(super) fn sanitized<'s>(&self, raw: &'s str) -> Cow<'s, str> {
        if self.multiline {
            Cow::Borrowed(raw)
        } else {
            sanitize_single_line(raw)
        }
    }

    pub(super) fn paste(&mut self, raw: &str) {
        let cleaned = self.sanitized(raw);
        if !cleaned.is_empty() {
            self.replace_selection(&cleaned, EditKind::Other);
        }
    }

    pub(super) fn cut_selection(&mut self) {
        let Some(r) = self.state.sel_range() else {
            return;
        };
        self.replace_range(r, "", EditKind::Other);
    }

    pub(super) fn selected_text(&self) -> Option<&str> {
        self.state.sel_range().map(|range| &self.text[range])
    }

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

    pub(super) fn select_all(&mut self) {
        self.select_range(0, self.text.len());
    }

    /// Select `start..end` with the caret at `end` (a bare caret when empty),
    /// keeping the "never `Some(caret)`" invariant and ending the edit-coalesce
    /// group.
    pub(super) fn select_range(&mut self, start: usize, end: usize) {
        self.state.selection = (start != end).then_some(start);
        self.state.caret = end;
        self.state.last_edit_kind = None;
    }

    /// Move the caret to `new_caret`, extending the selection (latching the anchor
    /// on the first extending move) or collapsing it; ends the edit-coalesce group.
    pub(super) fn move_caret(&mut self, new_caret: usize, extend: bool) {
        let anchor = match self.state.selection {
            Some(anchor) if extend => anchor,
            // With nothing selected an extending move latches the anchor at the
            // caret; a collapsing one anchors on the destination (empty).
            _ if extend => self.state.caret,
            _ => new_caret,
        };
        self.select_range(anchor, new_caret);
    }

    pub(super) fn undo(&mut self) {
        self.ensure_history_matches();
        if let Some(delta) = self.state.undo.pop_back() {
            self.apply_history(&delta, true);
            self.state.redo.push(delta);
        }
    }

    pub(super) fn redo(&mut self) {
        self.ensure_history_matches();
        if let Some(delta) = self.state.redo.pop() {
            self.apply_history(&delta, false);
            self.state.undo.push_back(delta);
        }
    }

    /// Type `text` over the selection or at the caret. A whole string: one press
    /// can produce two characters (an uncomposed dead-key sequence) that belong in
    /// one undo step.
    pub(super) fn insert_str(&mut self, text: &str) {
        self.replace_selection(text, EditKind::Typing);
    }

    pub(super) fn delete_to_line_start(&mut self) {
        let range = if let Some(range) = self.state.sel_range() {
            range
        } else {
            let caret = self.state.caret;
            let start = self.text[..caret].rfind('\n').map_or(0, |i| i + 1);
            start..caret
        };
        if !range.is_empty() {
            self.replace_range(range, "", EditKind::Delete);
        }
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

    pub(super) const fn collapse_selection(&mut self) -> bool {
        if self.state.selection.is_none() {
            return false;
        }
        self.state.selection = None;
        self.state.last_edit_kind = None;
        true
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::widgets::text_edit::edit_state::EditState;
    use crate::widgets::text_edit::editor::Editor;

    impl Editor<'_> {
        pub(crate) fn redo_len(&self) -> usize {
            self.state.redo.len()
        }

        pub(crate) fn observe_text(&mut self) {
            let text_hash = EditState::text_hash(self.text);
            self.state.observe_text_hash(Some(text_hash));
        }
    }
}
