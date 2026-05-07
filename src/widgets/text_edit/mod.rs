use crate::input::keyboard::{Key, KeyPress};
use crate::layout::types::sense::Sense;
use crate::primitives::background::Background;
use crate::primitives::rect::Rect;
use crate::primitives::spacing::Spacing;
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
}

impl<'a> TextEdit<'a> {
    #[track_caller]
    pub fn new(text: &'a mut String) -> Self {
        let mut element = Element::new_auto(LayoutMode::Leaf);
        element.sense = Sense::CLICK;
        element.focusable = true;
        // `Element::padding` left at zero — `show()` substitutes
        // `theme.text_edit.padding` when the user didn't call
        // `.padding(...)`. Same renderer semantics as before; the
        // value just lives on the theme instead of hard-coded here.
        Self {
            element,
            text,
            style: None,
            placeholder: Cow::Borrowed(""),
        }
    }

    pub fn placeholder(mut self, s: impl Into<Cow<'static, str>>) -> Self {
        self.placeholder = s.into();
        self
    }

    /// Override the whole TextEdit theme — all-or-nothing. To tweak
    /// one axis, build the bundle from the theme:
    /// `TextEditTheme { caret: red, ..ui.theme.text_edit }`. Buffer
    /// font/leading/color live on the per-state `text` slot (a
    /// [`crate::TextStyle`]) — `None` inherits [`crate::Theme::text`]
    /// like every other text-rendering widget.
    pub fn style(mut self, s: TextEditTheme) -> Self {
        self.style = Some(s);
        self
    }

