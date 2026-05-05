use crate::input::keyboard::{Key, KeyPress};
use crate::layout::types::sense::Sense;
use crate::primitives::rect::Rect;
use crate::shape::{Shape, TextWrap};
use crate::tree::element::{Configure, Element, LayoutMode};
use crate::tree::widget_id::WidgetId;
use crate::ui::Ui;
use crate::widgets::Response;
use crate::widgets::theme::TextEditTheme;
use std::borrow::Cow;

/// Cross-frame state for one [`TextEdit`]. Stored in [`Ui`]'s
/// `WidgetId → Any` map keyed by the widget's id; lifecycle managed by
/// the same removed-widget sweep that drives the layout/text caches.
///
/// `caret` is a *byte* offset into the buffer (cosmic-text returns
/// byte cursors and `&buffer[..caret]` is the natural prefix-measure
/// path). v1 mutates byte boundaries that always coincide with
/// codepoint boundaries (insert at caret, remove one codepoint at a
/// time on backspace/delete) so a malformed offset shouldn't be
/// reachable from inside the widget. Host code that mutates the
/// buffer between frames may shrink it past `caret`; `show()` clamps
/// at the top each frame.
#[derive(Clone, Default, Debug)]
pub(crate) struct TextEditState {
    pub(crate) caret: usize,
    /// Selection anchor. `None` = no selection. Unused in v1 — the
    /// slot exists so adding shift+arrow / drag selection later doesn't
    /// require a state migration.
    #[allow(dead_code)] // first reader is the v1.1 selection branch
    pub(crate) selection: Option<usize>,
}

/// Single-line editable text leaf. v1 supports: typing (via `KeyDown`
/// printable chars or IME `Text` commits), backspace/delete, left/right
/// arrows, home/end, escape-to-blur, click-to-place-caret. Selection,
/// shift+arrow, drag-select, multi-line, copy/paste, undo are deferred.
///
/// Borrows `&'a mut String` for the buffer — host owns the storage,
/// widget mutates in place. State row carries only caret/selection so
/// host-side buffer mutations between frames are visible immediately
/// (the widget clamps `caret <= text.len()` at the top of every show).
pub struct TextEdit<'a> {
    element: Element,
    text: &'a mut String,
    style: Option<TextEditTheme>,
    placeholder: Cow<'static, str>,
    /// Per-widget font-size override; falls back to
    /// `Theme::text_edit.size_px` when `None`.
    size_px: Option<f32>,
}

impl<'a> TextEdit<'a> {
    #[track_caller]
    pub fn new(text: &'a mut String) -> Self {
        let mut element = Element::new_auto(LayoutMode::Leaf);
        element.sense = Sense::CLICK;
        element.focusable = true;
        Self {
            element,
            text,
            style: None,
            placeholder: Cow::Borrowed(""),
            size_px: None,
        }
    }

    pub fn placeholder(mut self, s: impl Into<Cow<'static, str>>) -> Self {
        self.placeholder = s.into();
        self
    }

    pub fn style(mut self, s: TextEditTheme) -> Self {
        self.style = Some(s);
        self
    }

