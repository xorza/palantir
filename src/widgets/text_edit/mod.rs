use crate::forest::element::{Configure, Element, LayoutMode};
use crate::input::keyboard::{Key, KeyPress};
use crate::input::sense::Sense;
use crate::input::shortcut::Shortcut;
use crate::layout::types::clip_mode::ClipMode;
use crate::primitives::rect::Rect;
use crate::primitives::spacing::Spacing;
use crate::primitives::stroke::Stroke;
use crate::primitives::widget_id::WidgetId;
use crate::shape::{Shape, TextWrap};
use crate::ui::Ui;
use crate::widgets::Response;
use crate::widgets::context_menu::{ContextMenu, MenuItem};
use crate::widgets::theme::TextEditTheme;
use glam::Vec2;
use std::borrow::Cow;
use std::collections::VecDeque;
use std::time::Duration;

/// Half-period of the caret blink, in seconds. The caret is visible
/// for `BLINK_HALF` then hidden for `BLINK_HALF`, repeating. Reset
/// to the visible phase on every caret or text change so during
/// active typing the caret stays solid.
const BLINK_HALF: f32 = 0.5;

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
    /// `Ui::time` snapshot from the last frame the caret moved, text
    /// changed, or selection shifted. The blink phase is computed
    /// against this so the caret stays solid for the first
    /// [`BLINK_HALF`] seconds after any input.
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

fn record_edit(text: &str, state: &mut TextEditState, kind: EditKind) {
    let coalesce =
        kind != EditKind::Other && state.last_edit_kind == Some(kind) && !state.undo.is_empty();
    if !coalesce {
        if state.undo.len() >= UNDO_LIMIT {
            state.undo.pop_front();
        }
        state.undo.push_back(EditSnapshot {
            text: text.to_owned(),
            caret: state.caret,
            selection: state.selection,
        });
    }
    state.redo.clear();
    state.last_edit_kind = Some(kind);
}

fn apply_history(text: &mut String, state: &mut TextEditState, snap: EditSnapshot) {
    *text = snap.text;
    state.caret = snap.caret.min(text.len());
    state.selection = snap
        .selection
        .filter(|&a| a <= text.len() && a != state.caret);
    state.last_edit_kind = None;
}

fn apply_undo(text: &mut String, state: &mut TextEditState) {
    let Some(snap) = state.undo.pop_back() else {
        return;
    };
    state.redo.push(EditSnapshot {
        text: text.clone(),
        caret: state.caret,
        selection: state.selection,
    });
    apply_history(text, state, snap);
}

fn apply_redo(text: &mut String, state: &mut TextEditState) {
    let Some(snap) = state.redo.pop() else { return };
    state.undo.push_back(EditSnapshot {
        text: text.clone(),
        caret: state.caret,
        selection: state.selection,
    });
    apply_history(text, state, snap);
}

/// Cut the live selection to the clipboard. No-op when nothing is
/// selected — caller can use `state.sel_range().is_some()` to gate
/// menu / shortcut UI affordances.
fn cut_selection(text: &mut String, state: &mut TextEditState) {
    let Some(r) = state.sel_range() else { return };
    crate::clipboard::set(&text[r.clone()]);
    record_edit(text, state, EditKind::Other);
    text.replace_range(r.clone(), "");
    state.caret = r.start;
    state.selection = None;
}

/// Paste `raw` at the caret, replacing any live selection. Sanitizes
/// line breaks for single-line editors so `\n` / `\r` never enter the
/// buffer. No-op on an empty clipboard.
fn paste_at_caret(text: &mut String, state: &mut TextEditState, raw: &str, multiline: bool) {
    let cleaned: Cow<'_, str> = if multiline {
        Cow::Borrowed(raw)
    } else {
        Cow::Owned(sanitize_single_line(raw))
    };
    if cleaned.is_empty() {
        return;
    }
    record_edit(text, state, EditKind::Other);
    delete_selection(text, state);
    text.insert_str(state.caret, &cleaned);
    state.caret += cleaned.len();
}

fn clear_buffer(text: &mut String, state: &mut TextEditState) {
    if text.is_empty() {
        return;
    }
    record_edit(text, state, EditKind::Other);
    text.clear();
    state.caret = 0;
    state.selection = None;
}

impl TextEditState {
    fn sel_range(&self) -> Option<std::ops::Range<usize>> {
        let a = self.selection?;
        Some(a.min(self.caret)..a.max(self.caret))
    }
}

