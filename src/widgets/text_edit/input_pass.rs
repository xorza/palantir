//! One frame's pointer and keyboard dispatch for a TextEdit.

use crate::common::platform::{PLATFORM, Platform};
use crate::input::interaction::response_state::ResponseState;
use crate::input::key_class::KeyFilter;
use crate::input::keyboard::key::Key;
use crate::input::keyboard::key_press::KeyPress;
use crate::input::keyboard::modifiers::Modifiers;
use crate::text::probe::{Caret, TextProbe};
use crate::ui::Ui;
use crate::widgets::text_edit::TextEditState;
use crate::widgets::text_edit::action::EditAction;
use crate::widgets::text_edit::edit_state::EditKind;
use crate::widgets::text_edit::editor::Editor;
use crate::widgets::text_edit::shape_ctx::ShapeCtx;
use crate::widgets::text_edit::text_layout::TextLayout;

/// Result of one frame's input pass: edge signals `show()` folds into
/// [`crate::widgets::text_edit::TextEditResponse`].
#[derive(Debug)]
pub(super) struct InputResult {
    /// Escape canceled the edit; blurs before view recording.
    pub(super) canceled: bool,
    /// Enter accepted a single-line value this frame.
    pub(super) submitted: bool,
    /// The buffer was mutated this frame. Set by the mutation choke points, so
    /// a same-length overwrite counts.
    pub(super) edited: bool,
}

/// What the builder configured about accepting input, as opposed to
/// rendering it. Unrelated to [`crate::InputPolicy`], which gates re-recording.
#[derive(Clone, Copy, Debug)]
pub(super) struct AcceptPolicy {
    /// Cap on buffer length; `None` is unbounded.
    pub(super) max_chars: Option<usize>,
    /// Select everything when focus lands without a same-frame press.
    pub(super) select_all_on_focus: bool,
    /// The key classes this field consumes: the value its scope declares, so
    /// the two cannot drift.
    pub(super) filter: KeyFilter,
}

/// Everything one frame's input pass reads or writes.
///
/// The state row is a plain `&mut` because `Editor` holds it across the
/// keyboard drain, which hands `&mut Ui` to each handler. `TextEdit::show`
/// moves the row out for the pass, so its write-back must be unconditional.
#[derive(Debug)]
pub(super) struct InputPass<'a> {
    pub(super) resp_state: &'a ResponseState,
    pub(super) is_focused: bool,
    pub(super) text: &'a mut String,
    pub(super) layout: &'a TextLayout,
    pub(super) policy: AcceptPolicy,
    pub(super) state: &'a mut TextEditState,
}