    /// Override the font size used for the buffer this frame. Defaults
    /// to `Theme::text_edit.size_px` (16 px).
    pub fn size_px(mut self, px: f32) -> Self {
        assert!(px > 0.0, "TextEdit size_px must be positive, got {px}");
        self.size_px = Some(px);
        self
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        let id = self.element.id;
        let is_focused = ui.input.focused == Some(id);
        let style = self
            .style
            .clone()
            .unwrap_or_else(|| ui.theme.text_edit.clone());
        let font_size = self.size_px.unwrap_or(style.size_px);

        // Phase 1: input handling. Touches `ui.state` and `ui.input`
        // (separate fields, disjoint borrows). Click-to-place-caret
        // happens here too, before keystroke processing, so a click +
        // type in the same frame inserts at the click point.
        let mut blur_after = false;
        let caret_byte = handle_input(ui, id, is_focused, self.text, font_size, &mut blur_after);

        // Phase 2: open the node and push shapes. `caret_x` for the
        // caret position lives inside the closure since it touches
        // `ui.pipeline.text` (disjoint from `ui.tree`, so add_shape
        // sequences fine after the measurement returns).
        let element = self.element;
        let placeholder = self.placeholder;
        let text_ptr = &*self.text;
        let resp_node = ui.node(element, |ui| {
            // Background.
            let bg = if is_focused {
                style.background_focused
            } else {
                style.background
            };
            let stroke = if is_focused {
                style.stroke_focused
            } else {
                style.stroke
            };
            ui.add_shape(Shape::RoundedRect {
                radius: style.radius,
                fill: bg,
                stroke,
            });

            // Text or placeholder. Empty buffer + unfocused shows the
            // placeholder; focused shows the buffer (even if empty)
            // because we still want the caret to render flush-left.
            let (display, color) = if text_ptr.is_empty() && !is_focused {
                (placeholder.clone(), style.placeholder)
            } else {
                (Cow::Owned(text_ptr.clone()), style.text)
            };
            if !display.is_empty() {
                ui.add_shape(Shape::Text {
                    text: display,
                    color,
                    font_size_px: font_size,
                    wrap: TextWrap::Single,
                    align: Default::default(),
                });
            }

            // Caret. Painted as a thin Overlay rect at owner-local
            // coords so it stays in the widget's clip and renders
            // *over* the text. Only when focused.
            if is_focused {
                let caret_x = ui.pipeline.text.caret_x(text_ptr, caret_byte, font_size);
                let pad = style.padding;
                let caret_rect =
                    Rect::new(pad.left + caret_x, pad.top, style.caret_width, font_size);
                ui.add_shape(Shape::Overlay {
                    rect: caret_rect,
                    radius: Default::default(),
                    fill: style.caret,
                });
            }
        });

        // Phase 3: side effects that need a fresh borrow of `ui`
        // (Escape-to-blur). Done after the node closes so we don't
        // accidentally mutate during recording.
        if blur_after {
            ui.request_focus(None);
        }

        let state = ui.response_for(id);
        Response {
            node: resp_node,
            state,
        }
    }
}

impl Configure for TextEdit<'_> {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}

/// Process this frame's pointer + keyboard input for one TextEdit
/// widget and return the caret byte offset to render. Splitting this
/// out of `show()` keeps the borrow choreography contained: we touch
/// `ui.state`, `ui.input`, and `ui.pipeline.text` here, but never the
/// shape/tree storage.
fn handle_input(
    ui: &mut Ui,
    id: WidgetId,
    is_focused: bool,
    text: &mut String,
    font_size: f32,
    blur_after: &mut bool,
) -> usize {
    let resp_state = ui.response_for(id);
    // Clamp caret. Read state via a mutable borrow that's compatible
    // with reading from `ui.input` (separate field).
    let state = ui
        .state
        .get_or_insert_with::<TextEditState, _>(id, Default::default);
    if state.caret > text.len() {
        state.caret = text.len();
    }

    // Click-to-place-caret. While the widget is being pressed, the
    // caret tracks the pointer x → drag-to-place falls out for free.
    // Uses last-frame's rect (one-frame stale, same model as the wheel
    // pan clamp); first frame after layout the widget may set caret
    // against a stale rect, the next settles.
    if resp_state.pressed
        && let (Some(rect), Some(ptr)) = (resp_state.rect, ui.input.pointer_pos())
    {
        let pad = ui.theme.text_edit.padding; // approximate; per-widget style override not available here without re-clone
        let local_x = ptr.x - rect.min.x - pad.left;
        let new_caret = caret_from_x(text, local_x, font_size, &mut ui.pipeline.text);
        let state = ui
            .state
            .get_or_insert_with::<TextEditState, _>(id, Default::default);
        state.caret = new_caret;
    }

    if !is_focused {
        let state = ui
            .state
            .get_or_insert_with::<TextEditState, _>(id, Default::default);
        return state.caret;
    }

    // Drain per-frame keyboard queues. `frame_text` first (so a
    // ModifiersChanged + keystrokes-for-shortcut sequence still leaves
    // typed text intact), then `frame_keys` for navigation/edits.
    if !ui.input.frame_text.is_empty() {
        let state = ui
            .state
            .get_or_insert_with::<TextEditState, _>(id, Default::default);
        text.insert_str(state.caret, &ui.input.frame_text);
        state.caret += ui.input.frame_text.len();
    }

    // Iterate frame_keys with a swap-like manual loop to avoid borrow
    // conflict between `ui.input.frame_keys` (read) and `ui.state`
    // (write). Disjoint fields → both borrows allowed but not via the
    // `state_mut` helper (which takes `&mut self` on the whole Ui).
    let n_keys = ui.input.frame_keys.len();
    for i in 0..n_keys {
        let kp = ui.input.frame_keys[i];
        let state = ui
            .state
            .get_or_insert_with::<TextEditState, _>(id, Default::default);
        match apply_key(text, &mut state.caret, kp) {
            KeyOutcome::Consumed => {}
            KeyOutcome::Blur => {
                *blur_after = true;
            }
            KeyOutcome::Pass => {}
        }
    }

    let state = ui
        .state
        .get_or_insert_with::<TextEditState, _>(id, Default::default);
    state.caret
}