/// Move the caret to `new_caret`, extending the selection if `extend`
/// is set (latches anchor on the first extending move) or collapsing it
/// otherwise. Maintains the "never Some(caret)" invariant. Always ends
/// the current edit-coalesce group — caret-only motion breaks Typing /
/// Delete runs into separate undo entries.
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
    state.last_edit_kind = None;
}

/// Strip line-break chars from an inbound string so the single-line
/// TextEdit's buffer never contains `\n` / `\r`. Hit by both the
/// paste path and the IME-text-commit path — host events and OS
/// clipboards routinely carry `\r\n` / `\n` from multi-line sources
/// that this widget can't render or hit-test correctly. Spaces are a
/// safer substitute than outright deletion (preserves intent for
/// "First Name\nLast Name" → "First Name Last Name").
fn sanitize_single_line(s: &str) -> String {
    if !s.contains(['\n', '\r']) {
        return s.to_owned();
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
    out
}

/// Scroll the editor so the caret stays inside the visible inner
/// rect, updating `state.scroll` in place and returning the new
/// offset (which the caller subtracts from every emitted shape).
/// Single-line scrolls only on the x axis; multi-line wraps to inner
/// width so only y scrolls. `response_rect` is one frame stale: on
/// the first frame the widget is recorded it's `None` and scroll
/// stays at zero — acceptable, the caret is at byte 0 then anyway.
#[allow(clippy::too_many_arguments)]
fn update_scroll(
    state: &mut TextEditState,
    response_rect: Option<Rect>,
    padding: Spacing,
    multiline: bool,
    caret_x: f32,
    caret_y_top: f32,
    line_height: f32,
    caret_width: f32,
) -> Vec2 {
    let Some(rect) = response_rect else {
        state.scroll = Vec2::ZERO;
        return Vec2::ZERO;
    };
    let inner_w = (rect.size.w - padding.horiz()).max(0.0);
    let inner_h = (rect.size.h - padding.vert()).max(0.0);
    if multiline {
        // Only y scrolls — wrap kills horizontal overflow.
        state.scroll.x = 0.0;
        let caret_bottom = caret_y_top + line_height;
        if caret_y_top < state.scroll.y {
            state.scroll.y = caret_y_top;
        } else if caret_bottom > state.scroll.y + inner_h {
            state.scroll.y = caret_bottom - inner_h;
        }
        state.scroll.y = state.scroll.y.max(0.0);
    } else {
        state.scroll.y = 0.0;
        let caret_right = caret_x + caret_width;
        if caret_x < state.scroll.x {
            state.scroll.x = caret_x;
        } else if caret_right > state.scroll.x + inner_w {
            state.scroll.x = caret_right - inner_w;
        }
        state.scroll.x = state.scroll.x.max(0.0);
    }
    state.scroll
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

/// Editable text leaf. Supports typing (`KeyDown` printable chars or
/// IME `Text` commits), backspace/delete, left/right (+ shift / home /
/// end), drag-select, multi-line, cut/copy/paste, undo+redo
/// (Cmd/Ctrl+Z, Cmd/Ctrl+Shift+Z), escape-to-blur, click-to-place-caret.
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
    /// When `true`, Enter inserts `\n`, paste/IME preserve newlines,
    /// click hit-test + caret + selection render in 2D, and text
    /// soft-wraps to the editor's inner width via cosmic-text. v1
    /// single-line behaviour is the default — flip via [`Self::multiline`].
    multiline: bool,
}

impl<'a> TextEdit<'a> {
    #[track_caller]
    pub fn new(text: &'a mut String) -> Self {
        let mut element = Element::new(LayoutMode::Leaf);
        element.sense = Sense::CLICK;
        element.focusable = true;
        // Clip glyphs, caret, and selection wash to the editor's own
        // rect so a `Fixed`-sized editor with long content doesn't
        // bleed over its neighbours. Chrome (background) draws before
        // the clip, so the editor's surround still paints normally.
        element.clip = ClipMode::Rect;
        // `Element::padding` left at zero — `show()` substitutes
        // `theme.text_edit.padding` when the user didn't call
        // `.padding(...)`. Same renderer semantics as before; the
        // value just lives on the theme instead of hard-coded here.
        Self {
            element,
            text,
            style: None,
            placeholder: Cow::Borrowed(""),
            multiline: false,
        }
    }

    /// Switch to multi-line mode. Enter inserts `\n` (instead of
    /// blurring), paste / IME-text preserve newlines, text soft-wraps
    /// to the editor's inner width, and click/caret/selection all
    /// route through cosmic-text's 2D layout.
    pub fn multiline(mut self, on: bool) -> Self {
        self.multiline = on;
        self
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

        // Wrap target for multi-line: editor's inner width (outer −
        // padding). Read from the previous arrange via `response.rect`
        // — cascade runs in `post_record` so the value is up-to-date
        // both in steady state and across `request_relayout` passes.
        // `None` on the first frame the widget is recorded; cosmic
        // then lays out unbounded (single visual line per `\n` chunk)
        // until the next frame catches up.
        let wrap_target: Option<f32> = if self.multiline {
            response.rect.map(|r| (r.size.w - padding.horiz()).max(1.0))
        } else {
            None
        };

        // Phase 1: input handling. Touches `ui.state` and `ui.input`
        // (separate fields, disjoint borrows). Click-to-place-caret
        // happens here too, before keystroke processing, so a click +
        // type in the same frame inserts at the click point.
        // Snapshot caret + selection + text length before input so
        // the blink reset can detect any change without instrumenting
        // every mutation site.
        let text_len_before = self.text.len();
        let (caret_before, sel_before) = {
            let s = ui.state_mut::<TextEditState>(id);
            (s.caret, s.selection)
        };
        let mut blur_after = false;
        let input = handle_input(
            ui,
            id,
            is_focused,
            self.text,
            font_size,
            font_size * line_height_mult,
            padding,
            self.multiline,
            wrap_target,
            look.text.family,
            &mut blur_after,
        );
        let caret_byte = input.caret;
        let selection = input.selection;
        let caret_changed = caret_before != caret_byte
            || sel_before != ui.state_mut::<TextEditState>(id).selection
            || text_len_before != self.text.len();

        // Phase 2: scroll-to-caret. Compute the caret position in
        // unscrolled coords once (used for both the scroll update
        // and the caret shape), then adjust `state.scroll` so the
        // caret stays inside the visible area. `response.rect` is
        // one frame stale; on the first recorded frame it's `None`
        // and scroll defaults to `Vec2::ZERO`.
        let caret_pos = ui.text.cursor_xy(
            self.text,
            caret_byte,
            font_size,
            font_size * line_height_mult,
            wrap_target,
            look.text.family,
        );
        let scroll = update_scroll(
            ui.state_mut::<TextEditState>(id),
            response.rect,
            padding,
            self.multiline,
            caret_pos.x,
            caret_pos.y_top,
            caret_pos.line_height,
            theme.caret_width,
        );

        // Caret blink. Reset to phase 0 (solid) on any caret /
        // selection / text change so active typing always paints a
        // visible caret. Then flip on/off every `BLINK_HALF` seconds.
        // Schedule a wake at the next phase boundary; deadlines
        // dedup so re-scheduling each frame doesn't pile up.
        let now = ui.time;
        let caret_visible = if is_focused {
            let state = ui.state_mut::<TextEditState>(id);
            if caret_changed {
                state.last_caret_change = now;
            }
            let elapsed = now.saturating_sub(state.last_caret_change).as_secs_f32();
            let phase = (elapsed / BLINK_HALF).floor();
            let until_next = ((phase + 1.0) * BLINK_HALF - elapsed).max(0.0);
            ui.request_repaint_after(Duration::from_secs_f32(until_next));
            (phase as u64).is_multiple_of(2)
        } else {
            true
        };

        // Phase 3: open the node and push shapes. `cursor_xy` +
        // `selection_rects` handle both single- and multi-line via
        // cosmic's shaped-buffer APIs; the single-line case is just
        // an unwrapped layout with one visual run. Touch `ui.text`
        // (disjoint from `ui.forest`, so `add_shape` sequences fine).
        // Chrome paints via `Tree::chrome_for` — encoder emits it
        // before any clip. Every shape's local_rect is shifted by
        // `-scroll` so the caret/text/selection wash track the
        // visible viewport; the editor's `ClipMode::Rect` (set in
        // `new()`) scissors anything that slips past the edge.
        let mut element = self.element;
        element.chrome = Some(look.background);
        let placeholder = self.placeholder;
        let text_ptr = &*self.text;
        let multiline = self.multiline;
        let line_h = font_size * line_height_mult;
        let resp_node = ui.node(element, |ui| {
            // Selection highlight, painted *before* the text so glyphs
            // sit on top of the wash. Only when focused and a range is
            // actually live (anchor != caret — collapsed selections are
            // stored as `None`, so any `Some` here has positive width).
            if is_focused && let Some(range) = selection.clone() {
                // `ui.text` (the shaper) and `ui.forest` (where
                // `add_shape` writes) are disjoint fields on `Ui`;
                // cosmic's `LayoutRun::highlight` calls through the
                // shaper while the closure pushes shapes — no
                // `Vec<Rect>` round-trip needed. Single-line lays out
                // unwrapped (`wrap_target=None`) and emits one rect.
                let sel_color = theme.selection;
                ui.text.selection_rects(
                    text_ptr,
                    range,
                    font_size,
                    line_h,
                    wrap_target,
                    look.text.family,
                    |x, y, w, h| {
                        ui.forest.add_shape(Shape::RoundedRect {
                            local_rect: Some(Rect::new(
                                padding.left + x - scroll.x,
                                padding.top + y - scroll.y,
                                w,
                                h,
                            )),
                            radius: Default::default(),
                            fill: sel_color.into(),
                            stroke: Stroke::ZERO,
                        });
                    },
                );
            }

            // Text or placeholder. Empty buffer + unfocused shows the
            // placeholder; focused shows the buffer (even if empty)
            // because we still want the caret to render flush-left.
            // `local_rect: Some(...)` positions the shaped text at
            // owner-local `(padding − scroll)`; size carries the
            // visible inner extent but doesn't affect alignment under
            // `Align::Auto` (text origin sits at `leaf.min`).
            let (display, color) = if text_ptr.is_empty() && !is_focused {
                (placeholder.clone(), theme.placeholder)
            } else {
                (Cow::Owned(text_ptr.clone()), look.text.color)
            };
            if !display.is_empty() {
                ui.add_shape(Shape::Text {
                    local_rect: Some(Rect::new(
                        padding.left - scroll.x,
                        padding.top - scroll.y,
                        // Size is unused under `Align::Auto`; pick
                        // something positive so `is_paint_empty`
                        // doesn't reject the shape.
                        1.0,
                        1.0,
                    )),
                    text: display,
                    brush: color.into(),
                    font_size_px: font_size,
                    line_height_px: line_h,
                    wrap: if multiline {
                        TextWrap::Wrap
                    } else {
                        TextWrap::Single
                    },
                    align: Default::default(),
                    family: look.text.family,
                });
            }

            // Caret. Painted as a thin Overlay rect at owner-local
            // coords so it stays in the widget's clip and renders
            // *over* the text. Only when focused and inside the
            // visible half of the blink cycle.
            if is_focused && caret_visible {
                let caret_rect = Rect::new(
                    padding.left + caret_pos.x - scroll.x,
                    padding.top + caret_pos.y_top - scroll.y,
                    theme.caret_width,
                    caret_pos.line_height,
                );
                ui.add_shape(Shape::RoundedRect {
                    local_rect: Some(caret_rect),
                    radius: Default::default(),
                    fill: theme.caret.into(),
                    stroke: Stroke::ZERO,
                });
            }
        });

        // Phase 4: side effects that need a fresh borrow of `ui`
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

        // Phase 5: default Cut / Copy / Paste / Clear context menu.
        // Triggered by secondary click on the editor; items mutate
        // the host's buffer through the same `&mut String` borrow
        // `show` was given, then sync `TextEditState.caret` /
        // `selection` so the next frame paints the right place.
        // Selection range + clipboard liveness snapshotted before
        // the closure so the items can render with the right
        // `.enabled(...)` per state.
        let sel = ui.state_mut::<TextEditState>(id).sel_range();
        let has_sel = sel.is_some();
        let cb_has = !crate::clipboard::get().is_empty();
        let has_text = !self.text.is_empty();
        let text = self.text;
        ContextMenu::attach(ui, &response).show(ui, |ui, popup| {
            if MenuItem::new("Cut")
                .shortcut(Shortcut::cmd('X'))
                .enabled(has_sel)
                .show(ui, popup)
                .clicked()
            {
                cut_selection(text, ui.state_mut::<TextEditState>(id));
            }
            if MenuItem::new("Copy")
                .shortcut(Shortcut::cmd('C'))
                .enabled(has_sel)
                .show(ui, popup)
                .clicked()
                && let Some(r) = sel.clone()
            {
                crate::clipboard::set(&text[r]);
            }
            if MenuItem::new("Paste")
                .shortcut(Shortcut::cmd('V'))
                .enabled(cb_has)
                .show(ui, popup)
                .clicked()
            {
                paste_at_caret(
                    text,
                    ui.state_mut::<TextEditState>(id),
                    &crate::clipboard::get(),
                    multiline,
                );
            }
            MenuItem::separator(ui);
            if MenuItem::new("Clear")
                .enabled(has_text)
                .show(ui, popup)
                .clicked()
            {
                clear_buffer(text, ui.state_mut::<TextEditState>(id));
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
    // Resolved padding from the per-widget style (after merging
    // `.style()` override with the theme default). Subtracted from
    // the press position so caret/click hit-test runs in text-local
    // coords. Multi-line uses both axes; single-line only `.left`.
    padding: Spacing,
    multiline: bool,
    wrap_target: Option<f32>,
    family: crate::text::FontFamily,
    blur_after: &mut bool,
) -> InputResult {
    let resp_state = ui.response_for(id);
    // Snapshot once before the long `&mut state` borrow below. The
    // menu and the text-edit state live under the same WidgetId but
    // different TypeIds; the borrow checker can't see the disjoint
    // rows so we read the menu row first.
    let clipboard_active = !ContextMenu::is_open(ui, id);

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
        // Hit-test runs against the *unscrolled* shaped layout, so
        // we add last frame's scroll back into the pointer's local
        // coords. Updated scroll for this frame is computed after
        // `handle_input` returns — the user clicked on what they
        // saw, which is last frame's scroll.
        let local_x = ptr.x - rect.min.x - padding.left + state.scroll.x;
        let local_y = ptr.y - rect.min.y - padding.top + state.scroll.y;
        // `byte_at_xy` handles both axes; single-line probes at
        // `y=0` (against an unwrapped layout) collapse to cosmic's
        // 1D `Buffer::hit` walk — one shaped lookup.
        let hit = ui.text.byte_at_xy(
            text,
            local_x,
            if multiline { local_y } else { 0.0 },
            font_size,
            line_height_px,
            wrap_target,
            family,
        );
        if !state.prev_pressed {
            state.drag_anchor = Some(hit);
            state.selection = None;
            state.caret = hit;
            state.last_edit_kind = None;
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
        let to_insert: String = if multiline {
            ui.input.frame_text.clone()
        } else {
            sanitize_single_line(&ui.input.frame_text)
        };
        if !to_insert.is_empty() {
            record_edit(text, state, EditKind::Typing);
            delete_selection(text, state);
            text.insert_str(state.caret, &to_insert);
            state.caret += to_insert.len();
        }
    }

    // Drain keys via the pure `apply_key`; vertical-nav probes
    // happen inline because they need the shaper + layout. Indexing
    // (instead of iter) keeps the borrow on `frame_keys` short-lived
    // so we can dispatch to `ui.text` inside the same loop without a
    // scratch Vec.
    let mut vert: Option<VerticalMotion> = None;
    let n = ui.input.frame_keys.len();
    for i in 0..n {
        let kp = ui.input.frame_keys[i];
        if apply_key(text, state, kp, multiline, clipboard_active, &mut vert) {
            *blur_after = true;
        }
        if let Some(v) = vert.take() {
            let pos = ui.text.cursor_xy(
                text,
                state.caret,
                font_size,
                line_height_px,
                wrap_target,
                family,
            );
            let probe_y = match v.direction {
                VerticalDir::Up => pos.y_top - 1.0,
                VerticalDir::Down => pos.y_top + pos.line_height + 1.0,
            };
            let target = if matches!(v.direction, VerticalDir::Up) && pos.y_top <= 0.5 {
                0
            } else {
                ui.text.byte_at_xy(
                    text,
                    pos.x,
                    probe_y,
                    font_size,
                    line_height_px,
                    wrap_target,
                    family,
                )
            };
            move_caret(state, target, v.extend);
        }
    }

    InputResult {
        caret: state.caret,
        selection: state.sel_range(),
    }
}

/// Up/Down direction emitted by [`apply_key`] for the caller to
/// resolve against the shaper's 2D layout. Multi-line nav needs the
/// editor's `font_size` / `line_height` / `wrap_target` to probe one
/// line above or below the current caret; pulling that resolution out
/// of `apply_key` keeps the function pure on `(text, state, key)`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct VerticalMotion {
    direction: VerticalDir,
    extend: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum VerticalDir {
    Up,
    Down,
}

/// Apply one keypress to the buffer + state. Returns `true` if the
/// caller should blur (Escape with no live selection in single-line
/// mode); every other recognized key is consumed silently. Sets
/// `out_vertical` to `Some(VerticalMotion)` when the key is `Up` /
/// `Down` in multi-line mode — the caller resolves the cross-axis
/// probe (it needs the shaper + layout context, which this pure
/// function doesn't carry). Multi-line mode also treats `Enter` as
/// `\n` insertion.
fn apply_key(
    text: &mut String,
    state: &mut TextEditState,
    kp: KeyPress,
    multiline: bool,
    clipboard_active: bool,
    out_vertical: &mut Option<VerticalMotion>,
) -> bool {
    // Platform clipboard shortcuts. Routed before the `Char` insert
    // branch so they don't get swallowed by the `any_command`
    // suppression below. Gated by `clipboard_active` so an open
    // context menu can intercept the same bindings.
    const SELECT_ALL: Shortcut = Shortcut::cmd('A');
    const COPY: Shortcut = Shortcut::cmd('C');
    const CUT: Shortcut = Shortcut::cmd('X');
    const PASTE: Shortcut = Shortcut::cmd('V');
    const UNDO: Shortcut = Shortcut::cmd('Z');
    const REDO: Shortcut = Shortcut::cmd_shift('Z');

    if UNDO.matches(kp) {
        apply_undo(text, state);
        return false;
    }
    if REDO.matches(kp) {
        apply_redo(text, state);
        return false;
    }
    if clipboard_active {
        if SELECT_ALL.matches(kp) {
            if !text.is_empty() {
                state.selection = Some(0);
                state.caret = text.len();
                state.last_edit_kind = None;
            }
            return false;
        }
        if COPY.matches(kp) {
            if let Some(r) = state.sel_range() {
                crate::clipboard::set(&text[r]);
            }
            return false;
        }
        if CUT.matches(kp) {
            cut_selection(text, state);
            return false;
        }
        if PASTE.matches(kp) {
            paste_at_caret(text, state, &crate::clipboard::get(), multiline);
            return false;
        }
    }
    let shift = kp.mods.shift;
    match kp.key {
        Key::Char(c) if !kp.mods.any_command() => {
            record_edit(text, state, EditKind::Typing);
            delete_selection(text, state);
            let mut buf = [0u8; 4];
            let s = c.encode_utf8(&mut buf);
            text.insert_str(state.caret, s);
            state.caret += s.len();
        }
        Key::Backspace => {
            let has_sel = state.sel_range().is_some();
            if has_sel || state.caret > 0 {
                record_edit(text, state, EditKind::Delete);
                if !delete_selection(text, state) {
                    let prev = prev_char_boundary(text, state.caret);
                    text.replace_range(prev..state.caret, "");
                    state.caret = prev;
                }
            }
        }
        Key::Delete => {
            let has_sel = state.sel_range().is_some();
            if has_sel || state.caret < text.len() {
                record_edit(text, state, EditKind::Delete);
                if !delete_selection(text, state) {
                    let next = next_char_boundary(text, state.caret);
                    text.replace_range(state.caret..next, "");
                }
            }
        }
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
        // Vertical motion in multi-line mode emits a `VerticalMotion`
        // for the caller to resolve against the shaper (needs layout
        // context this pure fn doesn't carry). Caret hasn't moved yet
        // so the coalesce reset rides on the caller's `move_caret`.
        Key::ArrowUp if multiline => {
            *out_vertical = Some(VerticalMotion {
                direction: VerticalDir::Up,
                extend: shift,
            });
        }
        Key::ArrowDown if multiline => {
            *out_vertical = Some(VerticalMotion {
                direction: VerticalDir::Down,
                extend: shift,
            });
        }
        Key::Enter if multiline => {
            record_edit(text, state, EditKind::Other);
            delete_selection(text, state);
            text.insert(state.caret, '\n');
            state.caret += 1;
        }
        Key::Home => move_caret(state, 0, shift),
        Key::End => move_caret(state, text.len(), shift),
        Key::Escape => {
            // Two-stage: collapse selection first, blur only when
            // there's no selection to drop.
            if state.selection.is_some() {
                state.selection = None;
                state.last_edit_kind = None;
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

#[cfg(test)]
mod tests;
