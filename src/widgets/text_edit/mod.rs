use crate::forest::element::{Configure, Element, LayoutMode};
use crate::forest::widget_id::WidgetId;
use crate::input::keyboard::{Key, KeyPress};
use crate::input::sense::Sense;
use crate::primitives::rect::Rect;
use crate::primitives::spacing::Spacing;
use crate::primitives::stroke::Stroke;
use crate::shape::{Shape, TextWrap};
use crate::ui::Ui;
use crate::widgets::Response;
use crate::widgets::context_menu::{ContextMenu, MenuItem};
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
#[derive(Clone, Copy, Default, Debug)]
pub(crate) struct TextEditState {
    pub(crate) caret: usize,
    /// Selection anchor. `None` = no selection. Invariant: never
    /// `Some(caret)` — every mutation site collapses an empty selection
    /// to `None` so "selection live" is a single `is_some()` check.
    pub(crate) selection: Option<usize>,
    /// Caret byte at the rising edge of the pointer press, used as the
    /// drag anchor for click+drag selection. Reset on release.
    pub(crate) drag_anchor: Option<usize>,
    /// Was the widget pressed last frame? Used to detect the press
    /// rising edge for anchor latching.
    pub(crate) prev_pressed: bool,
}

impl TextEditState {
    fn sel_range(&self) -> Option<std::ops::Range<usize>> {
        let a = self.selection?;
        Some(a.min(self.caret)..a.max(self.caret))
    }
}

/// Move the caret to `new_caret`, extending the selection if `extend`
/// is set (latches anchor on the first extending move) or collapsing it
/// otherwise. Maintains the "never Some(caret)" invariant.
fn move_caret(state: &mut TextEditState, new_caret: usize, extend: bool) {
    if extend {
        state.selection.get_or_insert(state.caret);
    } else {
        state.selection = None;
    }
    state.caret = new_caret;
    if state.selection == Some(state.caret) {
        state.selection = None;
    }
}