/// Result of dispatching one key press through the editor: did the
/// widget consume the key, request blur, or pass it through?
enum KeyOutcome {
    Consumed,
    Blur,
    /// Reserved for future key dispatch (Tab focus cycle, etc.).
    Pass,
}

fn apply_key(text: &mut String, caret: &mut usize, kp: KeyPress) -> KeyOutcome {
    match kp.key {
        // Printable character without a command modifier counts as text
        // input. Shift alone doesn't disqualify (shift+'a' = 'A' is the
        // capitalized letter winit already gave us).
        Key::Char(c) if !kp.mods.any_command() => {
            let mut buf = [0u8; 4];
            let s = c.encode_utf8(&mut buf);
            text.insert_str(*caret, s);
            *caret += s.len();
            KeyOutcome::Consumed
        }
        Key::Space if !kp.mods.any_command() => {
            text.insert(*caret, ' ');
            *caret += 1;
            KeyOutcome::Consumed
        }
        Key::Backspace => {
            if *caret > 0 {
                let prev = prev_char_boundary(text, *caret);
                text.replace_range(prev..*caret, "");
                *caret = prev;
            }
            KeyOutcome::Consumed
        }
        Key::Delete => {
            if *caret < text.len() {
                let next = next_char_boundary(text, *caret);
                text.replace_range(*caret..next, "");
            }
            KeyOutcome::Consumed
        }
        Key::ArrowLeft => {
            *caret = prev_char_boundary(text, *caret);
            KeyOutcome::Consumed
        }
        Key::ArrowRight => {
            *caret = next_char_boundary(text, *caret);
            KeyOutcome::Consumed
        }
        Key::Home => {
            *caret = 0;
            KeyOutcome::Consumed
        }
        Key::End => {
            *caret = text.len();
            KeyOutcome::Consumed
        }
        Key::Escape => KeyOutcome::Blur,
        // Single-line v1 ignores Enter / Tab / PageUp / PageDown and
        // any control-modified shortcut (ctrl+a, cmd+c, etc.).
        _ => KeyOutcome::Pass,
    }
}

fn prev_char_boundary(text: &str, offset: usize) -> usize {
    if offset == 0 {
        return 0;
    }
    let mut i = offset - 1;
    while i > 0 && !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn next_char_boundary(text: &str, offset: usize) -> usize {
    if offset >= text.len() {
        return text.len();
    }
    let mut i = offset + 1;
    while i < text.len() && !text.is_char_boundary(i) {
        i += 1;
    }
    i
}

/// Linear scan: pick the byte offset whose prefix-x is closest to
/// `target_x`. O(n) measure calls per click — acceptable for v1 since
/// click events are rare and short strings cheap. A future
/// `MeasureResult::byte_to_x` API (exposed by cosmic-text via
/// `Buffer::layout_runs`) would collapse this to one shaped lookup.
fn caret_from_x(
    text: &str,
    target_x: f32,
    font_size: f32,
    m: &mut crate::text::TextMeasurer,
) -> usize {
    let mut best_off = 0usize;
    let mut best_dist = target_x.abs();
    for (i, ch) in text.char_indices() {
        let next = i + ch.len_utf8();
        let x = m.caret_x(text, next, font_size);
        let d = (x - target_x).abs();
        if d < best_dist {
            best_dist = d;
            best_off = next;
        }
    }
    best_off
}

#[cfg(test)]
mod tests;
