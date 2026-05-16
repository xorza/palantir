use crate::animation::paint::PaintAnim;
use crate::forest::element::{Configure, Element, LayoutMode};
use crate::input::keyboard::{Key, KeyPress, KeyboardEvent, Modifiers};
use crate::input::sense::Sense;
use crate::input::shortcut::Shortcut;
use crate::layout::types::align::{Align, HAlign, VAlign};
use crate::layout::types::clip_mode::ClipMode;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::stroke::Stroke;
use crate::primitives::widget_id::WidgetId;
use crate::shape::{Shape, TextWrap};
use crate::text::{CursorPos, FontFamily};
use crate::ui::Ui;
use crate::widgets::Response;
use crate::widgets::context_menu::{ContextMenu, MenuItem};
use crate::widgets::theme::text_edit::TextEditTheme;
use glam::Vec2;
use std::borrow::Cow;
use std::collections::VecDeque;
use std::time::Duration;

/// Half-period of the caret blink, in seconds. The caret is visible
/// for `BLINK_HALF` then hidden for `BLINK_HALF`, repeating. Reset
/// to the visible phase on every caret or text change so during
/// active typing the caret stays solid.
const BLINK_HALF: f32 = 0.5;

/// Max time between presses still counted as one multi-click
/// sequence. Standard OS default.
const MULTI_CLICK_WINDOW: f32 = 0.5;

/// Max distance (in logical px) between consecutive presses still
/// counted as the same multi-click sequence.
const MULTI_CLICK_RADIUS: f32 = 5.0;

/// After this long without caret/text/selection change, the blink
/// stops scheduling wakes and the caret stays solid — saves the host
/// a forever 2 Hz repaint loop on a focused-but-idle editor.
const BLINK_STOP_AFTER_IDLE: f32 = 30.0;

/// Cross-frame state for one [`TextEdit`]. Stored in [`Ui`]'s
/// `WidgetId → Any` map keyed by the widget's id; lifecycle managed by
/// the same removed-widget sweep that drives the layout/text caches.
///
/// `caret` is a *byte* offset into the buffer (cosmic-text returns
/// byte cursors and `&buffer[..caret]` is the natural prefix-measure
/// path). All widget-driven mutations step grapheme-cluster boundaries
/// (which are themselves codepoint-aligned), so the caret should never
/// land mid-codepoint. Host code that mutates the buffer between
/// frames may shrink it past `caret`; `show()` clamps at the top each
/// frame.
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
    /// `Ui::time` snapshot from the last frame the caret moved, text
    /// changed, or selection shifted. The blink phase is computed
    /// against this so the caret stays solid for the first
    /// [`BLINK_HALF`] seconds after any input.
    pub(crate) last_caret_change: Duration,
    /// `Ui::time` of the most recent press rising-edge. Compared
    /// against the next press to detect double/triple clicks within
    /// [`MULTI_CLICK_WINDOW`].
    pub(crate) last_press_time: Duration,
    /// Pointer position of the most recent press, for the "click
    /// didn't move" half of the multi-click predicate.
    pub(crate) last_press_pos: Vec2,
    /// Running click count for the current multi-click sequence.
    /// 1 = single click, 2 = double, ≥3 = triple. Reset to 1 once
    /// the time or distance threshold is exceeded.
    pub(crate) click_count: u8,
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
    assert!(snap.caret <= snap.text.len());
    *text = snap.text;
    state.caret = snap.caret;
    state.selection = snap.selection.filter(|&a| a != state.caret);
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

