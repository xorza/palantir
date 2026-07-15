//! Per-frame pointer and keyboard dispatch for TextEdit.

use crate::input::keyboard::{Key, KeyboardEvent};
use crate::primitives::widget_id::WidgetId;
use crate::ui::Ui;
use crate::widgets::context_menu::ContextMenu;
use crate::widgets::text_edit::ShapeCtx;
use crate::widgets::text_edit::model::{
    EditKind, Editor, KeyOutcome, TextEditState, word_range_at,
};

/// Result of one frame's input pass over a TextEdit: the caret byte,
/// the (sorted) selection range for the painter, and the edge signals
/// `show()` folds into [`crate::widgets::text_edit::TextEditResponse`].
#[derive(Debug)]
pub(crate) struct InputResult {
    pub(crate) caret: usize,
    pub(crate) selection: Option<std::ops::Range<usize>>,
    /// Caret or selection differ from their pre-input values (compared
    /// against the pre-clamp snapshot, so an external buffer shrink
    /// that displaces the caret also reads as motion) — drives the
    /// blink-phase reset.
    pub(crate) caret_moved: bool,
    /// The state row's `prev_focused` before this pass — `show()`
    /// derives the gained/lost focus edges from it; Phase 2 rewrites
    /// the row afterwards.
    pub(crate) was_focused: bool,
    /// Escape asked to blur — applied by `show()` after the node closes.
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
/// `ui.ctx.shaper` here, but never the shape/tree storage.
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
    // Snapshot once before the long `&mut state` borrow below. The
    // menu and the text-edit state live under the same WidgetId but
    // different TypeIds; the borrow checker can't see the disjoint
    // rows so we read the menu row first.
    let menu_open = ContextMenu::is_open(ui, id);

    // Hold the state row once for the whole function (inside the
    // `Editor`). `ui.state`, `ui.input`, and `ui.ctx.shaper` are
    // disjoint fields of `Ui`, so the `&mut state` can stay alive
    // while also reading the input queues and dispatching to the text
    // measurer.
    let state = ui
        .state
        .get_or_insert_with::<TextEditState, _>(id, Default::default);
    // Pre-input snapshot for the `caret_moved` / focus edges — taken
    // before the clamp so an external buffer shrink that displaces the
    // caret still reads as caret motion (blink reset).
    let caret_before = state.caret;
    let sel_before = state.selection;
    let was_focused = state.prev_focused;
    // Repair persisted byte offsets before any range/slice operation.
    // Application code may have replaced `*text` with a same-length or
    // longer string whose UTF-8 boundaries differ from the prior frame.
    state.normalize(text);
    let mut ed = Editor::new(text, state, ctx.multiline, max_chars);

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
    // has left the surface (`pointer_pos == None`) the inner `let` fails
    // and we fall through *without* clearing the anchor — the gesture is
    // still live, just position-less this frame.
    if resp_state.left.held()
        && let (Some(rect), Some(ptr)) = (resp_state.rect, ui.input.pointer_pos)
    {
        // `ptr` and `rect` are in surface (post-transform) space, but glyphs
        // are laid out — and `byte_at_xy` hit-tests — in logical px. Under a
        // scaled ancestor (canvas zoom) the widget's on-screen size differs
        // from its layout size, so divide out the transform's scale to bring
        // the click's offset into logical space before subtracting the logical
        // padding / align / scroll — else the caret lands on the wrong glyph
        // whenever zoom ≠ 1.
        let scale = resp_state.transform.scale;
        // Hit-test runs against the *unscrolled* shaped layout, so
        // we add last frame's scroll back into the pointer's local
        // coords. Updated scroll for this frame is computed after
        // `handle_input` returns — the user clicked on what they
        // saw, which is last frame's scroll.
        let [pad_l, pad_t, _, _] = ctx.padding.as_array();
        let local_x = (ptr.x - rect.min.x) / scale - pad_l - ctx.block_offset.x + ed.state.scroll.x;
        let local_y = (ptr.y - rect.min.y) / scale - pad_t - ctx.block_offset.y + ed.state.scroll.y;
        // `byte_at_xy` handles both axes; single-line probes at
        // `y=0` (against an unwrapped layout) collapse to cosmic's
        // 1D `Buffer::hit` walk — one shaped lookup.
        let hit = ui.ctx.shaper.byte_at_xy(
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
                        ed.state.drag_anchor = Some(hit);
                        ed.state.selection = None;
                        ed.state.caret = hit;
                    } else {
                        ed.state.drag_anchor = None;
                        ed.state.selection = Some(r.start);
                        ed.state.caret = r.end;
                    }
                }
                3.. => {
                    // Triple-click and beyond: select everything.
                    ed.state.drag_anchor = None;
                    ed.select_all();
                }
                _ => {
                    ed.state.drag_anchor = Some(hit);
                    ed.state.selection = None;
                    ed.state.caret = hit;
                }
            }
        } else if ed.state.drag_anchor.is_some() {
            // Held drag from a single-click press — caret follows
            // pointer, selection grows from the anchor. Multi-click
            // sequences clear `drag_anchor` so they don't enter this
            // branch and the selection stays locked at the word/all
            // range chosen on the press.
            let anchor = ed.state.drag_anchor.unwrap_or(hit);
            ed.state.caret = hit;
            ed.state.selection = if hit == anchor { None } else { Some(anchor) };
        }
    } else if !resp_state.left.held() {
        ed.state.drag_anchor = None;
    }

    if !is_focused {
        ed.state.normalize(ed.text);
        return InputResult {
            caret: ed.state.caret,
            selection: ed.state.sel_range(),
            caret_moved: caret_before != ed.state.caret || sel_before != ed.state.selection,
            was_focused,
            blur,
            submitted,
            edited: ed.edited,
        };
    }

    // Drain the unified keyboard event stream in arrival order:
    // Text chunks splice into the buffer (sanitized for single-line);
    // Down events route through `dispatch_shortcut` (clipboard / undo)
    // then `apply_key` (edit / nav). Vertical-nav probes happen inline
    // because they need the shaper + layout. Indexing keeps the borrow
    // on `frame_keyboard_events` short-lived so we can dispatch to
    // `ui.ctx.shaper` inside the same loop without a scratch Vec.
    let n = ui.input.frame_keyboard_events.len();
    for i in 0..n {
        match ui.input.frame_keyboard_events[i] {
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
                if ed.dispatch_shortcut(kp, menu_open) {
                    continue;
                }
                match ed.apply_key(kp) {
                    KeyOutcome::Blur => blur = true,
                    KeyOutcome::Vertical { up, extend } => {
                        ed.resolve_vertical(&ui.ctx.shaper, ctx.params(), up, extend);
                    }
                    KeyOutcome::None => {}
                }
            }
        }
    }

    ed.state.normalize(ed.text);
    InputResult {
        caret: ed.state.caret,
        selection: ed.state.sel_range(),
        caret_moved: caret_before != ed.state.caret || sel_before != ed.state.selection,
        was_focused,
        blur,
        submitted,
        edited: ed.edited,
    }
}