    pub fn show(mut self, ui: &mut Ui) -> Response {
        let id = self.element.id;
        let is_focused = ui.input.focused == Some(id);
        let theme = self.style.unwrap_or_else(|| ui.theme.text_edit.clone());
        // Apply theme padding/margin when the builder hasn't set
        // anything (sentinel: `Spacing::ZERO` == "use theme"). The
        // renderer reads `element.padding` to deflate the buffer
        // layout, and the caret hit-test reads it back below — both
        // see the resolved value.
        if self.element.padding == Spacing::ZERO {
            self.element.padding = theme.padding;
        }
        if self.element.margin == Spacing::ZERO {
            self.element.margin = theme.margin;
        }
        // Pick the per-state style. Disabled wins over focus — a
        // disabled editor that still happens to hold focus paints with
        // its disabled visuals (mirrors Button).
        let state = if self.element.disabled {
            theme.disabled.clone()
        } else if is_focused {
            theme.focused.clone()
        } else {
            theme.normal.clone()
        };
        // `None` text inherits the global `Theme::text` (same rule as
        // Button's per-state `text`). Apps changing `theme.text.color`
        // recolor every editor that didn't override.
        let text_style = state.text.unwrap_or_else(|| ui.theme.text.clone());
        let font_size = text_style.font_size_px;
        let line_height_mult = text_style.line_height_mult;
        // The renderer deflates by `element.padding` when laying out
        // `Shape::Text` (see `encoder::mod.rs`). Reading the same value
        // here keeps the caret rect aligned with the glyphs.
        let padding = self.element.padding;

        // Phase 1: input handling. Touches `ui.state` and `ui.input`
        // (separate fields, disjoint borrows). Click-to-place-caret
        // happens here too, before keystroke processing, so a click +
        // type in the same frame inserts at the click point.
        let mut blur_after = false;
        let caret_byte = handle_input(
            ui,
            id,
            is_focused,
            self.text,
            font_size,
            font_size * line_height_mult,
            padding.left,
            &mut blur_after,
        );

        // Phase 2: open the node and push shapes. `caret_x` for the
        // caret position lives inside the closure since it touches
        // `ui.text` (disjoint from `ui.tree`, so add_shape
        // sequences fine after the measurement returns).
        // Chrome paints via `Tree::chrome_for` — encoder emits it before
        // any clip. The surface's clip stays `None` (TextEdit's caret
        // and selection handle their own painting; no rect-clipping).
        let surface = state
            .background
            .or(Some(Background::default()))
            .map(crate::widgets::theme::Surface::from);
        let placeholder = self.placeholder;
        let text_ptr = &*self.text;
        let resp_node = ui.node(self.element, surface, |ui| {
            // Text or placeholder. Empty buffer + unfocused shows the
            // placeholder; focused shows the buffer (even if empty)
            // because we still want the caret to render flush-left.
            let (display, color) = if text_ptr.is_empty() && !is_focused {
                (placeholder.clone(), theme.placeholder)
            } else {
                (Cow::Owned(text_ptr.clone()), text_style.color)
            };
            if !display.is_empty() {
                ui.add_shape(Shape::Text {
                    text: display,
                    color,
                    font_size_px: font_size,
                    line_height_px: font_size * line_height_mult,
                    wrap: TextWrap::Single,
                    align: Default::default(),
                });
            }

            // Caret. Painted as a thin Overlay rect at owner-local
            // coords so it stays in the widget's clip and renders
            // *over* the text. Only when focused.
            if is_focused {
                let caret_x = ui.text.caret_x(
                    text_ptr,
                    caret_byte,
                    font_size,
                    font_size * line_height_mult,
                );
                let pad = padding;
                // Caret height = `font_size × line_height_mult`
                // (default 1.2 from `TextEditTheme`, matching the
                // shaper's leading) so the rect spans the same y-range
                // the shaped text occupies. Using `font_size` alone
                // leaves it ~20 % short and visually offset upward
                // against the glyph baseline.
                let caret_rect = Rect::new(
                    pad.left + caret_x,
                    pad.top,
                    theme.caret_width,
                    font_size * line_height_mult,
                );
                ui.add_shape(Shape::RoundedRect {
                    local_rect: Some(caret_rect),
                    radius: Default::default(),
                    fill: theme.caret,
                    stroke: None,
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
/// `ui.state`, `ui.input`, and `ui.text` here, but never the
/// shape/tree storage.
#[allow(clippy::too_many_arguments)]
fn handle_input(
    ui: &mut Ui,
    id: WidgetId,
    is_focused: bool,
    text: &mut String,
    font_size: f32,
    line_height_px: f32,
    // Resolved left-padding from the per-widget style (after merging
    // `.style()` override with the theme default). Subtracted from the
    // press-x so the caret hit-test runs in text-local coords. Passed
    // in rather than re-resolved here so the override branch in
    // `show()` doesn't desync from the click target.
    pad_left: f32,
    blur_after: &mut bool,
) -> usize {
    let resp_state = ui.response_for(id);

    // Hold the state row once for the whole function. `ui.state`,
    // `ui.input`, and `ui.text` are disjoint fields of `Ui`,
    // so we can keep `&mut state` alive while also reading the input
    // queues and dispatching to the text measurer.
    let state = ui
        .state
        .get_or_insert_with::<TextEditState, _>(id, Default::default);
    // Clamp caret. Host code may have shrunk `*text` between frames.
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
        let local_x = ptr.x - rect.min.x - pad_left;
        state.caret = caret_from_x(text, local_x, font_size, line_height_px, &mut ui.text);
    }

    if !is_focused {
        return state.caret;
    }

    // Drain per-frame keyboard queues. `frame_text` first (so a
    // ModifiersChanged + keystrokes-for-shortcut sequence still leaves
    // typed text intact), then `frame_keys` for navigation/edits.
    if !ui.input.frame_text.is_empty() {
        text.insert_str(state.caret, &ui.input.frame_text);
        state.caret += ui.input.frame_text.len();
    }

    for kp in &ui.input.frame_keys {
        if apply_key(text, &mut state.caret, *kp) {
            *blur_after = true;
        }
    }

    state.caret
}

/// Apply one keypress to the buffer + caret. Returns `true` if the
/// caller should blur (Escape); every other recognized key is consumed
/// silently. Single-line v1 ignores Enter / Tab / PageUp / PageDown
/// and anything held with a command modifier (ctrl+a, cmd+c, …).
fn apply_key(text: &mut String, caret: &mut usize, kp: KeyPress) -> bool {
    match kp.key {
        // Printable character without a command modifier counts as text
        // input. Shift alone doesn't disqualify (shift+'a' = 'A' is the
        // capitalized letter winit already gave us).
        Key::Char(c) if !kp.mods.any_command() => {
            let mut buf = [0u8; 4];
            let s = c.encode_utf8(&mut buf);
            text.insert_str(*caret, s);
            *caret += s.len();
        }
        Key::Backspace if *caret > 0 => {
            let prev = prev_char_boundary(text, *caret);
            text.replace_range(prev..*caret, "");
            *caret = prev;
        }
        Key::Delete if *caret < text.len() => {
            let next = next_char_boundary(text, *caret);
            text.replace_range(*caret..next, "");
        }
        Key::ArrowLeft => *caret = prev_char_boundary(text, *caret),
        Key::ArrowRight => *caret = next_char_boundary(text, *caret),
        Key::Home => *caret = 0,
        Key::End => *caret = text.len(),
        Key::Escape => return true,
        _ => {}
    }
    false
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
    line_height_px: f32,
    m: &mut crate::text::TextMeasurer,
) -> usize {
    let mut best_off = 0usize;
    let mut best_dist = target_x.abs();
    for (i, ch) in text.char_indices() {
        let next = i + ch.len_utf8();
        let x = m.caret_x(text, next, font_size, line_height_px);
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
