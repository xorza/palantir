//! Persistent text-edit state, mutation history, and Unicode navigation.

use crate::common::clipboard;
use crate::common::platform::{PLATFORM, Platform};
use crate::input::keyboard::{Key, KeyPress, Modifiers};
use crate::input::shortcut::Shortcut;
use crate::text::{SelectionRects, ShapeParams, TextShaper};
use glam::Vec2;
use std::borrow::Cow;
use std::collections::VecDeque;
use std::time::Duration;

/// Editing shortcuts, shared by the keyboard dispatch ([`dispatch_shortcut`])
/// and the default context menu, so a chord and its menu label can't drift.
pub(crate) const CUT: Shortcut = Shortcut::ctrl('X');
pub(crate) const COPY: Shortcut = Shortcut::ctrl('C');
pub(crate) const PASTE: Shortcut = Shortcut::ctrl('V');
const SELECT_ALL: Shortcut = Shortcut::ctrl('A');
const UNDO: Shortcut = Shortcut::ctrl('Z');
const REDO: Shortcut = Shortcut::ctrl_shift('Z');

/// Cross-frame state for one [`crate::widgets::text_edit::TextEdit`]. Stored in [`crate::ui::Ui`]'s
/// `WidgetId → Any` map keyed by the widget's id; lifecycle managed by
/// the same removed-widget sweep that drives the layout/text caches.
///
/// `caret` is a *byte* offset into the buffer (cosmic-text returns
/// byte cursors and `&buffer[..caret]` is the natural prefix-measure
/// path). All widget-driven mutations step grapheme-cluster boundaries
/// (which are themselves codepoint-aligned), so the caret should never
/// land mid-codepoint. Application code may replace the buffer between
/// frames, so `show()` repairs every persisted byte offset to the nearest
/// preceding UTF-8 boundary before processing input.
#[derive(Clone, Default, Debug)]
pub(crate) struct TextEditState {
    pub(crate) caret: usize,
    /// Selection anchor. `None` = no selection. Invariant: never
    /// `Some(caret)` — every mutation site collapses an empty selection
    /// to `None` so "selection live" is a single `is_some()` check.
    pub(crate) selection: Option<usize>,
    /// Lazily retained only after selection geometry spills past inline storage.
    pub(crate) selection_rects: Option<Box<SelectionRects>>,
    /// Caret byte at the rising edge of the pointer press, used as the
    /// drag anchor for click+drag selection. Reset on release.
    pub(crate) drag_anchor: Option<usize>,
    /// Was the widget focused last frame? Used to detect the
    /// focus rising edge so the caret blink resets on re-focus
    /// even when the caret position itself didn't change.
    pub(crate) prev_focused: bool,
    pub(crate) undo: VecDeque<EditSnapshot>,
    pub(crate) redo: Vec<EditSnapshot>,
    /// Kind of the most recent recorded edit, used to coalesce
    /// consecutive same-kind edits (typing chars, deleting chars) into
    /// a single undo unit. `None` after any caret-only motion so the
    /// next edit always opens a fresh group.
    pub(crate) last_edit_kind: Option<EditKind>,
    /// Viewport offset into the unscrolled text layout, in
    /// editor-local px. Single-line uses `.x` only (text wraps to
    /// inner width in multi-line so x stays at 0); multi-line uses
    /// `.y` for scroll-to-caret as content grows past the visible
    /// height. Updated each frame after input so the caret stays
    /// inside the visible area; subtracted from every shape
    /// (text / selection / caret) at emit time.
    pub(crate) scroll: Vec2,
    /// Frame-runtime clock snapshot from the last frame the caret moved, text
    /// changed, or selection shifted. The blink phase is computed
    /// against this so the caret stays solid for the first
    /// [`crate::widgets::text_edit::BLINK_HALF`] seconds after any input.
    pub(crate) last_caret_change: Duration,
}