impl InputPass<'_> {
    /// Process this frame's pointer and keyboard input; returns the edge
    /// signals. Touches input streams and text probes, never shape/tree storage.
    pub(super) fn run(self, ui: &mut Ui) -> InputResult {
        let InputPass {
            resp_state,
            is_focused,
            text,
            layout,
            policy,
            state,
        } = self;
        let ctx = &layout.ctx;
        let AcceptPolicy {
            max_chars,
            select_all_on_focus,
            filter,
        } = policy;
        let mut canceled = false;
        let mut submitted = false;
        let clipboard = ui.clipboard();

        let TextEditState {
            edit,
            view,
            // Filled by the geometry pass after this one.
            selection_rects: _,
            placeholder: _,
            commit_pending: _,
            preedit: _,
            preedit_cursor: _,
            display: _,
            composing: _,
        } = state;
        let was_focused = view.was_focused();
        // Repair persisted byte offsets: application code may have replaced
        // `*text` with a string whose UTF-8 boundaries differ.
        edit.normalize(text);
        let mut ed = Editor::new(text, edit, ctx.multiline, max_chars);
        ed.enforce_single_line();

        // Select all on the frame focus lands, unless a press places the caret.
        if select_all_on_focus && is_focused && !was_focused && !resp_state.left.held() {
            ed.select_all();
        }

        // Click and drag-to-select. Gated on `held`, not `pressed`: a drag must
        // keep its anchor while the pointer is outside the rect or off the
        // surface. With no `pointer_local` the anchor is kept, not cleared.
        if resp_state.left.held()
            && let Some(pointer_offset) = resp_state.pointer_local
        {
            // Hit-testing runs against the unscrolled layout, so add back last
            // frame's scroll and block offset: the user clicked on what they saw.
            let [pad_l, pad_t, _, _] = ctx.padding.as_array();
            let block = layout.prev_block_offset;
            let local_x = pointer_offset.x - pad_l - block.x + view.scroll.offset.x;
            let local_y = pointer_offset.y - pad_t - block.y + view.scroll.offset.y;
            // Single-line probes at `y=0`.
            let hit = ui
                .probe_text(ctx.run(ed.text()))
                .byte_at(local_x, if ctx.multiline { local_y } else { 0.0 });
            let clicks = resp_state.left.press_count();
            if clicks > 0 {
                ed.press(hit, clicks, ui.peek_modifiers().shift);
            } else {
                ed.drag_to(hit);
            }
        } else if !resp_state.left.held() {
            ed.end_drag();
        }

        if !is_focused {
            ed.normalize();
            return InputResult {
                canceled,
                submitted,
                edited: ed.edited(),
            };
        }

        // Drain presses in arrival order, by index so probes can borrow `ui`:
        // shared edit actions first, then `apply_key`.
        for i in 0..ui.keyboard_events().len() {
            let kp = ui.keyboard_events()[i];
            if !filter.takes_press(kp) {
                continue;
            }
            // Single-line Enter submits without editing the buffer.
            if !ed.multiline() && kp.key == Key::Enter && !kp.mods.has_command() {
                submitted = true;
                assert_only_repeats_after(ui.keyboard_events(), i);
                break;
            }
            if let Some(action) = EditAction::from_keypress(kp) {
                action.execute(&mut ed, &clipboard);
                continue;
            }
            match apply_key(&mut ed, kp) {
                KeyOutcome::Blur => {
                    canceled = true;
                    assert_only_repeats_after(ui.keyboard_events(), i);
                    break;
                }
                KeyOutcome::Vertical { up, extend } => {
                    resolve_vertical(&mut ed, ui, ctx, up, extend);
                }
                KeyOutcome::LineEdge { end, extend } => {
                    resolve_line_edge(&mut ed, ui, ctx, end, extend);
                }
                KeyOutcome::None => {}
            }
        }

        ed.normalize();
        InputResult {
            canceled,
            submitted,
            edited: ed.edited(),
        }
    }
}

/// Assert that only repeats of the key at `terminal` follow it. `InputQueue`
/// holds every press after a command key for the next frame, so only repeats
/// can follow; the assert catches a change to that rule.
fn assert_only_repeats_after(events: &[KeyPress], terminal: usize) {
    let key = events[terminal].key;
    debug_assert!(
        events[terminal + 1..]
            .iter()
            .all(|press| press.repeat && press.key == key),
        "keys after a terminal {key:?} reached the field in its frame: {:?}",
        &events[terminal + 1..],
    );
}

pub(super) fn apply_key(editor: &mut Editor<'_>, keypress: KeyPress) -> KeyOutcome {
    let extend = keypress.mods.shift;
    let line_nav = is_line_nav(keypress.mods);
    match keypress.key {
        Key::Backspace if line_nav => editor.delete_to_line_start(),
        Key::Backspace => editor.delete_backward(),
        Key::Delete => editor.delete_forward(),
        Key::ArrowLeft if is_word_nav(keypress.mods) => editor.move_word_left(extend),
        Key::ArrowRight if is_word_nav(keypress.mods) => editor.move_word_right(extend),
        Key::ArrowLeft if line_nav && editor.multiline() => {
            return KeyOutcome::LineEdge { end: false, extend };
        }
        Key::ArrowRight if line_nav && editor.multiline() => {
            return KeyOutcome::LineEdge { end: true, extend };
        }
        Key::ArrowLeft if line_nav => editor.move_caret(0, extend),
        Key::ArrowRight if line_nav => editor.move_caret(editor.text().len(), extend),
        // Document ends: Ctrl+Home/End, and Cmd+Up/Down on macOS.
        Key::Home | Key::ArrowUp if is_document_nav(keypress) => editor.move_caret(0, extend),
        Key::End | Key::ArrowDown if is_document_nav(keypress) => {
            editor.move_caret(editor.text().len(), extend);
        }
        Key::ArrowLeft => editor.move_grapheme_left(extend),
        Key::ArrowRight => editor.move_grapheme_right(extend),
        Key::ArrowUp if editor.multiline() => {
            return KeyOutcome::Vertical { up: true, extend };
        }
        Key::ArrowDown if editor.multiline() => {
            return KeyOutcome::Vertical { up: false, extend };
        }
        Key::Enter if editor.multiline() => editor.replace_selection("\n", EditKind::Other),
        // Multi-line Home / End mean the visual line.
        Key::Home if editor.multiline() => return KeyOutcome::LineEdge { end: false, extend },
        Key::End if editor.multiline() => return KeyOutcome::LineEdge { end: true, extend },
        Key::Home => editor.move_caret(0, extend),
        Key::End => editor.move_caret(editor.text().len(), extend),
        // Escape peels one layer: selection first, then focus.
        Key::Escape => {
            let had_selection = editor.collapse_selection();
            if !had_selection {
                return KeyOutcome::Blur;
            }
        }
        // The press's text is what gets typed (the platform resolved layout,
        // dead keys and modifiers). Named keys above answer first. Command
        // chords type nothing even if the platform reports text, as macOS
        // does for Cmd+A (`KeyPress::types_text`).
        _ if keypress.types_text() => {
            editor.insert_str(keypress.text.as_str());
        }
        _ => {}
    }
    KeyOutcome::None
}

