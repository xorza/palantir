//! One frame's pointer and keyboard dispatch for a TextEdit.

use crate::common::platform::{PLATFORM, Platform};
use crate::input::key_class::KeyFilter;
use crate::input::keyboard::key::Key;
use crate::input::keyboard::key_press::KeyPress;
use crate::input::keyboard::keyboard_event::KeyboardEvent;
use crate::input::keyboard::modifiers::Modifiers;
use crate::input::response::response_state::ResponseState;
use crate::text::probe::{Caret, TextProbe};
use crate::ui::Ui;
use crate::widgets::text_edit::TextEditState;
use crate::widgets::text_edit::action::EditAction;
use crate::widgets::text_edit::edit_state::EditKind;
use crate::widgets::text_edit::editor::Editor;
use crate::widgets::text_edit::shape_ctx::ShapeCtx;
use crate::widgets::text_edit::text_layout::TextLayout;

/// Result of one frame's input pass over a TextEdit: the caret byte,
/// the (sorted) selection range for the painter, and the edge signals
/// `show()` folds into [`crate::widgets::text_edit::TextEditResponse`].
#[derive(Debug)]
pub(super) struct InputResult {
    /// Escape cancelled the edit, which also blurs before view
    /// recording.
    pub(super) cancelled: bool,
    /// Enter accepted a single-line value this frame.
    pub(super) submitted: bool,
    /// The buffer was mutated this frame (typing, delete, paste, cut,
    /// undo/redo). Reported by the mutation choke points, so it's
    /// content-accurate — a same-length overwrite still counts, unlike
    /// a length-delta proxy.
    pub(super) edited: bool,
}

/// What the builder configured about *accepting* input, as opposed to
/// rendering it. The three travel together because they are set on the
/// same builder and read at the same call.
///
/// Not to be confused with [`crate::InputPolicy`], which is unrelated —
/// that one gates whether a frame re-records at all. This is per-widget
/// and concerns which keystrokes a field takes.
#[derive(Clone, Copy, Debug)]
pub(super) struct AcceptPolicy {
    /// Cap on buffer length; `None` is unbounded.
    pub(super) max_chars: Option<usize>,
    /// Select everything when focus lands without a same-frame press.
    pub(super) select_all_on_focus: bool,
    /// The key classes this field consumes — the same value its scope
    /// declares, so what it tells other readers it takes and what it
    /// acts on cannot drift apart.
    pub(super) filter: KeyFilter,
}

/// Everything one frame's input pass reads or writes, bundled the way
/// this module's [`LayoutInput`](super::text_layout::LayoutInput) and
/// [`GeometryInput`](super::text_geometry::GeometryInput) siblings are.
///
/// Holds the state row as a plain `&mut` rather than reaching for it
/// through `ui`. `Editor` holds it mutably across the keyboard drain,
/// and the drain hands `&mut Ui` back to each handler, so a row borrowed
/// *from* `ui` could not survive it. `TextEdit::show` moves the row out
/// once for the whole pass, which is what makes this possible *and* is
/// why the caller's write-back has to be unconditional.
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
    /// Process this frame's pointer + keyboard input and return the
    /// caret + selection to render plus the frame's edge signals.
    /// Separate from `TextEdit::show` so the borrow choreography stays
    /// contained: this touches the input streams and text probes, never
    /// the shape/tree storage.
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
        let mut cancelled = false;
        let mut submitted = false;
        let clipboard = ui.clipboard();

        let TextEditState {
            edit,
            view,
            // Filled by the geometry pass after this one, and read only by the
            // painter — the input pass has no business in it.
            selection_rects: _,
        } = state;
        let was_focused = view.was_focused();
        // Repair persisted byte offsets before any range/slice operation.
        // Application code may have replaced `*text` with a same-length or
        // longer string whose UTF-8 boundaries differ from the prior frame.
        edit.normalize(text);
        let mut ed = Editor::new(text, edit, ctx.multiline, max_chars);
        ed.enforce_single_line();

        // Select-all-on-focus: the frame focus lands (and no press this frame — a
        // press falls through to place the caret below).
        if select_all_on_focus && is_focused && !was_focused && !resp_state.left.held() {
            ed.select_all();
        }

        // Click + drag-to-select. What each edge means to the selection is
        // `Editor::press` / `drag_to` / `end_drag`; what reaches them is
        // here.
        //
        // Gated on `held` (capture-based), not `pressed` (which also demands
        // the pointer stay *over* the widget): a drag-select must keep
        // tracking — and keep its anchor — while the pointer drags outside
        // the editor's rect or off the surface. `held` stays true from press
        // to release regardless of pointer position, so the caret follows the
        // clamped hit (byte 0 / end-of-text) and the selection grows instead
        // of freezing and dropping the anchor at the edge. When the pointer
        // has left the surface (`pointer_local == None`) the inner `let` fails
        // and we fall through *without* clearing the anchor — the gesture is
        // still live, just position-less this frame.
        if resp_state.left.held()
            && let Some(pointer_offset) = resp_state.pointer_local
        {
            // Hit-test runs against the *unscrolled* shaped layout, so we
            // add last frame's scroll back into the pointer's local coords.
            // Both offsets are last frame's for the same reason: the user
            // clicked on what they saw, and this frame's block offset and
            // scroll are computed after this pass returns.
            let [pad_l, pad_t, _, _] = ctx.padding.as_array();
            let block = layout.prev_block_offset;
            let local_x = pointer_offset.x - pad_l - block.x + view.scroll.offset.x;
            let local_y = pointer_offset.y - pad_t - block.y + view.scroll.offset.y;
            // `byte_at_xy` handles both axes; single-line probes at
            // `y=0` (against an unwrapped layout) collapse to cosmic's
            // 1D `Buffer::hit` walk — one shaped lookup.
            let hit = ui
                .probe_text(ctx.run(ed.text()))
                .byte_at(local_x, if ctx.multiline { local_y } else { 0.0 });
            let clicks = resp_state.left.press_count();
            if clicks > 0 {
                ed.press(hit, clicks);
            } else {
                ed.drag_to(hit);
            }
        } else if !resp_state.left.held() {
            ed.end_drag();
        }

        if !is_focused {
            ed.normalize();
            return InputResult {
                cancelled,
                submitted,
                edited: ed.edited(),
            };
        }

        // Drain the unified keyboard event stream in arrival order:
        // Text chunks splice into the buffer (sanitized for single-line);
        // Down events route through shared edit actions (clipboard / undo)
        // then `apply_key` (edit / nav). Vertical-nav probes happen inline
        // because they need a text probe, which is what `Ui`'s walk keeps
        // the borrow free for.
        ui.each_keyboard_event(|ui, event| {
            let Some(event) = filter.accepts(event) else {
                return;
            };
            match event {
                KeyboardEvent::Text(chunk) => {
                    let to_insert = ed.sanitized(chunk.as_str());
                    if !to_insert.is_empty() {
                        ed.replace_selection(&to_insert, EditKind::Typing);
                    }
                }
                KeyboardEvent::Down(kp) => {
                    // Single-line Enter is a *submit* signal, not an edit: the buffer
                    // is left untouched (multi-line handles `\n` in `apply_key`), but
                    // the caller learns the user accepted the value.
                    if !ed.multiline() && kp.key == Key::Enter && !kp.mods.any_command() {
                        submitted = true;
                        return;
                    }
                    if let Some(action) = EditAction::from_keypress(kp) {
                        action.execute(&mut ed, &clipboard);
                        return;
                    }
                    match apply_key(&mut ed, kp) {
                        KeyOutcome::Blur => cancelled = true,
                        KeyOutcome::Vertical { up, extend } => {
                            resolve_vertical(&mut ed, ui, ctx, up, extend);
                        }
                        KeyOutcome::LineEdge { end, extend } => {
                            resolve_line_edge(&mut ed, ui, ctx, end, extend);
                        }
                        KeyOutcome::None => {}
                    }
                }
            }
        });

        ed.normalize();
        InputResult {
            cancelled,
            submitted,
            edited: ed.edited(),
        }
    }
}