#[derive(Clone, Debug)]
pub(crate) struct EditSnapshot {
    pub(crate) text: String,
    pub(crate) caret: usize,
    pub(crate) selection: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum EditKind {
    Typing,
    Delete,
    /// Bulk edits (paste, cut, clear, newline insert) — never coalesce.
    Other,
}

const UNDO_LIMIT: usize = 128;

/// One frame's editing session: the host-owned buffer, the widget's
/// cross-frame [`TextEditState`] row, and the config that gates
/// mutations — everything the edit / nav / clipboard paths need,
/// bundled so they read as methods instead of free functions each
/// threading the same five parameters and an `&mut bool` out-flag.
#[derive(Debug)]
pub(crate) struct Editor<'a> {
    pub(crate) text: &'a mut String,
    pub(crate) state: &'a mut TextEditState,
    pub(crate) multiline: bool,
    max_chars: Option<usize>,
    /// The buffer was mutated this session (typing, delete, paste,
    /// cut, undo/redo). Set by the mutation choke points, so it's
    /// content-accurate — a same-length overwrite still reports.
    pub(crate) edited: bool,
}

impl<'a> Editor<'a> {
    pub(crate) fn new(
        text: &'a mut String,
        state: &'a mut TextEditState,
        multiline: bool,
        max_chars: Option<usize>,
    ) -> Self {
        Self {
            text,
            state,
            multiline,
            max_chars,
            edited: false,
        }
    }

    /// Undo snapshot of the current buffer + caret/selection.
    fn snapshot(&self) -> EditSnapshot {
        EditSnapshot {
            text: self.text.clone(),
            caret: self.state.caret,
            selection: self.state.selection,
        }
    }

    /// Open (or coalesce into) an undo unit before a mutation.
    /// Consecutive same-kind `Typing` / `Delete` edits merge into one
    /// unit; `Other` never coalesces. Any redo tail is invalidated.
    fn record_edit(&mut self, kind: EditKind) {
        let coalesce = kind != EditKind::Other
            && self.state.last_edit_kind == Some(kind)
            && !self.state.undo.is_empty();
        if !coalesce {
            if self.state.undo.len() >= UNDO_LIMIT {
                self.state.undo.pop_front();
            }
            let snap = self.snapshot();
            self.state.undo.push_back(snap);
        }
        self.state.redo.clear();
        self.state.last_edit_kind = Some(kind);
    }

    fn apply_history(&mut self, snap: EditSnapshot) {
        assert!(snap.caret <= snap.text.len());
        assert!(snap.selection.is_none_or(|s| s <= snap.text.len()));
        *self.text = snap.text;
        self.state.caret = snap.caret;
        self.state.selection = snap.selection.filter(|&a| a != snap.caret);
        self.state.last_edit_kind = None;
        self.edited = true;
    }

    /// No-op on an empty stack.
    fn undo(&mut self) {
        if let Some(snap) = self.state.undo.pop_back() {
            let cur = self.snapshot();
            self.state.redo.push(cur);
            self.apply_history(snap);
        }
    }

    /// No-op on an empty stack.
    fn redo(&mut self) {
        if let Some(snap) = self.state.redo.pop() {
            let cur = self.snapshot();
            self.state.undo.push_back(cur);
            self.apply_history(snap);
        }
    }

    /// Delete the live selection range (if any), landing the caret at
    /// its start. Returns whether anything was deleted — callers use
    /// it to know whether to skip a subsequent codepoint-delete
    /// (Backspace/Delete).
    fn delete_selection(&mut self) -> bool {
        let Some(range) = self.state.sel_range() else {
            return false;
        };
        let start = range.start;
        self.text.replace_range(range, "");
        self.state.caret = start;
        self.state.selection = None;
        true
    }

