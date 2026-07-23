//! Text-buffer mutation history and Unicode navigation.

use std::borrow::Cow;
use std::collections::VecDeque;

use crate::common::hash;

/// Semantic state for the host-owned text buffer.
///
/// `caret` is a byte offset. Widget-driven mutations step grapheme
/// boundaries, while [`Self::normalize`] repairs offsets after the host
/// replaces the buffer between frames.
#[derive(Clone, Default, Debug)]
pub(crate) struct EditState {
    pub(crate) caret: usize,
    /// Selection anchor. `None` = no selection. Invariant: never
    /// `Some(caret)` — every mutation site collapses an empty selection
    /// to `None` so "selection live" is a single `is_some()` check.
    pub(crate) selection: Option<usize>,
    pub(crate) undo: VecDeque<EditDelta>,
    pub(crate) redo: Vec<EditDelta>,
    /// Kind of the most recent recorded edit, used to coalesce
    /// consecutive same-kind edits (typing chars, deleting chars) into
    /// a single undo unit. `None` after any caret-only motion so the
    /// next edit always opens a fresh group.
    pub(crate) last_edit_kind: Option<EditKind>,
    pub(crate) expected_hash: Option<u64>,
    pub(crate) local_edit_pending: bool,
    pub(crate) char_count: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SelectionState {
    caret: usize,
    selection: Option<usize>,
}

#[derive(Clone, Debug)]
pub(crate) struct EditDelta {
    start: usize,
    removed: String,
    inserted: String,
    before: SelectionState,
    after: SelectionState,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum EditKind {
    Typing,
    Delete,
    /// Bulk edits (paste, cut, clear, newline insert) — never coalesce.
    Other,
}

const UNDO_LIMIT: usize = 128;

/// One frame's semantic editing session.
#[derive(Debug)]
pub(crate) struct Editor<'a> {
    pub(crate) text: &'a mut String,
    pub(crate) state: &'a mut EditState,
    pub(crate) multiline: bool,
    max_chars: Option<usize>,
    history_checked: bool,
    /// The buffer was mutated this session (typing, delete, paste,
    /// cut, undo/redo). Set by the mutation choke points, so it's
    /// content-accurate — a same-length overwrite still reports.
    pub(crate) edited: bool,
}

impl<'a> Editor<'a> {
    pub(crate) fn new(
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
    pub(crate) fn replace_selection(&mut self, s: &str, kind: EditKind) {
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
    pub(crate) fn sanitized<'s>(&self, raw: &'s str) -> Cow<'s, str> {
        if self.multiline {
            Cow::Borrowed(raw)
        } else {
            sanitize_single_line(raw)
        }
    }

    /// Paste at the caret, replacing any live selection; line breaks
    /// are sanitized away for single-line editors. No-op on an empty
    /// clipboard.
    pub(crate) fn paste(&mut self, raw: &str) {
        let cleaned = self.sanitized(raw);
        if !cleaned.is_empty() {
            self.replace_selection(&cleaned, EditKind::Other);
        }
    }

    /// Delete the live selection as one bulk edit.
    pub(crate) fn cut_selection(&mut self) {
        let Some(r) = self.state.sel_range() else {
            return;
        };
        self.replace_range(r, "", EditKind::Other);
    }

    pub(crate) fn selected_text(&self) -> Option<&str> {
        self.state.sel_range().map(|range| &self.text[range])
    }

    /// Clear the whole buffer (the context menu's Clear).
    pub(crate) fn clear(&mut self) {
        if !self.text.is_empty() {
            self.replace_range(0..self.text.len(), "", EditKind::Other);
        }
    }