pub(super) fn apply_key(editor: &mut Editor<'_>, keypress: KeyPress) -> KeyOutcome {
    let extend = keypress.mods.shift;
    match keypress.key {
        Key::Char(c) if !keypress.mods.any_command() => editor.insert_char(c),
        Key::Backspace => editor.delete_backward(),
        Key::Delete => editor.delete_forward(),
        Key::ArrowLeft if is_word_nav(keypress.mods) => editor.move_word_left(extend),
        Key::ArrowRight if is_word_nav(keypress.mods) => editor.move_word_right(extend),
        Key::ArrowLeft => editor.move_grapheme_left(extend),
        Key::ArrowRight => editor.move_grapheme_right(extend),
        Key::ArrowUp if editor.multiline() => {
            return KeyOutcome::Vertical { up: true, extend };
        }
        Key::ArrowDown if editor.multiline() => {
            return KeyOutcome::Vertical { up: false, extend };
        }
        Key::Enter if editor.multiline() => editor.replace_selection("\n", EditKind::Other),
        // A multi-line editor's Home / End belong to the *visual* line,
        // like the arrows above — jumping to the buffer's ends is what a
        // single-line field means by them, and there the two agree.
        Key::Home if editor.multiline() => return KeyOutcome::LineEdge { end: false, extend },
        Key::End if editor.multiline() => return KeyOutcome::LineEdge { end: true, extend },
        Key::Home => editor.move_caret(0, extend),
        Key::End => editor.move_caret(editor.text().len(), extend),
        Key::Escape if !editor.collapse_selection() => return KeyOutcome::Blur,
        Key::Escape => {}
        _ => {}
    }
    KeyOutcome::None
}

/// Move the caret to the offset `target` picks from a point relative to
/// where it sits now.
///
/// Both queries sit under one `probe_text`: the caret position and the
/// hit resolve under a single shaper borrow and one cache dispatch, which
/// is exactly what the scoped probe is for. Every caret motion that needs
/// the shaped buffer — the ones a byte scan cannot answer — goes through
/// here, so none of them re-states that discipline.
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

/// Home / End on the visual line the caret sits on.
///
/// A soft-wrapped line has no `\n` to scan for, so its edges are a probe
/// question the way [`resolve_vertical`]'s target is: hit-test past each
/// end of the caret's own row, which `byte_at` clamps back to that row's
/// first or last offset.
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

fn is_word_nav(modifiers: Modifiers) -> bool {
    match PLATFORM {
        Platform::Mac => modifiers.alt && !modifiers.ctrl,
        _ => modifiers.ctrl && !modifiers.alt,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum KeyOutcome {
    None,
    Blur,
    Vertical {
        up: bool,
        extend: bool,
    },
    /// Home / End in a multi-line editor: the visual line's edge, which
    /// only the shaped buffer knows.
    LineEdge {
        end: bool,
        extend: bool,
    },
}
