//! One frame's pointer and keyboard dispatch for a TextEdit.

use crate::common::platform::PLATFORM;
use crate::input::interaction::response_state::ResponseState;
use crate::input::key_class::KeyFilter;
use crate::input::keyboard::key::Key;
use crate::input::keyboard::key_press::KeyPress;
use crate::text::probe::{Caret, TextProbe};
use crate::ui::Ui;
use crate::widgets::text_edit::action::EditAction;
use crate::widgets::text_edit::edit_end::EditEnd;
use crate::widgets::text_edit::edit_state::EditState;
use crate::widgets::text_edit::editor::{CaretMove, EditRules, Editor};
use crate::widgets::text_edit::history::EditKind;
use crate::widgets::text_edit::shape_ctx::ShapeCtx;
use crate::widgets::text_edit::text_layout::TextLayout;
use crate::widgets::text_edit::text_nav::TextNav;
use crate::widgets::text_edit::view_state::ViewState;

/// Result of one frame's input pass: edge signals `show()` folds into
/// [`crate::widgets::text_edit::TextEditResponse`].
#[derive(Debug)]
pub(super) struct InputResult {
    /// How the frame ended the edit, if it did; a cancel blurs before view recording.
    pub(super) ended: Option<EditEnd>,
    /// The buffer was mutated this frame. Set by the mutation choke points, so
    /// a same-length overwrite counts.
    pub(super) edited: bool,
}

/// What the builder configured about accepting input, as opposed to
/// rendering it. Unrelated to [`crate::InputPolicy`], which gates re-recording.
#[derive(Clone, Copy, Debug)]
pub(super) struct AcceptPolicy {
    pub(super) rules: EditRules,
    /// Select everything when focus lands without a same-frame press.
    pub(super) select_all_on_focus: bool,
    /// The key classes this field consumes: the value its scope declares, so
    /// the two cannot drift.
    pub(super) filter: KeyFilter,
}