    pub(crate) fn enforce_single_line(&mut self) {
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
    pub(crate) fn select_all(&mut self) {
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
    pub(crate) fn move_caret(&mut self, new_caret: usize, extend: bool) {
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
    pub(crate) fn undo(&mut self) {
        self.ensure_history_matches();
        if let Some(delta) = self.state.undo.pop_back() {
            self.apply_history(&delta, true);
            self.state.redo.push(delta);
        }
    }

    /// No-op on an empty stack.
    pub(crate) fn redo(&mut self) {
        self.ensure_history_matches();
        if let Some(delta) = self.state.redo.pop() {
            self.apply_history(&delta, false);
            self.state.undo.push_back(delta);
        }
    }

    pub(crate) fn insert_char(&mut self, c: char) {
        let mut buf = [0u8; 4];
        self.replace_selection(c.encode_utf8(&mut buf), EditKind::Typing);
    }

    pub(crate) fn delete_backward(&mut self) {
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

    pub(crate) fn delete_forward(&mut self) {
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

    pub(crate) fn move_grapheme_left(&mut self, extend: bool) {
        let target = if !extend && let Some(range) = self.state.sel_range() {
            range.start
        } else {
            prev_grapheme_boundary(self.text, self.state.caret)
        };
        self.move_caret(target, extend);
    }

    pub(crate) fn move_grapheme_right(&mut self, extend: bool) {
        let target = if !extend && let Some(range) = self.state.sel_range() {
            range.end
        } else {
            next_grapheme_boundary(self.text, self.state.caret)
        };
        self.move_caret(target, extend);
    }

    pub(crate) fn move_word_left(&mut self, extend: bool) {
        let target = prev_word_boundary(self.text, self.state.caret);
        self.move_caret(target, extend);
    }

    pub(crate) fn move_word_right(&mut self, extend: bool) {
        let target = next_word_boundary(self.text, self.state.caret);
        self.move_caret(target, extend);
    }

    pub(crate) fn collapse_selection(&mut self) -> bool {
        if self.state.selection.is_none() {
            return false;
        }
        self.state.selection = None;
        self.state.last_edit_kind = None;
        true
    }
}

impl EditDelta {
    fn coalesce(&mut self, next: &Self, kind: EditKind) -> bool {
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
    pub(crate) fn observe_text_hash(&mut self, text_hash: u64) {
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

    pub(crate) fn sel_range(&self) -> Option<std::ops::Range<usize>> {
        let a = self.selection?;
        Some(a.min(self.caret)..a.max(self.caret))
    }

    pub(crate) fn repair_offset(text: &str, offset: usize) -> usize {
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
    pub(crate) fn normalize(&mut self, text: &str) {
        self.caret = Self::repair_offset(text, self.caret);
        self.selection = self
            .selection
            .map(|offset| Self::repair_offset(text, offset));
        if self.selection == Some(self.caret) {
            self.selection = None;
        }
    }
}

/// Strip line-break chars from an inbound string so the single-line
/// TextEdit's buffer never contains `\n` / `\r`. Hit by both the
/// paste path and the IME-text-commit path — host events and OS
/// clipboards routinely carry `\r\n` / `\n` from multi-line sources
/// that this widget can't render or hit-test correctly. Spaces are a
/// safer substitute than outright deletion (preserves intent for
/// "First Name\nLast Name" → "First Name Last Name"). Borrowed
/// pass-through on the common break-free case — no per-keystroke
/// allocation.
pub(crate) fn sanitize_single_line(s: &str) -> Cow<'_, str> {
    if memchr::memchr2(b'\n', b'\r', s.as_bytes()).is_none() {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut prev_was_break = false;
    for ch in s.chars() {
        if ch == '\n' || ch == '\r' {
            // Collapse `\r\n` and runs of breaks into a single space.
            if !prev_was_break {
                out.push(' ');
            }
            prev_was_break = true;
        } else {
            out.push(ch);
            prev_was_break = false;
        }
    }
    Cow::Owned(out)
}

/// Next grapheme-cluster boundary strictly after `offset` (clamped to
/// `text.len()`). Walks extended grapheme clusters via
/// [`unicode_segmentation::GraphemeCursor`] so multi-codepoint clusters
/// (combining marks, ZWJ-joined family emoji) advance as one unit.
pub(crate) fn next_grapheme_boundary(text: &str, offset: usize) -> usize {
    if offset >= text.len() {
        return text.len();
    }
    let mut cursor = unicode_segmentation::GraphemeCursor::new(offset, text.len(), true);
    cursor
        .next_boundary(text, 0)
        .ok()
        .flatten()
        .unwrap_or(text.len())
}

/// Previous grapheme-cluster boundary strictly before `offset` (clamped
/// to zero).
pub(crate) fn prev_grapheme_boundary(text: &str, offset: usize) -> usize {
    if offset == 0 {
        return 0;
    }
    let mut cursor = unicode_segmentation::GraphemeCursor::new(offset, text.len(), true);
    cursor.prev_boundary(text, 0).ok().flatten().unwrap_or(0)
}

/// Coarse char classification used by word-nav and double-click word
/// selection. Underscore is bound to `Word` so identifiers in code-like
/// text don't fragment. Codepoint-granular — fine for Latin / digit /
/// mixed text; a Unicode word-break iterator would do better on CJK
/// and friends but isn't wired yet.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CharKind {
    Whitespace,
    Word,
    Other,
}

fn char_kind(c: char) -> CharKind {
    if c.is_whitespace() {
        CharKind::Whitespace
    } else if c.is_alphanumeric() || c == '_' {
        CharKind::Word
    } else {
        CharKind::Other
    }
}

/// Forward word boundary: skip whitespace, then skip the run of
/// same-`CharKind` chars. Returns `text.len()` if `from` is already at
/// the end. The result is the byte index *just past* the end of the
/// consumed word run — same convention as `Ctrl+Right` in most editors.
pub(crate) fn next_word_boundary(text: &str, from: usize) -> usize {
    let mut chars = text[from..].char_indices();
    let mut pos;
    let target_kind = loop {
        let Some((i, c)) = chars.next() else {
            return text.len();
        };
        if char_kind(c) != CharKind::Whitespace {
            pos = from + i + c.len_utf8();
            break char_kind(c);
        }
    };
    for (i, c) in chars {
        if char_kind(c) == target_kind {
            pos = from + i + c.len_utf8();
        } else {
            break;
        }
    }
    pos
}

/// Mirror of [`next_word_boundary`]. Walks backward from `from` over
/// whitespace and then over the run of same-`CharKind` chars; returns
/// the byte index of the first consumed char (start of that run).
pub(crate) fn prev_word_boundary(text: &str, from: usize) -> usize {
    let mut rev = text[..from].char_indices().rev();
    let mut pos;
    let target_kind = loop {
        let Some((i, c)) = rev.next() else {
            return 0;
        };
        if char_kind(c) != CharKind::Whitespace {
            pos = i;
            break char_kind(c);
        }
    };
    for (i, c) in rev {
        if char_kind(c) == target_kind {
            pos = i;
        } else {
            break;
        }
    }
    pos
}

/// Word range surrounding `byte`. Returns the smallest `[start, end)`
/// such that every char in it shares one `CharKind` and `byte` lies on
/// or just past a boundary inside the run. Whitespace runs collapse to
/// `byte..byte` so a double-click on a space doesn't select the gap.
/// Used by double-click word selection.
pub(crate) fn word_range_at(text: &str, byte: usize) -> std::ops::Range<usize> {
    if text.is_empty() {
        return 0..0;
    }
    let byte = byte.min(text.len());
    // Pick the char that "anchors" this position: the one at `byte`
    // (forward) if it's word/other, otherwise the char before `byte`
    // (so a trailing-edge caret on the last char of a word still
    // selects that word).
    let forward_char = text[byte..].chars().next();
    let backward_char = text[..byte].chars().next_back();
    let anchor_kind = match (forward_char.map(char_kind), backward_char.map(char_kind)) {
        (Some(CharKind::Whitespace) | None, Some(k)) if k != CharKind::Whitespace => k,
        (Some(k), _) if k != CharKind::Whitespace => k,
        _ => return byte..byte,
    };
    // Walk left while same kind. When the anchor is the *backward*
    // char (forward char is whitespace / EOT), step `start` back over
    // it first — that char ends at `byte`, so it starts at
    // `byte - c.len_utf8()`. Otherwise the forward char is the anchor
    // and `start` already points at its start.
    let mut start = byte;
    if !forward_char.is_some_and(|c| char_kind(c) == anchor_kind)
        && let Some(c) = backward_char
    {
        start = byte - c.len_utf8();
    }
    for (i, c) in text[..start].char_indices().rev() {
        if char_kind(c) == anchor_kind {
            start = i;
        } else {
            break;
        }
    }
    // Walk right while same kind.
    let mut end = byte;
    for (i, c) in text[end..].char_indices() {
        if char_kind(c) == anchor_kind {
            end = byte + i + c.len_utf8();
        } else {
            break;
        }
    }
    start..end
}