/// Delete the live selection range (if any), update caret to the range
/// start, and return the deleted range — callers use it to know whether
/// to skip a subsequent codepoint-delete (Backspace/Delete) or not.
fn delete_selection(text: &mut String, state: &mut TextEditState) -> bool {
    let Some(range) = state.sel_range() else {
        return false;
    };
    let start = range.start;
    text.replace_range(range, "");
    state.caret = start;
    state.selection = None;
    true
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
    pub fn new(text: &'a mut String) -> Self {
        let mut element = Element::new(LayoutMode::Leaf);
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
        // Pick the per-state look + animate its visual components.
        // Disabled wins over focus — a disabled editor that still
        // happens to hold focus paints with its disabled visuals
        // (mirrors Button). State.disabled comes from the cascade
        // (one-frame stale); OR self-disabled in for lag-free
        // response to a freshly toggled `.disabled(true)`.
        let mut response = ui.response_for(id);
        response.disabled |= self.element.disabled;
        let fallback_text = ui.theme.text;
        let look = theme
            .pick(response)
            .animate(ui, id, fallback_text, theme.anim);
        let font_size = look.text.font_size_px;
        let line_height_mult = look.text.line_height_mult;
        // The renderer deflates by `element.padding` when laying out
        // `ShapeRecord::Text` (see `encoder::mod.rs`). Reading the same value
        // here keeps the caret rect aligned with the glyphs.
        let padding = self.element.padding;

        // Phase 1: input handling. Touches `ui.state` and `ui.input`
        // (separate fields, disjoint borrows). Click-to-place-caret
        // happens here too, before keystroke processing, so a click +
        // type in the same frame inserts at the click point.
        let mut blur_after = false;
        let input = handle_input(
            ui,
            id,
            is_focused,
            self.text,
            font_size,
            font_size * line_height_mult,
            padding.left,
            &mut blur_after,
        );
        let caret_byte = input.caret;
        let selection = input.selection;

        // Phase 2: open the node and push shapes. `caret_x` for the
        // caret position lives inside the closure since it touches
        // `ui.text` (disjoint from `ui.tree`, so add_shape
        // sequences fine after the measurement returns).
        // Chrome paints via `Tree::chrome_for` — encoder emits it before
        // any clip. No clip is set: TextEdit's caret and selection
        // handle their own painting and don't need rect-clipping.
        let mut element = self.element;
        element.chrome = Some(look.background);
        let placeholder = self.placeholder;
        let text_ptr = &*self.text;
        let resp_node = ui.node(element, |ui| {
            // Selection highlight, painted *before* the text so glyphs
            // sit on top of the wash. Only when focused and a range is
            // actually live (anchor != caret — collapsed selections are
            // stored as `None`, so any `Some` here has positive width).
            if is_focused && let Some(range) = selection {
                let x0 = ui.text.caret_x(
                    text_ptr,
                    range.start,
                    font_size,
                    font_size * line_height_mult,
                );
                let x1 =
                    ui.text
                        .caret_x(text_ptr, range.end, font_size, font_size * line_height_mult);
                let sel_rect = Rect::new(
                    padding.left + x0,
                    padding.top,
                    x1 - x0,
                    font_size * line_height_mult,
                );
                ui.add_shape(Shape::RoundedRect {
                    local_rect: Some(sel_rect),
                    radius: Default::default(),
                    fill: theme.selection.into(),
                    stroke: Stroke::ZERO,
                });
            }

            // Text or placeholder. Empty buffer + unfocused shows the
            // placeholder; focused shows the buffer (even if empty)
            // because we still want the caret to render flush-left.
            let (display, color) = if text_ptr.is_empty() && !is_focused {
                (placeholder.clone(), theme.placeholder)
            } else {
                (Cow::Owned(text_ptr.clone()), look.text.color)
            };
            if !display.is_empty() {
                ui.add_shape(Shape::Text {
                    local_rect: None,
                    text: display,
                    brush: color.into(),
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
                    fill: theme.caret.into(),
                    stroke: Stroke::ZERO,
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
        let response = Response {
            node: resp_node,
            id,
            state,
        };

        // Phase 4: default Cut / Copy / Paste / Clear context menu.
        // Triggered by secondary click on the editor; items mutate
        // the host's buffer through the same `&mut String` borrow
        // `show` was given, then sync `TextEditState.caret` /
        // `selection` so the next frame paints the right place.
        // Selection range + clipboard liveness snapshotted before
        // the closure so the items can render with the right
        // `.enabled(...)` per state.
        let sel = ui.state_mut::<TextEditState>(id).sel_range();
        let has_sel = sel.is_some();
        let cb_has = !crate::clipboard::is_empty();
        let has_text = !self.text.is_empty();
        let text = self.text;
        ContextMenu::attach(ui, &response).show(ui, |ui| {
            if MenuItem::new("Cut")
                .shortcut("⌘X")
                .enabled(has_sel)
                .show(ui)
                .clicked()
                && let Some(r) = sel.clone()
            {
                crate::clipboard::set(&text[r.clone()]);
                text.replace_range(r.clone(), "");
                let st = ui.state_mut::<TextEditState>(id);
                st.caret = r.start;
                st.selection = None;
            }
            if MenuItem::new("Copy")
                .shortcut("⌘C")
                .enabled(has_sel)
                .show(ui)
                .clicked()
                && let Some(r) = sel.clone()
            {
                crate::clipboard::set(&text[r]);
            }
            if MenuItem::new("Paste")
                .shortcut("⌘V")
                .enabled(cb_has)
                .show(ui)
                .clicked()
            {
                let cb = crate::clipboard::get();
                let st_snap = *ui.state_mut::<TextEditState>(id);
                let new_caret = if let Some(r) = st_snap.sel_range() {
                    text.replace_range(r.clone(), &cb);
                    r.start + cb.len()
                } else {
                    text.insert_str(st_snap.caret, &cb);
                    st_snap.caret + cb.len()
                };
                let st = ui.state_mut::<TextEditState>(id);
                st.caret = new_caret;
                st.selection = None;
            }
            MenuItem::separator(ui);
            if MenuItem::new("Clear").enabled(has_text).show(ui).clicked() {
                text.clear();
                let st = ui.state_mut::<TextEditState>(id);
                st.caret = 0;
                st.selection = None;
            }
        });

        response
    }
}

impl Configure for TextEdit<'_> {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}

/// Result of one frame's input pass over a TextEdit: the caret byte
/// and the (sorted) selection range, if any. Painter consumes both.
struct InputResult {
    caret: usize,
    selection: Option<std::ops::Range<usize>>,
}

/// Process this frame's pointer + keyboard input for one TextEdit
/// widget and return the caret + selection to render. Splitting this
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
) -> InputResult {
    let resp_state = ui.response_for(id);

    // Hold the state row once for the whole function. `ui.state`,
    // `ui.input`, and `ui.text` are disjoint fields of `Ui`,
    // so we can keep `&mut state` alive while also reading the input
    // queues and dispatching to the text measurer.
    let state = ui
        .state
        .get_or_insert_with::<TextEditState, _>(id, Default::default);
    // Clamp caret + anchor. Host code may have shrunk `*text` between
    // frames; an OOB anchor would corrupt the selection range derivation.
    if state.caret > text.len() {
        state.caret = text.len();
    }
    if let Some(a) = state.selection
        && a > text.len()
    {
        state.selection = Some(text.len());
    }
    if state.selection == Some(state.caret) {
        state.selection = None;
    }

    // Click + drag-to-select. On the rising edge of `pressed`, latch the
    // hit caret as the drag anchor and clear any prior selection. On
    // subsequent pressed frames, the active end follows the pointer and
    // the anchor flips into `selection` once it diverges. On release
    // (falling edge), drop the anchor so the next press starts fresh.
    if resp_state.pressed
        && let (Some(rect), Some(ptr)) = (resp_state.rect, ui.input.pointer_pos)
    {
        let local_x = ptr.x - rect.min.x - pad_left;
        let hit = caret_from_x(text, local_x, font_size, line_height_px, &ui.text);
        if !state.prev_pressed {
            state.drag_anchor = Some(hit);
            state.selection = None;
            state.caret = hit;
        } else {
            let anchor = state.drag_anchor.unwrap_or(hit);
            state.caret = hit;
            state.selection = if hit == anchor { None } else { Some(anchor) };
        }
    } else if !resp_state.pressed {
        state.drag_anchor = None;
    }
    state.prev_pressed = resp_state.pressed;

    if !is_focused {
        return InputResult {
            caret: state.caret,
            selection: state.sel_range(),
        };
    }

    // Drain per-frame keyboard queues. `frame_text` first (so a
    // ModifiersChanged + keystrokes-for-shortcut sequence still leaves
    // typed text intact), then `frame_keys` for navigation/edits.
    if !ui.input.frame_text.is_empty() {
        delete_selection(text, state);
        text.insert_str(state.caret, &ui.input.frame_text);
        state.caret += ui.input.frame_text.len();
    }

    for kp in &ui.input.frame_keys {
        if apply_key(text, state, *kp) {
            *blur_after = true;
        }
    }

    InputResult {
        caret: state.caret,
        selection: state.sel_range(),
    }
}

/// Apply one keypress to the buffer + state. Returns `true` if the
/// caller should blur (Escape with no live selection); every other
/// recognized key is consumed silently. Single-line v1 ignores Enter /
/// Tab / PageUp / PageDown.
fn apply_key(text: &mut String, state: &mut TextEditState, kp: KeyPress) -> bool {
    // Select-all: ctrl+A on Win/Linux, cmd+A on macOS. Routed before
    // the `Char` insert branch so it doesn't get swallowed by the
    // any_command suppression below.
    if let Key::Char(c) = kp.key
        && (c == 'a' || c == 'A')
        && (kp.mods.ctrl || kp.mods.meta)
        && !kp.mods.alt
    {
        if !text.is_empty() {
            state.selection = Some(0);
            state.caret = text.len();
            if state.selection == Some(state.caret) {
                state.selection = None;
            }
        }
        return false;
    }
    let shift = kp.mods.shift;
    match kp.key {
        Key::Char(c) if !kp.mods.any_command() => {
            delete_selection(text, state);
            let mut buf = [0u8; 4];
            let s = c.encode_utf8(&mut buf);
            text.insert_str(state.caret, s);
            state.caret += s.len();
        }
        Key::Backspace if !delete_selection(text, state) && state.caret > 0 => {
            let prev = prev_char_boundary(text, state.caret);
            text.replace_range(prev..state.caret, "");
            state.caret = prev;
        }
        Key::Delete if !delete_selection(text, state) && state.caret < text.len() => {
            let next = next_char_boundary(text, state.caret);
            text.replace_range(state.caret..next, "");
        }
        // Backspace/Delete after the guard's `delete_selection` ran:
        // the guard already consumed a live selection or established
        // we're at the buffer edge; nothing left to do.
        Key::Backspace | Key::Delete => {}
        Key::ArrowLeft => {
            let target = if !shift && let Some(r) = state.sel_range() {
                r.start
            } else {
                prev_char_boundary(text, state.caret)
            };
            move_caret(state, target, shift);
        }
        Key::ArrowRight => {
            let target = if !shift && let Some(r) = state.sel_range() {
                r.end
            } else {
                next_char_boundary(text, state.caret)
            };
            move_caret(state, target, shift);
        }
        Key::Home => move_caret(state, 0, shift),
        Key::End => move_caret(state, text.len(), shift),
        Key::Escape => {
            // Two-stage: collapse selection first, blur only when
            // there's no selection to drop.
            if state.selection.is_some() {
                state.selection = None;
            } else {
                return true;
            }
        }
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
    m: &crate::text::TextShaper,
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