/// Place a text bbox of size `measured` inside an inner rect of size
/// `inner` per `align`, returning the top-left offset. Overflow on
/// either axis clamps that axis to zero, matching the encoder's
/// `align_text_in` so widget-side caret/selection placement can't
/// drift from the rendered glyphs. The widget owns this rather than
/// delegating to `Shape::Text.align` because caret + selection rects
/// must use the same offset, and they aren't `Shape::Text`.
fn align_offset(inner: Size, measured: Size, align: Align) -> Vec2 {
    let dx = match align.halign() {
        HAlign::Auto | HAlign::Left | HAlign::Stretch => 0.0,
        HAlign::Center => (inner.w - measured.w) * 0.5,
        HAlign::Right => inner.w - measured.w,
    };
    let dy = match align.valign() {
        VAlign::Auto | VAlign::Top | VAlign::Stretch => 0.0,
        VAlign::Center => (inner.h - measured.h) * 0.5,
        VAlign::Bottom => inner.h - measured.h,
    };
    Vec2::new(dx.max(0.0), dy.max(0.0))
}

/// Bundle of text-shape parameters resolved once at the top of
/// `show()` and threaded down to input handling, scroll, and caret
/// resolution. All fields are read-only for the duration of one
/// `show()` call. `halign` is the alignment the shaper applies
/// per-line when `wrap_target.is_some()` — it has to travel through
/// every shaper call so the cached buffer's `TextCacheKey` matches.
#[derive(Clone, Copy)]
struct ShapeCtx {
    font_size: f32,
    line_height_px: f32,
    padding: Spacing,
    wrap_target: Option<f32>,
    family: FontFamily,
    multiline: bool,
    halign: HAlign,
}

