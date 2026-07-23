//! Per-frame pointer and keyboard dispatch for TextEdit.

use crate::common::clipboard::Clipboard;
use crate::common::platform::{PLATFORM, Platform};
use crate::input::keyboard::{Key, KeyPress, KeyboardEvent, Modifiers};
use crate::primitives::widget_id::WidgetId;
use crate::text::{ShapeParams, TextShaper};
use crate::ui::Ui;
use crate::widgets::text_edit::TextEditState;
use crate::widgets::text_edit::action::EditAction;
use crate::widgets::text_edit::model::{EditKind, Editor, word_range_at};
use crate::widgets::text_edit::view::ShapeCtx;

/// Result of one frame's input pass over a TextEdit: the caret byte,
/// the (sorted) selection range for the painter, and the edge signals
/// `show()` folds into [`crate::widgets::text_edit::TextEditResponse`].
#[derive(Debug)]
pub(crate) struct InputResult {
    /// Caret or selection differ from their pre-input values (compared
    /// against the pre-clamp snapshot, so an external buffer shrink
    /// that displaces the caret also reads as motion) — drives the
    /// blink-phase reset.
    pub(crate) caret_moved: bool,
    /// The view state's focus bit before this pass. `show()` derives
    /// gained/lost edges before the view update stores the new value.
    pub(crate) was_focused: bool,
    /// Escape asked to blur before view recording.
    pub(crate) blur: bool,
    /// Enter accepted a single-line value this frame.
    pub(crate) submitted: bool,
    /// The buffer was mutated this frame (typing, delete, paste, cut,
    /// undo/redo). Reported by the mutation choke points, so it's
    /// content-accurate — a same-length overwrite still counts, unlike
    /// a length-delta proxy.
    pub(crate) edited: bool,
}

/// Process this frame's pointer + keyboard input for one TextEdit
/// widget and return the caret + selection to render plus the frame's
/// edge signals. Splitting this out of `show()` keeps the borrow
/// choreography contained: we touch `ui.state`, `ui.input`, and
/// `ui.resources.text` here, but never the shape/tree storage.
pub(crate) fn handle_input(
    ui: &mut Ui,
    id: WidgetId,
    is_focused: bool,
    text: &mut String,
    ctx: &ShapeCtx,
    max_chars: Option<usize>,
    select_all_on_focus: bool,
) -> InputResult {
    let mut blur = false;
    let mut submitted = false;
    let resp_state = ui.response_for(id);
    let clipboard = ui.resources.clipboard.clone();

    // Hold the state row once for the whole function (inside the
    // `Editor`). `ui.state`, `ui.input`, and `ui.resources.text` are
    // disjoint fields of `Ui`, so the `&mut state` can stay alive
    // while also reading the input queues and dispatching to the text
    // measurer.
    let state = ui
        .state
        .get_or_insert_with::<TextEditState, _>(id, Default::default);
    let TextEditState {
        edit,
        interaction,
        view,
    } = state;
    // Pre-input snapshot for the `caret_moved` / focus edges — taken
    // before the clamp so an external buffer shrink that displaces the
    // caret still reads as caret motion (blink reset).
    let caret_before = edit.caret;
    let sel_before = edit.selection;
    let was_focused = view.prev_focused;
    // Repair persisted byte offsets before any range/slice operation.
    // Application code may have replaced `*text` with a same-length or
    // longer string whose UTF-8 boundaries differ from the prior frame.
    edit.normalize(text);
    interaction.normalize(text);
    let mut ed = Editor::new(text, edit, ctx.multiline, max_chars);
    ed.enforce_single_line();

    // Select-all-on-focus: the frame focus lands (and no press this frame — a
    // press falls through to place the caret below).
    if select_all_on_focus && is_focused && !was_focused && !resp_state.left.held() {
        ed.select_all();
    }

    // Click + drag-to-select. On the rising edge of the press, latch the
    // hit caret as the drag anchor and clear any prior selection. On
    // subsequent held frames, the active end follows the pointer and
    // the anchor flips into `selection` once it diverges. On release
    // (falling edge), drop the anchor so the next press starts fresh.
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
        // Hit-test runs against the *unscrolled* shaped layout, so
        // we add last frame's scroll back into the pointer's local
        // coords. Updated scroll for this frame is computed after
        // `handle_input` returns — the user clicked on what they
        // saw, which is last frame's scroll.
        let [pad_l, pad_t, _, _] = ctx.padding.as_array();
        let local_x = pointer_offset.x - pad_l - ctx.block_offset.x + view.scroll.x;
        let local_y = pointer_offset.y - pad_t - ctx.block_offset.y + view.scroll.y;
        // `byte_at_xy` handles both axes; single-line probes at
        // `y=0` (against an unwrapped layout) collapse to cosmic's
        // 1D `Buffer::hit` walk — one shaped lookup.
        let hit = ui.resources.text.byte_at_xy(
            ed.text,
            local_x,
            if ctx.multiline { local_y } else { 0.0 },
            ctx.params(),
        );
        if resp_state.left.press_count() > 0 {
            // Press rising edge — the input layer counts the
            // multi-press run (`press_count`: 1 = single, 2 = double,
            // 3+ = triple) within the shared double-click window +
            // radius, so single/word/all selection dispatches straight
            // off it.
            ed.state.last_edit_kind = None;
            match resp_state.left.press_count() {
                2 => {
                    // Double-click: select the word under the caret.
                    let r = word_range_at(ed.text, hit);
                    if r.is_empty() {
                        interaction.drag_anchor = Some(hit);
                        ed.state.selection = None;
                        ed.state.caret = hit;
                    } else {
                        interaction.drag_anchor = None;
                        ed.state.selection = Some(r.start);
                        ed.state.caret = r.end;
                    }
                }
                3.. => {
                    // Triple-click and beyond: select everything.
                    interaction.drag_anchor = None;
                    ed.select_all();
                }
                _ => {
                    interaction.drag_anchor = Some(hit);
                    ed.state.selection = None;
                    ed.state.caret = hit;
                }
            }
        } else if interaction.drag_anchor.is_some() {
            // Held drag from a single-click press — caret follows
            // pointer, selection grows from the anchor. Multi-click
            // sequences clear `drag_anchor` so they don't enter this
            // branch and the selection stays locked at the word/all
            // range chosen on the press.
            let anchor = interaction.drag_anchor.unwrap_or(hit);
            ed.state.caret = hit;
            ed.state.selection = if hit == anchor { None } else { Some(anchor) };
        }
    } else if !resp_state.left.held() {
        interaction.drag_anchor = None;
    }

    if !is_focused {
        ed.state.normalize(ed.text);
        interaction.normalize(ed.text);
        return InputResult {
            caret_moved: caret_before != ed.state.caret || sel_before != ed.state.selection,
            was_focused,
            blur,
            submitted,
            edited: ed.edited,
        };
    }

    // Drain the unified keyboard event stream in arrival order:
    // Text chunks splice into the buffer (sanitized for single-line);
    // Down events route through shared edit actions (clipboard / undo)
    // then `apply_key` (edit / nav). Vertical-nav probes happen inline
    // because they need the shaper + layout. Indexing keeps the borrow
    // on the input queue short-lived so we can dispatch to
    // `ui.resources.text` inside the same loop without a scratch Vec.
    let n = ui.input.keyboard_events().len();
    for i in 0..n {
        let event = ui.input.keyboard_events()[i];
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
                if !ed.multiline && kp.key == Key::Enter && !kp.mods.any_command() {
                    submitted = true;
                    continue;
                }
                if dispatch_action(&mut ed, kp, &clipboard) {
                    continue;
                }
                match apply_key(&mut ed, kp) {
                    KeyOutcome::Blur => blur = true,
                    KeyOutcome::Vertical { up, extend } => {
                        resolve_vertical(&mut ed, &ui.resources.text, ctx.params(), up, extend);
                    }
                    KeyOutcome::None => {}
                }
            }
        }
    }

    ed.state.normalize(ed.text);
    interaction.normalize(ed.text);
    InputResult {
        caret_moved: caret_before != ed.state.caret || sel_before != ed.state.selection,
        was_focused,
        blur,
        submitted,
        edited: ed.edited,
    }
}