    /// Portion of `s` that fits after deleting the live selection.
    /// The cap is by character count; the returned prefix remains on
    /// a UTF-8 boundary.
    fn capped_prefix<'s>(&self, s: &'s str) -> &'s str {
        match self.max_chars {
            Some(max) => {
                let selected_chars = self
                    .state
                    .sel_range()
                    .map_or(0, |range| self.text[range].chars().count());
                let chars_after_delete = self.text.chars().count() - selected_chars;
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
        let fit = self.capped_prefix(s);
        if self.state.selection.is_none() && fit.is_empty() {
            return;
        }
        self.record_edit(kind);
        self.delete_selection();
        if !fit.is_empty() {
            self.text.insert_str(self.state.caret, fit);
            self.state.caret += fit.len();
        }
        self.edited = true;
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

    /// Cut the live selection to the clipboard. No-op without one.
    pub(crate) fn cut(&mut self) {
        let Some(r) = self.state.sel_range() else {
            return;
        };
        if clipboard::set(&self.text[r.clone()]).is_err() {
            return;
        }
        self.record_edit(EditKind::Other);
        self.text.replace_range(r.clone(), "");
        self.state.caret = r.start;
        self.state.selection = None;
        self.edited = true;
    }

    /// Copy the live selection to the clipboard. No-op without one.
    pub(crate) fn copy(&self) {
        if let Some(r) = self.state.sel_range() {
            let _ = clipboard::set(&self.text[r]);
        }
    }

    /// Clear the whole buffer (the context menu's Clear).
    pub(crate) fn clear(&mut self) {
        if !self.text.is_empty() {
            self.record_edit(EditKind::Other);
            self.text.clear();
            self.state.caret = 0;
            self.state.selection = None;
            self.edited = true;
        }
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
    fn move_caret(&mut self, new_caret: usize, extend: bool) {
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

    /// Route platform shortcuts (undo / redo / select-all / cut /
    /// copy / paste) before keyboard edit dispatch. Returns `true`
    /// when `kp` was claimed; the caller skips [`Self::apply_key`] for
    /// that key. Undo/redo always fire; clipboard + select-all are
    /// suppressed when a context menu owns the same bindings
    /// (`menu_open == true`).
    pub(crate) fn dispatch_shortcut(&mut self, kp: KeyPress, menu_open: bool) -> bool {
        if UNDO.matches(kp) {
            self.undo();
            return true;
        }
        if REDO.matches(kp) {
            self.redo();
            return true;
        }
        if menu_open {
            return false;
        }
        if SELECT_ALL.matches(kp) {
            self.select_all();
            return true;
        }
        if COPY.matches(kp) {
            self.copy();
            return true;
        }
        if CUT.matches(kp) {
            self.cut();
            return true;
        }
        if PASTE.matches(kp) {
            self.paste(&clipboard::get());
            return true;
        }
        false
    }

    /// Apply one keypress to the buffer + state. Recognized keys are
    /// consumed silently except the two [`KeyOutcome`]s the caller
    /// must act on: Escape asked to blur, or Up/Down needs the
    /// shaper's 2D layout — which this pure buffer+state method
    /// deliberately doesn't carry. Platform shortcuts (undo /
    /// clipboard / select-all) are handled by [`Self::dispatch_shortcut`]
    /// before this; `multiline` toggles Enter → `\n` insertion and
    /// enables Up/Down motion.
    pub(crate) fn apply_key(&mut self, kp: KeyPress) -> KeyOutcome {
        let shift = kp.mods.shift;
        match kp.key {
            Key::Char(c) if !kp.mods.any_command() => {
                let mut buf = [0u8; 4];
                self.replace_selection(c.encode_utf8(&mut buf), EditKind::Typing);
            }
            Key::Backspace => {
                if self.state.selection.is_some() || self.state.caret > 0 {
                    self.record_edit(EditKind::Delete);
                    if !self.delete_selection() {
                        let prev = prev_grapheme_boundary(self.text, self.state.caret);
                        self.text.replace_range(prev..self.state.caret, "");
                        self.state.caret = prev;
                    }
                    self.edited = true;
                }
            }
            Key::Delete => {
                if self.state.selection.is_some() || self.state.caret < self.text.len() {
                    self.record_edit(EditKind::Delete);
                    if !self.delete_selection() {
                        let next = next_grapheme_boundary(self.text, self.state.caret);
                        self.text.replace_range(self.state.caret..next, "");
                    }
                    self.edited = true;
                }
            }
            Key::ArrowLeft if is_word_nav(kp.mods) => {
                let target = prev_word_boundary(self.text, self.state.caret);
                self.move_caret(target, shift);
            }
            Key::ArrowRight if is_word_nav(kp.mods) => {
                let target = next_word_boundary(self.text, self.state.caret);
                self.move_caret(target, shift);
            }
            Key::ArrowLeft => {
                let target = if !shift && let Some(r) = self.state.sel_range() {
                    r.start
                } else {
                    prev_grapheme_boundary(self.text, self.state.caret)
                };
                self.move_caret(target, shift);
            }
            Key::ArrowRight => {
                let target = if !shift && let Some(r) = self.state.sel_range() {
                    r.end
                } else {
                    next_grapheme_boundary(self.text, self.state.caret)
                };
                self.move_caret(target, shift);
            }
            Key::ArrowUp if self.multiline => {
                return KeyOutcome::Vertical {
                    up: true,
                    extend: shift,
                };
            }
            Key::ArrowDown if self.multiline => {
                return KeyOutcome::Vertical {
                    up: false,
                    extend: shift,
                };
            }
            Key::Enter if self.multiline => {
                self.replace_selection("\n", EditKind::Other);
            }
            Key::Home => self.move_caret(0, shift),
            Key::End => self.move_caret(self.text.len(), shift),
            Key::Escape => {
                // Two-stage: collapse selection first, blur only when
                // there's no selection to drop.
                if self.state.selection.is_some() {
                    self.state.selection = None;
                    self.state.last_edit_kind = None;
                } else {
                    return KeyOutcome::Blur;
                }
            }
            _ => {}
        }
        KeyOutcome::None
    }

    /// Resolve an Up/Down [`KeyOutcome::Vertical`] against the
    /// shaper's 2D layout: probe one line above/below the caret's
    /// current x and move the caret there (extending the selection if
    /// `extend`). Up from the first line snaps to byte 0.
    pub(crate) fn resolve_vertical(
        &mut self,
        shaper: &TextShaper,
        params: ShapeParams,
        up: bool,
        extend: bool,
    ) {
        let pos = shaper.cursor_xy(self.text, self.state.caret, params);
        let target = if up && pos.y_top <= 0.5 {
            0
        } else {
            let probe_y = if up {
                pos.y_top - 1.0
            } else {
                pos.y_top + pos.line_height + 1.0
            };
            shaper.byte_at_xy(self.text, pos.x, probe_y, params)
        };
        self.move_caret(target, extend);
    }
}

/// Non-edit outcome of one keypress that the dispatcher must act on:
/// Escape asked to blur (applied by `show()` after the node closes),
/// or Up/Down needs resolving against the shaper's 2D layout via
/// [`Editor::resolve_vertical`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum KeyOutcome {
    None,
    Blur,
    Vertical { up: bool, extend: bool },
}

impl TextEditState {
    pub(crate) fn sel_range(&self) -> Option<std::ops::Range<usize>> {
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
    pub(crate) fn normalize(&mut self, text: &str) {
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
    if !s.contains(['\n', '\r']) {
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

/// Word-nav modifier: Alt (Option) on macOS, Ctrl elsewhere — matches
/// the platform conventions every desktop text field follows. Shift may
/// be held in addition (selection-extending word nav).
fn is_word_nav(m: Modifiers) -> bool {
    // `m.ctrl` is the platform primary command bit (= Cmd on macOS).
    match PLATFORM {
        // macOS: Option (Alt) + arrow, with Cmd not held.
        Platform::Mac => m.alt && !m.ctrl,
        // Elsewhere: Ctrl + arrow, with Alt not held.
        _ => m.ctrl && !m.alt,
    }
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