/// Move the caret to the offset `target` picks from the caret's current
/// position. Both queries share one `probe_text` borrow and cache dispatch.
fn move_caret_by_probe(
    editor: &mut Editor<'_>,
    ui: &mut Ui,
    ctx: &ShapeCtx,
    extend: bool,
    target: impl FnOnce(&TextProbe<'_>, Caret) -> usize,
) {
    let byte = {
        let probe = ui.probe_text(ctx.run(editor.text()));
        let pos = probe.caret_at(editor.caret());
        target(&probe, pos)
    };
    editor.move_caret(byte, extend);
}

fn resolve_vertical(editor: &mut Editor<'_>, ui: &mut Ui, ctx: &ShapeCtx, up: bool, extend: bool) {
    move_caret_by_probe(editor, ui, ctx, extend, |probe, pos| {
        if up && pos.y_top <= 0.5 {
            return 0;
        }
        let probe_y = if up {
            pos.y_top - 1.0
        } else {
            pos.y_top + pos.line_height + 1.0
        };
        probe.byte_at(pos.x, probe_y)
    });
}

/// Home / End on the visual line: a soft-wrapped line has no `\n` to scan for,
/// so hit-test past each end of the caret's row, which `byte_at` clamps.
fn resolve_line_edge(
    editor: &mut Editor<'_>,
    ui: &mut Ui,
    ctx: &ShapeCtx,
    end: bool,
    extend: bool,
) {
    move_caret_by_probe(editor, ui, ctx, extend, |probe, pos| {
        // Mid-row, so the hit cannot fall to a neighbouring line.
        let y = pos.y_top + pos.line_height * 0.5;
        let x = if end { probe.size().w + 1.0 } else { -1.0 };
        probe.byte_at(x, y)
    });
}

/// macOS line chords: Cmd+Left / Right and Cmd+Backspace (`Modifiers::ctrl`
/// is Cmd there).
fn is_line_nav(modifiers: Modifiers) -> bool {
    PLATFORM == Platform::Mac && modifiers.ctrl && !modifiers.alt
}

/// Ctrl with Home / End everywhere, Cmd with Up / Down on macOS.
fn is_document_nav(keypress: KeyPress) -> bool {
    let mods = keypress.mods;
    if !mods.ctrl || mods.alt {
        return false;
    }
    match keypress.key {
        Key::Home | Key::End => true,
        Key::ArrowUp | Key::ArrowDown => PLATFORM == Platform::Mac,
        _ => false,
    }
}

/// Alt on macOS (Cmd is the line chord), Ctrl elsewhere.
const WORD_NAV: Modifiers = match PLATFORM {
    Platform::Mac => Modifiers::ALT,
    _ => Modifiers::CTRL,
};

/// [`WORD_NAV`] held without the other of Ctrl and Alt.
const fn is_word_nav(modifiers: Modifiers) -> bool {
    modifiers.ctrl == WORD_NAV.ctrl && modifiers.alt == WORD_NAV.alt
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum KeyOutcome {
    None,
    Blur,
    Vertical {
        up: bool,
        extend: bool,
    },
    /// Home / End in a multi-line editor: the visual line's edge.
    LineEdge {
        end: bool,
        extend: bool,
    },
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::input::keyboard::modifiers::Modifiers;

    /// The platform's word-motion chord, for the cases that press it.
    pub(crate) const WORD_NAV: Modifiers = super::WORD_NAV;
}