pub(crate) fn dispatch_action(
    editor: &mut Editor<'_>,
    keypress: KeyPress,
    clipboard: &Clipboard,
) -> bool {
    let Some(action) = EditAction::from_keypress(keypress) else {
        return false;
    };
    action.execute(editor, clipboard);
    true
}

pub(crate) fn apply_key(editor: &mut Editor<'_>, keypress: KeyPress) -> KeyOutcome {
    let extend = keypress.mods.shift;
    match keypress.key {
        Key::Char(c) if !keypress.mods.any_command() => editor.insert_char(c),
        Key::Backspace => editor.delete_backward(),
        Key::Delete => editor.delete_forward(),
        Key::ArrowLeft if is_word_nav(keypress.mods) => editor.move_word_left(extend),
        Key::ArrowRight if is_word_nav(keypress.mods) => editor.move_word_right(extend),
        Key::ArrowLeft => editor.move_grapheme_left(extend),
        Key::ArrowRight => editor.move_grapheme_right(extend),
        Key::ArrowUp if editor.multiline => {
            return KeyOutcome::Vertical { up: true, extend };
        }
        Key::ArrowDown if editor.multiline => {
            return KeyOutcome::Vertical { up: false, extend };
        }
        Key::Enter if editor.multiline => editor.replace_selection("\n", EditKind::Other),
        Key::Home => editor.move_caret(0, extend),
        Key::End => editor.move_caret(editor.text.len(), extend),
        Key::Escape if !editor.collapse_selection() => return KeyOutcome::Blur,
        Key::Escape => {}
        _ => {}
    }
    KeyOutcome::None
}

fn resolve_vertical(
    editor: &mut Editor<'_>,
    shaper: &TextShaper,
    params: ShapeParams,
    up: bool,
    extend: bool,
) {
    let pos = shaper.cursor_xy(editor.text, editor.state.caret, params);
    let target = if up && pos.y_top <= 0.5 {
        0
    } else {
        let probe_y = if up {
            pos.y_top - 1.0
        } else {
            pos.y_top + pos.line_height + 1.0
        };
        shaper.byte_at_xy(editor.text, pos.x, probe_y, params)
    };
    editor.move_caret(target, extend);
}

fn is_word_nav(modifiers: Modifiers) -> bool {
    match PLATFORM {
        Platform::Mac => modifiers.alt && !modifiers.ctrl,
        _ => modifiers.ctrl && !modifiers.alt,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum KeyOutcome {
    None,
    Blur,
    Vertical { up: bool, extend: bool },
}