/// Everything one frame's input pass reads or writes: it edits the buffer and its
/// semantic state, and reads the view only for last frame's scroll and focus.
///
/// The edit state is a plain `&mut` because `Editor` holds it across the
/// keyboard drain, which hands `&mut Ui` to each handler.
#[derive(Debug)]
pub(super) struct InputPass<'a> {
    pub(super) resp_state: &'a ResponseState,
    pub(super) is_focused: bool,
    pub(super) text: &'a mut String,
    pub(super) layout: &'a TextLayout,
    pub(super) policy: AcceptPolicy,
    pub(super) edit: &'a mut EditState,
    pub(super) view: &'a ViewState,
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
            edit,
            view,
        } = self;
        let ctx = &layout.ctx;
        let AcceptPolicy {
            rules,
            select_all_on_focus,
            filter,
        } = policy;
        let mut ended = None;
        let clipboard = ui.clipboard();

        let was_focused = view.was_focused();
        // Repair persisted byte offsets: application code may have replaced
        // `*text` with a string whose UTF-8 boundaries differ.
        edit.normalize(text);
        let mut ed = Editor::new(text, edit, rules);
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
            let local_x = pointer_offset.x - pad_l - block.x + view.scroll.offset().x;
            let local_y = pointer_offset.y - pad_t - block.y + view.scroll.offset().y;
            // Single-line probes at `y=0`.
            let hit = ui
                .probe_text(ctx.run(ed.text()))
                .byte_at(local_x, if ctx.multiline { local_y } else { 0.0 });
            let clicks = resp_state.left.press_count();
            if clicks > 0 {
                ed.press(hit, clicks, CaretMove::from(ui.peek_modifiers()));
            } else {
                ed.drag_to(hit);
            }
        } else if !resp_state.left.held() {
            ed.end_drag();
        }

        if !is_focused {
            ed.normalize();
            return InputResult {
                ended,
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
                ended = Some(EditEnd::Submitted);
                assert_only_repeats_after(ui.keyboard_events(), i);
                break;
            }
            if let Some(action) = EditAction::from_keypress(kp) {
                action.execute(&mut ed, &clipboard);
                continue;
            }
            match apply_key(&mut ed, kp, TextNav::new(PLATFORM)) {
                KeyOutcome::Blur => {
                    ended = Some(EditEnd::Canceled);
                    assert_only_repeats_after(ui.keyboard_events(), i);
                    break;
                }
                KeyOutcome::Probe(probe) => probe.apply(&mut ed, ui, ctx),
                KeyOutcome::None => {}
            }
        }

        ed.normalize();
        InputResult {
            ended,
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

pub(super) fn apply_key(editor: &mut Editor<'_>, keypress: KeyPress, nav: TextNav) -> KeyOutcome {
    let motion = CaretMove::from(keypress.mods);
    let probe = |target| KeyOutcome::Probe(ProbeMotion { target, motion });
    let line_nav = nav.is_line(keypress.mods);
    match keypress.key {
        Key::Backspace if line_nav => editor.delete_to_line_start(),
        Key::Backspace if nav.is_word(keypress.mods) => editor.delete_word_backward(),
        Key::Backspace => editor.delete_backward(),
        Key::Delete if nav.is_word(keypress.mods) => editor.delete_word_forward(),
        Key::Delete => editor.delete_forward(),
        Key::ArrowLeft if nav.is_word(keypress.mods) => editor.move_word_left(motion),
        Key::ArrowRight if nav.is_word(keypress.mods) => editor.move_word_right(motion),
        Key::ArrowLeft if line_nav && editor.multiline() => return probe(ProbeTarget::RowStart),
        Key::ArrowRight if line_nav && editor.multiline() => return probe(ProbeTarget::RowEnd),
        Key::ArrowLeft if line_nav => editor.move_caret(0, motion),
        Key::ArrowRight if line_nav => editor.move_caret(editor.text().len(), motion),
        // Document ends: Ctrl+Home/End, and Cmd+Up/Down on macOS.
        Key::Home | Key::ArrowUp if nav.is_document(keypress) => editor.move_caret(0, motion),
        Key::End | Key::ArrowDown if nav.is_document(keypress) => {
            editor.move_caret(editor.text().len(), motion);
        }
        Key::ArrowLeft => editor.move_grapheme_left(motion),
        Key::ArrowRight => editor.move_grapheme_right(motion),
        Key::ArrowUp if editor.multiline() => return probe(ProbeTarget::Above),
        Key::ArrowDown if editor.multiline() => return probe(ProbeTarget::Below),
        Key::Enter if editor.multiline() => editor.replace_selection("\n", EditKind::Other),
        // Multi-line Home / End mean the visual line.
        Key::Home if editor.multiline() => return probe(ProbeTarget::RowStart),
        Key::End if editor.multiline() => return probe(ProbeTarget::RowEnd),
        Key::Home => editor.move_caret(0, motion),
        Key::End => editor.move_caret(editor.text().len(), motion),
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum KeyOutcome {
    None,
    Blur,
    /// A caret move only the shaped layout can answer, run once the press loop
    /// can hand it `ui`.
    Probe(ProbeMotion),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ProbeMotion {
    pub(super) target: ProbeTarget,
    pub(super) motion: CaretMove,
}

/// Where on the shaped layout a multi-line caret move lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ProbeTarget {
    /// The visual line above, at the caret's x; the first line goes to the start.
    Above,
    /// The visual line below, at the caret's x.
    Below,
    /// Home in a multi-line editor: the visual line's leading edge.
    RowStart,
    /// End in a multi-line editor: the visual line's trailing edge.
    RowEnd,
}

impl ProbeMotion {
    /// Move the caret to the byte [`ProbeTarget::byte`] picks from its current
    /// position. Both queries share one `probe_text` borrow and cache dispatch.
    fn apply(self, editor: &mut Editor<'_>, ui: &mut Ui, ctx: &ShapeCtx) {
        let byte = {
            let probe = ui.probe_text(ctx.run(editor.text()));
            let pos = probe.caret_at(editor.caret());
            self.target.byte(&probe, pos)
        };
        editor.move_caret(byte, self.motion);
    }
}

impl ProbeTarget {
    fn byte(self, probe: &TextProbe<'_>, pos: Caret) -> usize {
        match self {
            Self::Above if pos.y_top <= 0.5 => 0,
            Self::Above => probe.byte_at(pos.x, pos.y_top - 1.0),
            Self::Below => probe.byte_at(pos.x, pos.y_top + pos.line_height + 1.0),
            // A soft-wrapped line has no `\n` to scan for, so hit-test past each end
            // of the caret's row, which `byte_at` clamps. Mid-row, so the hit cannot
            // fall to a neighboring line.
            Self::RowStart => probe.byte_at(-1.0, pos.y_top + pos.line_height * 0.5),
            Self::RowEnd => probe.byte_at(probe.size().w + 1.0, pos.y_top + pos.line_height * 0.5),
        }
    }
}