/// Scroll the editor so the caret stays inside the visible inner
/// rect, mutating `state.scroll` in place. Single-line scrolls only
/// on the x axis; multi-line wraps to inner width so only y scrolls.
/// `response_rect` is one frame stale: on the first frame the widget
/// is recorded it's `None` and scroll stays at zero — acceptable, the
/// caret is at byte 0 then anyway.
fn update_scroll(
    state: &mut TextEditState,
    response_rect: Option<Rect>,
    ctx: &ShapeCtx,
    caret_pos: CursorPos,
    caret_width: f32,
) {
    let Some(rect) = response_rect else {
        state.scroll = Vec2::ZERO;
        return;
    };
    let inner_w = (rect.size.w - ctx.padding.horiz()).max(0.0);
    let inner_h = (rect.size.h - ctx.padding.vert()).max(0.0);
    if ctx.multiline {
        state.scroll.x = 0.0;
        let caret_bottom = caret_pos.y_top + caret_pos.line_height;
        if caret_pos.y_top < state.scroll.y {
            state.scroll.y = caret_pos.y_top;
        } else if caret_bottom > state.scroll.y + inner_h {
            state.scroll.y = caret_bottom - inner_h;
        }
        state.scroll.y = state.scroll.y.max(0.0);
    } else {
        state.scroll.y = 0.0;
        let caret_right = caret_pos.x + caret_width;
        if caret_pos.x < state.scroll.x {
            state.scroll.x = caret_pos.x;
        } else if caret_right > state.scroll.x + inner_w {
            state.scroll.x = caret_right - inner_w;
        }
        state.scroll.x = state.scroll.x.max(0.0);
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
    /// Caller-supplied alignment of the text inside the editor's
    /// inner rect. `None` means "pick the mode-appropriate default" —
    /// `Align::LEFT` (left + vcenter) for single-line, `Align::TOP_LEFT`
    /// for multi-line. Caret and selection rects derive from the same
    /// offset, so any alignment keeps them tracking the glyphs.
    text_align: Option<Align>,
}

impl<'a> TextEdit<'a> {
    #[track_caller]
    pub fn new(text: &'a mut String) -> Self {
        let mut element = Element::new(LayoutMode::Leaf);
        element.set_sense(Sense::CLICK);
        element.set_focusable(true);
        // Clip glyphs, caret, and selection wash to the editor's own
        // rect so a `Fixed`-sized editor with long content doesn't
        // bleed over its neighbours. Chrome (background) draws before
        // the clip, so the editor's surround still paints normally.
        element.set_clip(ClipMode::Rect);
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
            text_align: None,
        }
    }

    /// Position of the text inside the editor's inner rect (the rect
    /// minus padding). Defaults: `Align::LEFT` (left + vcenter) for
    /// single-line, `Align::TOP_LEFT` for multi-line. Overflow clamps
    /// the offset to zero on each axis so caret + horizontal scroll
    /// keep working when the text exceeds the inner rect. Distinct
    /// from [`Configure::align`], which positions the *widget* inside
    /// its parent's stack slot.
    pub fn text_align(mut self, a: Align) -> Self {
        self.text_align = Some(a);
        self
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
        response.disabled |= self.element.is_disabled();
        let fallback_text = ui.theme.text;
        let look = theme
            .pick(response)
            .animate(ui, id, fallback_text, theme.anim);
        let font_size = look.text.font_size_px;
        let line_height_mult = look.text.line_height_mult;
        let padding = self.element.padding;
        // Reserve a caret-width sliver at the trailing edge of every
        // line so a caret sitting at end-of-line on right/center-
        // aligned text stays inside the clip. The shaper's per-line
        // halign and the widget's single-line `align_offset` both see
        // the same reduced width, so glyphs + caret + selection wash
        // shift together and click hit-test (which reads back the
        // same `align_offset`) stays consistent.
        let caret_room = theme.caret_width.max(0.0);

        // Wrap target for multi-line: editor's inner width (outer −
        // padding − caret room). Read from the previous arrange via
        // `response.rect` — cascade runs in `post_record` so the value
        // is up-to-date both in steady state and across
        // `request_relayout` passes. `None` on the first frame the
        // widget is recorded; cosmic then lays out unbounded (single
        // visual line per `\n` chunk) until the next frame catches up.
        let wrap_target: Option<f32> = if self.multiline {
            response
                .rect
                .map(|r| (r.size.w - padding.horiz() - caret_room).max(1.0))
        } else {
            None
        };
        // Resolved alignment: explicit `.text_align(...)` wins, else
        // the mode-appropriate default. Single-line vcenters the one
        // visual line; multi-line top-lefts so growing content fills
        // downward.
        let text_align = self.text_align.unwrap_or(if self.multiline {
            Align::TOP_LEFT
        } else {
            Align::LEFT
        });
        let ctx = ShapeCtx {
            font_size,
            line_height_px: font_size * line_height_mult,
            padding,
            wrap_target,
            family: look.text.family,
            multiline: self.multiline,
            halign: text_align.halign(),
        };
        // Multi-line lets cosmic bake per-line halign offsets into
        // the shaped buffer (`BufferLine::set_align`), so the widget
        // applies only the vertical block offset. Single-line has no
        // wrap target for cosmic to align inside, so the widget
        // computes both axes from the measured bbox itself.
        let widget_align = if ctx.multiline {
            Align::v(text_align.valign())
        } else {
            text_align
        };
        let offset = if let Some(r) = response.rect {
            let measure_str: &str = if !self.text.is_empty() || is_focused {
                self.text
            } else {
                &self.placeholder
            };
            let m = ui
                .text
                .measure(
                    measure_str,
                    ctx.font_size,
                    ctx.line_height_px,
                    ctx.wrap_target,
                    ctx.family,
                    ctx.halign,
                )
                .size;
            let measured = Size::new(m.w, m.h.max(ctx.line_height_px));
            let inner_w = (r.size.w - ctx.padding.horiz() - caret_room).max(0.0);
            let inner_h = (r.size.h - ctx.padding.vert()).max(0.0);
            align_offset(Size::new(inner_w, inner_h), measured, widget_align)
        } else {
            Vec2::ZERO
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
        let input = handle_input(ui, id, is_focused, self.text, &ctx, offset, &mut blur_after);
        let caret_byte = input.caret;
        let selection = input.selection;

        // Phase 2: scroll-to-caret + blink-phase reset. One `state`
        // borrow covers (a) the post-input caret_changed compare for
        // the blink reset, (b) `update_scroll` mutating `state.scroll`,
        // and (c) snapshotting `last_caret_change` for the visibility
        // calc below. `caret_pos` is computed via the shaper (disjoint
        // field) first so the state borrow is contiguous.
        let caret_pos = ui.text.cursor_xy(
            self.text,
            caret_byte,
            ctx.font_size,
            ctx.line_height_px,
            ctx.wrap_target,
            ctx.family,
            ctx.halign,
        );
        let now = ui.time;
        let (scroll, last_caret_change) = {
            let state = ui.state_mut::<TextEditState>(id);
            let caret_changed = caret_before != caret_byte
                || sel_before != state.selection
                || text_len_before != self.text.len();
            let focus_gained = is_focused && !state.prev_focused;
            update_scroll(state, response.rect, &ctx, caret_pos, theme.caret_width);
            if is_focused && (caret_changed || focus_gained) {
                state.last_caret_change = now;
            }
            state.prev_focused = is_focused;
            (state.scroll, state.last_caret_change)
        };

        // Caret blink. `PaintAnim::BlinkOpacity` drives the on/off
        // phase and wake scheduling — encoder skips the caret quad
        // during the hidden half. After `BLINK_STOP_AFTER_IDLE` the
        // caret stays solid and no anim is registered, so an
        // unattended focused editor doesn't keep the host's repaint
        // loop spinning.
        let caret_anim = if is_focused {
            let elapsed = now.saturating_sub(last_caret_change).as_secs_f32();
            (elapsed < BLINK_STOP_AFTER_IDLE).then_some(PaintAnim::BlinkOpacity {
                half_period: Duration::from_secs_f32(BLINK_HALF),
                started_at: last_caret_change,
            })
        } else {
            None
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
        let element = self.element;
        let chrome = look.background;
        let placeholder = self.placeholder;
        let text_ptr = &*self.text;
        ui.node_with_chrome(element, chrome, |ui| {
            // Selection highlight, painted *before* the text so glyphs
            // sit on top of the wash. Only when focused and a range is
            // actually live (anchor != caret — collapsed selections are
            // stored as `None`, so any `Some` here has positive width).
            if is_focused && let Some(range) = selection.clone() {
                // Materialize selection rects via the shaper's out-arg
                // form, then release the `ui.text` borrow before
                // painting through the public `ui.add_shape` API.
                let sel_color = theme.selection;
                let mut rects = crate::text::SelectionRects::new();
                ui.text.selection_rects(
                    text_ptr,
                    range,
                    ctx.font_size,
                    ctx.line_height_px,
                    ctx.wrap_target,
                    ctx.family,
                    ctx.halign,
                    &mut rects,
                );
                let dx = ctx.padding.left() + offset.x - scroll.x;
                let dy = ctx.padding.top() + offset.y - scroll.y;
                for r in rects {
                    ui.add_shape(Shape::RoundedRect {
                        local_rect: Some(Rect {
                            min: r.min + glam::Vec2::new(dx, dy),
                            size: r.size,
                        }),
                        radius: Default::default(),
                        fill: sel_color.into(),
                        stroke: Stroke::ZERO,
                    });
                }
            }

            // Text or placeholder. Empty buffer + unfocused shows the
            // placeholder; focused shows the buffer (even if empty)
            // because we still want the caret to render flush-left.
            // `local_rect: Some(...)` positions the shaped text at
            // owner-local `(padding − scroll)`; the size is unused
            // under `Align::Auto` (text origin sits at `leaf.min`
            // and the painted extent is the shaped glyph bbox).
            let (display, color) = if text_ptr.is_empty() && !is_focused {
                (placeholder.clone(), theme.placeholder)
            } else {
                (Cow::Owned(text_ptr.clone()), look.text.color)
            };
            if !display.is_empty() {
                ui.add_shape(Shape::Text {
                    local_origin: Some(Vec2::new(
                        ctx.padding.left() + offset.x - scroll.x,
                        ctx.padding.top() + offset.y - scroll.y,
                    )),
                    text: display.into(),
                    brush: color.into(),
                    font_size_px: ctx.font_size,
                    line_height_px: ctx.line_height_px,
                    wrap: if ctx.multiline {
                        TextWrap::Wrap
                    } else {
                        TextWrap::Single
                    },
                    // Pass the user's `text_align` so the layout
                    // pipeline's `shape_wrap` builds a `TextCacheKey`
                    // whose `halign_q` matches the buffer the widget
                    // queries via `cursor_xy` / `selection_rects`.
                    // Without this the rendered text shapes against
                    // an `HAlign::Auto` cache entry while the widget
                    // reads from an aligned one — coords match by
                    // accident, but the user sees unaligned text.
                    align: text_align,
                    family: ctx.family,
                });
            }

            // Caret. Painted as a thin Overlay rect at owner-local
            // coords so it stays in the widget's clip and renders
            // *over* the text. `caret_anim = Some(_)` registers a
            // `BlinkOpacity` against the rect — encoder hides it
            // during the off half; `post_record` folds the wake.
            // When unfocused or quiesced, the rect is added without
            // an anim (solid) — unfocused gets skipped at emit time
            // anyway since callers gate on the same is_focused.
            if is_focused {
                let caret_rect = Rect::new(
                    ctx.padding.left() + offset.x + caret_pos.x - scroll.x,
                    ctx.padding.top() + offset.y + caret_pos.y_top - scroll.y,
                    theme.caret_width,
                    caret_pos.line_height,
                );
                let shape = Shape::RoundedRect {
                    local_rect: Some(caret_rect),
                    radius: Default::default(),
                    fill: theme.caret.into(),
                    stroke: Stroke::ZERO,
                };
                match caret_anim {
                    Some(anim) => ui.add_shape_animated(shape, anim),
                    None => ui.add_shape(shape),
                }
            }
        });

        // Phase 4: side effects that need a fresh borrow of `ui`
        // (Escape-to-blur). Done after the node closes so we don't
        // accidentally mutate during recording.
        if blur_after {
            ui.request_focus(None);
        }

        let state = ui.response_for(id);
        let response = Response { id, state };

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
                    ctx.multiline,
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
fn handle_input(
    ui: &mut Ui,
    id: WidgetId,
    is_focused: bool,
    text: &mut String,
    ctx: &ShapeCtx,
    align_offset: Vec2,
    blur_after: &mut bool,
) -> InputResult {
    let resp_state = ui.response_for(id);
    // Snapshot once before the long `&mut state` borrow below. The
    // menu and the text-edit state live under the same WidgetId but
    // different TypeIds; the borrow checker can't see the disjoint
    // rows so we read the menu row first.
    let menu_open = ContextMenu::is_open(ui, id);

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
        let local_x = ptr.x - rect.min.x - ctx.padding.left() - align_offset.x + state.scroll.x;
        let local_y = ptr.y - rect.min.y - ctx.padding.top() - align_offset.y + state.scroll.y;
        // `byte_at_xy` handles both axes; single-line probes at
        // `y=0` (against an unwrapped layout) collapse to cosmic's
        // 1D `Buffer::hit` walk — one shaped lookup.
        let hit = ui.text.byte_at_xy(
            text,
            local_x,
            if ctx.multiline { local_y } else { 0.0 },
            ctx.font_size,
            ctx.line_height_px,
            ctx.wrap_target,
            ctx.family,
            ctx.halign,
        );
        if !state.prev_pressed {
            // Press rising edge. Detect multi-click: consecutive
            // presses within `MULTI_CLICK_WINDOW` and `MULTI_CLICK_RADIUS`
            // increment `click_count`; otherwise it resets to 1.
            let elapsed = ui.time.saturating_sub(state.last_press_time).as_secs_f32();
            let near = (ptr - state.last_press_pos).length_squared()
                <= MULTI_CLICK_RADIUS * MULTI_CLICK_RADIUS;
            state.click_count = if elapsed < MULTI_CLICK_WINDOW && near {
                state.click_count.saturating_add(1)
            } else {
                1
            };
            state.last_press_time = ui.time;
            state.last_press_pos = ptr;
            state.last_edit_kind = None;
            match state.click_count {
                1 => {
                    state.drag_anchor = Some(hit);
                    state.selection = None;
                    state.caret = hit;
                }
                2 => {
                    // Double-click: select the word under the caret.
                    let r = word_range_at(text, hit);
                    if r.is_empty() {
                        state.drag_anchor = Some(hit);
                        state.selection = None;
                        state.caret = hit;
                    } else {
                        state.drag_anchor = None;
                        state.selection = Some(r.start);
                        state.caret = r.end;
                    }
                }
                _ => {
                    // Triple-click and beyond: select everything.
                    state.drag_anchor = None;
                    state.selection = if text.is_empty() { None } else { Some(0) };
                    state.caret = text.len();
                }
            }
        } else if state.drag_anchor.is_some() {
            // Held drag from a single-click press — caret follows
            // pointer, selection grows from the anchor. Multi-click
            // sequences clear `drag_anchor` so they don't enter this
            // branch and the selection stays locked at the word/all
            // range chosen on the press.
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

    // Drain the unified keyboard event stream in arrival order:
    // Text chunks splice into the buffer (sanitized for single-line);
    // Down events route through `dispatch_shortcut` (clipboard / undo)
    // then `apply_key` (edit / nav). Vertical-nav probes happen inline
    // because they need the shaper + layout. Indexing keeps the borrow
    // on `frame_keyboard_events` short-lived so we can dispatch to
    // `ui.text` inside the same loop without a scratch Vec.
    let mut vert: Option<VerticalMotion> = None;
    let n = ui.input.frame_keyboard_events.len();
    for i in 0..n {
        let ev = ui.input.frame_keyboard_events[i];
        match ev {
            KeyboardEvent::Text(chunk) => {
                let raw = chunk.as_str();
                let to_insert: String = if ctx.multiline {
                    raw.to_string()
                } else {
                    sanitize_single_line(raw)
                };
                if !to_insert.is_empty() {
                    record_edit(text, state, EditKind::Typing);
                    delete_selection(text, state);
                    text.insert_str(state.caret, &to_insert);
                    state.caret += to_insert.len();
                }
            }
            KeyboardEvent::Down(kp) => {
                if dispatch_shortcut(text, state, kp, ctx.multiline, menu_open) {
                    continue;
                }
                if apply_key(text, state, kp, ctx.multiline, &mut vert) {
                    *blur_after = true;
                }
                if let Some(v) = vert.take() {
                    let pos = ui.text.cursor_xy(
                        text,
                        state.caret,
                        ctx.font_size,
                        ctx.line_height_px,
                        ctx.wrap_target,
                        ctx.family,
                        ctx.halign,
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
                            ctx.font_size,
                            ctx.line_height_px,
                            ctx.wrap_target,
                            ctx.family,
                            ctx.halign,
                        )
                    };
                    move_caret(state, target, v.extend);
                }
            }
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

/// Route platform shortcuts (undo / redo / select-all / cut / copy /
/// paste) before keyboard edit dispatch. Returns `true` when `kp` was
/// claimed; caller skips `apply_key` for that key. Undo/redo always
/// fire; clipboard + select-all are suppressed when a context menu
/// owns the same bindings (`menu_open == true`).
fn dispatch_shortcut(
    text: &mut String,
    state: &mut TextEditState,
    kp: KeyPress,
    multiline: bool,
    menu_open: bool,
) -> bool {
    const SELECT_ALL: Shortcut = Shortcut::cmd('A');
    const COPY: Shortcut = Shortcut::cmd('C');
    const CUT: Shortcut = Shortcut::cmd('X');
    const PASTE: Shortcut = Shortcut::cmd('V');
    const UNDO: Shortcut = Shortcut::cmd('Z');
    const REDO: Shortcut = Shortcut::cmd_shift('Z');

    if UNDO.matches(kp) {
        apply_undo(text, state);
        return true;
    }
    if REDO.matches(kp) {
        apply_redo(text, state);
        return true;
    }
    if menu_open {
        return false;
    }
    if SELECT_ALL.matches(kp) {
        if !text.is_empty() {
            state.selection = Some(0);
            state.caret = text.len();
            state.last_edit_kind = None;
        }
        return true;
    }
    if COPY.matches(kp) {
        if let Some(r) = state.sel_range() {
            crate::clipboard::set(&text[r]);
        }
        return true;
    }
    if CUT.matches(kp) {
        cut_selection(text, state);
        return true;
    }
    if PASTE.matches(kp) {
        paste_at_caret(text, state, &crate::clipboard::get(), multiline);
        return true;
    }
    false
}

/// Apply one keypress to the buffer + state. Returns `true` if the
/// caller should blur (Escape with no live selection in single-line
/// mode); every other recognized key is consumed silently. Sets
/// `out_vertical` to `Some(VerticalMotion)` when the key is `Up` /
/// `Down` in multi-line mode — the caller resolves the cross-axis
/// probe against the shaper, which this function doesn't touch.
/// Platform shortcuts (undo / clipboard / select-all) are handled by
/// `dispatch_shortcut` before this; `multiline` toggles Enter → `\n`
/// insertion and enables Up/Down motion.
fn apply_key(
    text: &mut String,
    state: &mut TextEditState,
    kp: KeyPress,
    multiline: bool,
    out_vertical: &mut Option<VerticalMotion>,
) -> bool {
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
                    let prev = prev_grapheme_boundary(text, state.caret);
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
                    let next = next_grapheme_boundary(text, state.caret);
                    text.replace_range(state.caret..next, "");
                }
            }
        }
        Key::ArrowLeft if is_word_nav(kp.mods) => {
            let target = prev_word_boundary(text, state.caret);
            move_caret(state, target, shift);
        }
        Key::ArrowRight if is_word_nav(kp.mods) => {
            let target = next_word_boundary(text, state.caret);
            move_caret(state, target, shift);
        }
        Key::ArrowLeft => {
            let target = if !shift && let Some(r) = state.sel_range() {
                r.start
            } else {
                prev_grapheme_boundary(text, state.caret)
            };
            move_caret(state, target, shift);
        }
        Key::ArrowRight => {
            let target = if !shift && let Some(r) = state.sel_range() {
                r.end
            } else {
                next_grapheme_boundary(text, state.caret)
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

/// Word-nav modifier: Alt (Option) on macOS, Ctrl elsewhere — matches
/// the platform conventions every desktop text field follows. Shift may
/// be held in addition (selection-extending word nav).
fn is_word_nav(m: Modifiers) -> bool {
    if cfg!(target_os = "macos") {
        m.alt && !m.ctrl && !m.meta
    } else {
        m.ctrl && !m.alt && !m.meta
    }
}

/// Next grapheme-cluster boundary strictly after `offset` (clamped to
/// `text.len()`). Walks extended grapheme clusters via
/// [`unicode_segmentation::GraphemeCursor`] so multi-codepoint clusters
/// (combining marks, ZWJ-joined family emoji) advance as one unit.
fn next_grapheme_boundary(text: &str, offset: usize) -> usize {
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
fn prev_grapheme_boundary(text: &str, offset: usize) -> usize {
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
fn next_word_boundary(text: &str, from: usize) -> usize {
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
fn prev_word_boundary(text: &str, from: usize) -> usize {
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
fn word_range_at(text: &str, byte: usize) -> std::ops::Range<usize> {
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

#[cfg(test)]
mod tests;
