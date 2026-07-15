pub(crate) mod model;

use crate::common::clipboard;
use crate::forest::element::{Configure, Element};
use crate::layout::types::layout_mode::LayoutMode;

use crate::forest::tree::paint_anims::PaintAnim;
use crate::input::keyboard::{Key, KeyboardEvent};
use crate::input::sense::Sense;
use crate::layout::types::align::{Align, HAlign};
use crate::layout::types::clip_mode::ClipMode;
use crate::layout::types::sizing::Sizing;
use crate::primitives::approx::noop_f32;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::widget_id::WidgetId;
use crate::shape::{Shape, TextWrap};
use crate::text::{CursorPos, FontFamily, FontWeight, SelectionRects, ShapeParams, text_in_rect};
use crate::ui::Ui;
use crate::widgets::context_menu::{ContextMenu, MenuItem};
use crate::widgets::text_edit::model::{
    COPY, CUT, EditKind, Editor, KeyOutcome, PASTE, TextEditState, word_range_at,
};
use crate::widgets::theme::resolve_look;
use crate::widgets::theme::text_edit::TextEditTheme;
use crate::widgets::{Response, ResponseSnapshot};
use glam::Vec2;
use std::borrow::Cow;
use std::time::Duration;

/// Half-period of the caret blink, in seconds. The caret is visible
/// for `BLINK_HALF` then hidden for `BLINK_HALF`, repeating. Reset
/// to the visible phase on every caret or text change so during
/// active typing the caret stays solid.
const BLINK_HALF: f32 = 0.5;

/// After this long without caret/text/selection change, the blink
/// stops scheduling wakes and the caret stays solid — saves the host
/// a forever 2 Hz repaint loop on a focused-but-idle editor.
const BLINK_STOP_AFTER_IDLE: f32 = 30.0;

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
    weight: FontWeight,
    multiline: bool,
    halign: HAlign,
    /// Alignment offset of the measured text block inside the inner
    /// rect ([`text_in_rect`] against last frame's arranged
    /// rect). `ZERO` before the first arrange — patched onto the ctx
    /// right after the measure that derives it.
    block_offset: Vec2,
}

impl ShapeCtx {
    /// The shaper-facing subset, assembled in one place — every
    /// measure / cursor / hit query must pass identical params or it
    /// reads a different `TextCacheKey` than the rendered buffer.
    fn params(&self) -> ShapeParams {
        ShapeParams {
            font_size_px: self.font_size,
            line_height_px: self.line_height_px,
            max_width_px: self.wrap_target,
            family: self.family,
            weight: self.weight,
            halign: self.halign,
        }
    }
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
    content_w: f32,
) {
    let Some(rect) = response_rect else {
        state.scroll = Vec2::ZERO;
        return;
    };
    let inner_w = (rect.size.w - ctx.padding.horiz()).max(0.0);
    let inner_h = (rect.size.h - ctx.padding.vert()).max(0.0);
    if ctx.multiline {
        state.scroll.x = 0.0;
        // Trailing sliver like the single-line x clamp below — keep
        // the caret's bottom edge strictly inside the scissor so the
        // last line's caret can't lose its bottom pixel to rounding.
        let trailing = (inner_h - caret_width).max(0.0);
        let caret_bottom = caret_pos.y_top + caret_pos.line_height;
        if caret_pos.y_top < state.scroll.y {
            state.scroll.y = caret_pos.y_top;
        } else if caret_bottom > state.scroll.y + trailing {
            state.scroll.y = caret_bottom - trailing;
        }
        state.scroll.y = state.scroll.y.max(0.0);
    } else {
        state.scroll.y = 0.0;
        // Keep a caret-width sliver between the caret's right edge
        // and the scissor's right edge — otherwise the caret's last
        // pixel can land flush on the scissor boundary and get
        // clipped under sub-pixel rounding. Mirrors the
        // `inner_w - caret_room` reduction the multi-line wrap target
        // applies for the same reason.
        let trailing = (inner_w - caret_width).max(0.0);
        let caret_right = caret_pos.x + caret_width;
        if caret_pos.x < state.scroll.x {
            state.scroll.x = caret_pos.x;
        } else if caret_right > state.scroll.x + trailing {
            state.scroll.x = caret_right - trailing;
        }
        // Never scroll past what's needed to reveal the text's end (plus
        // the trailing caret sliver). Without this upper clamp a buffer
        // scrolled left while narrow — e.g. a `Hug` editor lagging one
        // frame behind its own growth as you type — stays scrolled even
        // after it widens enough to fit everything, permanently clipping
        // the leading glyphs. `response_rect` is one frame stale, so the
        // clamp is what makes the scroll settle back to 0 next frame.
        // `2·caret_width`: the caret quad past the text end, plus the
        // trailing sliver the clamp above reserves — matches the scroll
        // that branch sets when the caret is at end-of-text, so this only
        // ever trims *excess* scroll, never the legitimate end position.
        let max_scroll = (content_w + 2.0 * caret_width - inner_w).max(0.0);
        state.scroll.x = state.scroll.x.clamp(0.0, max_scroll);
    }
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
#[derive(Debug)]
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
    /// Max characters (Unicode scalar values) the buffer may hold.
    /// `None` = unbounded. Enforced at every insertion path (typing,
    /// IME/text, paste, newline): input that would overflow is dropped.
    max_chars: Option<usize>,
    /// Select the whole buffer when the field gains focus without a
    /// same-frame press (e.g. focus handed off programmatically, as
    /// [`crate::DragValue`] does on click-to-edit) so the first keystroke
    /// replaces it. A press that focuses the field still places the caret.
    select_all_on_focus: bool,
}

impl<'a> TextEdit<'a> {
    #[track_caller]
    pub fn new(text: &'a mut String) -> Self {
        let mut element = Element::new(LayoutMode::Leaf);
        element.flags.set_sense(Sense::CLICK);
        element.flags.set_focusable(true);
        // Clip glyphs, caret, and selection wash to the editor's own
        // rect so a `Fixed`-sized editor with long content doesn't
        // bleed over its neighbours. Chrome (background) draws before
        // the clip, so the editor's surround still paints normally.
        element.flags.set_clip(ClipMode::Rect);
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
            max_chars: None,
            select_all_on_focus: false,
        }
    }

    /// Select the whole buffer the moment the field gains focus without a
    /// same-frame pointer press — so a value handed to it (via `request_focus`)
    /// is replaced by the first keystroke. Clicking into the field still
    /// places the caret at the hit. Default off.
    pub fn select_all_on_focus(mut self) -> Self {
        self.select_all_on_focus = true;
        self
    }

    /// Cap the buffer at `n` characters. Insertions are truncated to
    /// what fits; content already longer than `n` is left alone (the
    /// cap only gates growth). `n == 0` makes the field read-only.
    pub fn max_chars(mut self, n: usize) -> Self {
        self.max_chars = Some(n);
        self
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

    pub fn show(mut self, ui: &mut Ui) -> TextEditResponse<'_> {
        let id = ui.widget_id(&self.element);
        let mut is_focused = ui.input.focused == Some(id);
        // Pick the per-state look + animate its visual components.
        // Disabled wins over focus — a disabled editor that still
        // happens to hold focus paints with its disabled visuals
        // (mirrors Button). State.disabled comes from the cascade
        // (one-frame stale); OR self-disabled in for lag-free
        // response to a freshly toggled `.disabled(true)`.
        let mut response = ui.response_for(id);
        response.disabled |= self.element.flags.is_disabled();
        // A disabled editor must not keep keyboard focus — it would
        // paint disabled while still routing typing / paste / undo
        // into the host's buffer. Kick focus out (mirrors `DragValue`'s
        // click-to-edit path) and run this frame unfocused, so the
        // same frame's keystrokes are dropped and no caret paints.
        if is_focused && response.disabled {
            ui.request_focus(None);
            is_focused = false;
        }
        // `resolve_look` also substitutes theme padding/margin where
        // the builder left the `Spacing::ZERO` sentinel. The renderer
        // reads `element.padding` to deflate the buffer layout, and
        // the caret hit-test reads it back below — both see the
        // resolved value.
        let look = resolve_look(
            ui,
            id,
            &mut self.element,
            response,
            self.style.as_ref(),
            |t| &t.text_edit,
        );
        // State-independent scalars off the same style source, copied
        // out so no theme borrow (or whole-theme clone) survives.
        let style = self.style.as_ref().unwrap_or(&ui.theme.text_edit);
        let caret_color = style.caret;
        let caret_width = style.caret_width;
        let selection_color = style.selection;
        let placeholder_color = style.placeholder;
        let font_size = look.text.font_size_px;
        let line_height_mult = look.text.line_height_mult;
        // `Tree::open_node` folds chrome stroke width into the stored
        // padding so children sit inside the painted stroke ring (see
        // `forest/tree/mod.rs::open_node`). Encoder's clip mask is
        // `rect.deflated_by(post-inflate padding)`, so glyph + caret
        // coordinates must use the same effective value — otherwise
        // the top row of glyphs sits above the clip and gets scissored
        // away. The element's own padding stays at the pre-inflate
        // value so Tree's fold reproduces the same effective padding.
        let stroke_w = if noop_f32(look.background.stroke.width) {
            0.0
        } else {
            look.background.stroke.width
        };
        let padding = Spacing::from_array(self.element.padding.as_array().map(|v| v + stroke_w));
        // Reserve a caret-width sliver at the trailing edge of every
        // line so a caret sitting at end-of-line on right/center-
        // aligned text stays inside the clip. The shaper's per-line
        // halign and the widget's single-line `text_in_rect` both see
        // the same reduced width, so glyphs + caret + selection wash
        // shift together and click hit-test (which reads back the
        // same `text_in_rect`) stays consistent.
        let caret_room = caret_width.max(0.0);

        // Wrap target for multi-line: editor's inner width (outer −
        // padding − caret room). Read from the previous arrange via
        // `response.layout_rect` — cascade runs in `post_record` so the
        // value is up-to-date both in steady state and across
        // `request_relayout` passes. `None` on the first frame the
        // widget is recorded; cosmic then lays out unbounded (single
        // visual line per `\n` chunk) until the next frame catches up.
        // **Must be `layout_rect`, not `rect`** — text shapes and the
        // shaper's measured sizes are in logical (pre-transform) units;
        // under an ancestor `Panel::transform` zoom, `rect.size`
        // includes the scale factor and would inflate the wrap target,
        // drifting the cached buffer's `TextCacheKey` off the one the
        // widget queries via `cursor_xy` / `selection_rects`.
        let wrap_target: Option<f32> = if self.multiline {
            response
                .layout_rect
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
        let mut ctx = ShapeCtx {
            font_size,
            line_height_px: font_size * line_height_mult,
            padding,
            wrap_target,
            family: look.text.family,
            weight: look.text.weight,
            multiline: self.multiline,
            halign: text_align.halign(),
            block_offset: Vec2::ZERO,
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
        // `layout_rect` (pre-transform) — `text_in_rect` math has to
        // stay in the same units as `measured` (logical, from the
        // shaper). Reading `rect` would mix post-transform widget
        // height with logical text height and drift the vertical center
        // by `(scale - 1) * line_height / 2` under any ancestor zoom.
        // Reused below by `update_scroll` to cap the single-line scroll
        // at the text's end (set from the same measure as the align
        // offset, so there's only one shaping call per frame). Stays 0
        // until the first arrange and on the multi-line path, where x
        // never scrolls.
        let mut content_w = 0.0_f32;
        let offset = if let Some(r) = response.layout_rect {
            let measure_str: &str = if !self.text.is_empty() || is_focused {
                self.text
            } else {
                &self.placeholder
            };
            let m = ui.ctx.shaper.measure(measure_str, ctx.params()).size;
            content_w = m.w;
            let measured = Size::new(m.w, m.h.max(ctx.line_height_px));
            let inner_w = (r.size.w - ctx.padding.horiz() - caret_room).max(0.0);
            let inner_h = (r.size.h - ctx.padding.vert()).max(0.0);
            text_in_rect(
                Rect::new(0.0, 0.0, inner_w, inner_h),
                measured,
                widget_align,
            )
            .min
        } else {
            Vec2::ZERO
        };
        ctx.block_offset = offset;

        // Phase 1: input handling. Touches `ui.state` and `ui.input`
        // (separate fields, disjoint borrows). Click-to-place-caret
        // happens here too, before keystroke processing, so a click +
        // type in the same frame inserts at the click point.
        // `edited` comes from the mutation choke points (`record_edit`,
        // undo/redo), so a same-length overwrite (select "a", type "b")
        // still reports. The context menu can also mutate the buffer —
        // Phase 5 ORs its result into `changed`. Focus edges read
        // against `was_focused` (this frame's `prev_focused`, snapshot
        // before Phase 2 rewrites it).
        let InputResult {
            caret: caret_byte,
            selection,
            caret_moved,
            was_focused,
            blur: blur_after,
            submitted,
            edited,
        } = handle_input(
            ui,
            id,
            is_focused,
            self.text,
            &ctx,
            self.max_chars,
            self.select_all_on_focus,
        );
        let mut changed = edited;
        let gained_focus = is_focused && !was_focused;
        let lost_focus = was_focused && !is_focused;

        // Phase 2: scroll-to-caret + blink-phase reset. One `state`
        // borrow covers `update_scroll` mutating `state.scroll` and
        // snapshotting `last_caret_change` for the visibility calc
        // below. `caret_pos` is computed via the shaper (disjoint
        // field) first so the state borrow is contiguous.
        let caret_pos = ui.ctx.shaper.cursor_xy(self.text, caret_byte, ctx.params());
        let now = ui.frame_runtime.time;
        let (scroll, last_caret_change) = {
            let state = ui.state_mut::<TextEditState>(id);
            update_scroll(
                state,
                response.layout_rect,
                &ctx,
                caret_pos,
                caret_width,
                content_w,
            );
            if is_focused && (caret_moved || edited || gained_focus) {
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
        // an unwrapped layout with one visual run. Touch `ui.ctx.shaper`
        // (disjoint from `ui.forest`, so `add_shape` sequences fine).
        // Chrome paints via `Tree::chrome_for` — encoder emits it
        // before any clip. Every shape's local_rect is shifted by
        // `-scroll` so the caret/text/selection wash track the
        // visible viewport; the editor's `ClipMode::Rect` (set in
        // `new()`) scissors anything that slips past the edge.
        //
        // Floor the editor's outer height at one shaped line plus
        // top+bottom padding so a `Sizing::Hug` editor with an empty
        // buffer still reserves a row's worth of space — without this
        // floor, Hug resolves to `0` (no content) and the editor
        // visually collapses, taking its clip rect (and any future
        // caret/text painted into it) with it. Single-line only;
        // multi-line callers usually set their own min_size and the
        // wrap target already gives them height per line.
        let mut element = self.element;
        if !self.multiline {
            let row_min_h = ctx.line_height_px + ctx.padding.vert();
            if element.min_size.h < row_min_h {
                element.min_size.h = row_min_h;
            }
            // A `Hug`-width single-line editor sizes to the glyph bbox,
            // but `update_scroll` keeps a caret-width sliver past the
            // text's end (so the end-of-line caret can't land on the
            // scissor boundary). With zero slack the end-of-text caret
            // scrolls the buffer left and clips the leading glyphs, so a
            // content-hugging field can never show its own full text.
            // Fold that reservation (the trailing sliver + the caret quad
            // itself) into the desired width so Hug accounts for it.
            // `Fixed`/`Fill` editors are meant to scroll, so leave them.
            if matches!(element.size.w(), Sizing::Hug) {
                let measure_str: &str = if self.text.is_empty() {
                    self.placeholder.as_ref()
                } else {
                    self.text.as_str()
                };
                // `ctx.params()` measures unbounded here — single-line,
                // so `wrap_target` is `None` by construction.
                let reserve_w = ui.ctx.shaper.measure(measure_str, ctx.params()).size.w;
                let reserved = reserve_w + ctx.padding.horiz() + 2.0 * caret_room;
                if element.min_size.w < reserved {
                    element.min_size.w = reserved;
                }
            }
        }
        let chrome = look.background;
        let placeholder = self.placeholder;
        let text_ptr = &*self.text;
        ui.node(id, element, Some(&chrome), |ui| {
            let [pad_l, pad_t, _, _] = ctx.padding.as_array();
            // Selection highlight, painted *before* the text so glyphs
            // sit on top of the wash. Only when focused and a range is
            // actually live (anchor != caret — collapsed selections are
            // stored as `None`, so any `Some` here has positive width).
            if is_focused && let Some(range) = selection {
                // Materialize selection rects via the shaper's out-arg
                // form, then release the `ui.ctx.shaper` borrow before
                // painting through the public `ui.add_shape` API.
                let sel_color = selection_color;
                let mut rects = SelectionRects::new();
                ui.ctx
                    .shaper
                    .selection_rects(text_ptr, range, ctx.params(), &mut rects);
                let delta = Vec2::new(pad_l + offset.x - scroll.x, pad_t + offset.y - scroll.y);
                for r in rects {
                    ui.add_shape(
                        Shape::rect(Rect {
                            min: r.min + delta,
                            size: r.size,
                        })
                        .fill(sel_color),
                    );
                }
            }

            // Text or placeholder. Empty buffer always renders the
            // placeholder — focused or not — so the leaf's
            // content-driven desired width stays stable across focus
            // transitions; a Hug parent (or any non-stretching parent)
            // would otherwise see the editor's width snap between
            // placeholder-width and zero on every focus change. The
            // caret position comes from `cursor_xy(self.text, ...)` on
            // the *buffer* (not the recorded shape), so a focused empty
            // editor still gets a caret at column 0 even though the
            // placeholder text sits behind it. `local_rect: Some(...)`
            // positions the shaped text at owner-local `(padding −
            // scroll)`; the size is unused under `Align::Auto` (text
            // origin sits at `leaf.min` and the painted extent is the
            // shaped glyph bbox).
            let (display, color) = if text_ptr.is_empty() {
                (placeholder.clone().into(), placeholder_color)
            } else {
                // Intern the live buffer into the retained frame arena
                // (a memcpy into `fmt_scratch`, not a per-frame `String`
                // allocation that scales with buffer length).
                (ui.intern(text_ptr), look.text.color)
            };
            if !display.is_empty() {
                ui.add_shape(Shape::Text {
                    local_origin: Some(Vec2::new(
                        pad_l + offset.x - scroll.x,
                        pad_t + offset.y - scroll.y,
                    )),
                    text: display,
                    brush: color.into(),
                    font_size_px: ctx.font_size,
                    line_height_px: ctx.line_height_px,
                    wrap: if ctx.multiline {
                        TextWrap::WrapWithOverflow
                    } else {
                        // Editable single line: the editor clips (`ClipMode::Rect`)
                        // and runs its own horizontal scroll, so `Scroll` reports
                        // zero min-content — a Fill/Hug field shrinks below its text
                        // instead of freezing at the buffer's natural width — while
                        // still shaping the whole buffer unbounded for caret/scroll.
                        TextWrap::Scroll
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
                    weight: ctx.weight,
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
                    pad_l + offset.x + caret_pos.x - scroll.x,
                    pad_t + offset.y + caret_pos.y_top - scroll.y,
                    caret_width,
                    caret_pos.line_height,
                );
                let shape = Shape::rect(caret_rect).fill(caret_color);
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

        // Re-read `response_for(id)` after Phase 4's
        // `request_focus(None)` blur — that's the only state
        // mutation between the theme-pick read at L475 and here,
        // and it would otherwise leak a stale `focused` bit into
        // the returned `Response`. Built as an owned snapshot so
        // the Phase-5 context-menu work below can keep using
        // `&mut ui` freely; `state` itself is `Copy` and survives
        // for the final `Response::eager` build at the bottom.
        let state = ui.response_for(id);
        let snapshot = ResponseSnapshot { id, state };

        // Phase 5: the default Cut / Copy / Paste / Clear context menu, opened
        // by secondary-click and mutating the buffer through the same borrow.
        // Menu actions edit the buffer after Phase 1 captured `input.edited`,
        // so fold their result into `changed` here.
        changed |=
            default_context_menu(ui, id, &snapshot, self.text, ctx.multiline, self.max_chars);

        // Eager Response build last — all `&mut ui` ops above are
        // done. Caller inherits the cached state without a re-probe. The
        // edit-specific signals were captured up in Phase 1.
        TextEditResponse {
            response: Response::eager(id, ui, state),
            changed,
            submitted,
            gained_focus,
            lost_focus,
        }
    }
}

impl Configure for TextEdit<'_> {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}

/// The editor's default Cut / Copy / Paste / Clear context menu, opened by
/// secondary-click. Items mutate the host's `&mut String` through the same
/// borrow `show` holds and sync `TextEditState` caret/selection for the next
/// frame. Returns whether a menu action mutated the buffer this frame.
/// Clipboard liveness is probed **inside** the closure — `ContextMenu`
/// early-returns when closed, so a closed menu makes no clipboard syscall
/// (`arboard` → `NSPasteboardItem`) on the common path. `Cut`/`Copy` gate on a
/// live selection; `Paste` on a non-empty clipboard; `Clear` on a non-empty
/// buffer.
fn default_context_menu(
    ui: &mut Ui,
    id: WidgetId,
    snapshot: &ResponseSnapshot,
    text: &mut String,
    multiline: bool,
    max_chars: Option<usize>,
) -> bool {
    #[derive(Clone, Copy)]
    enum MenuAction {
        Cut,
        Copy,
        Paste,
        Clear,
    }
    let has_sel = ui.state_mut::<TextEditState>(id).sel_range().is_some();
    let has_text = !text.is_empty();
    let mut action = None;
    ContextMenu::attach(ui, snapshot).show(ui, |ui, popup| {
        if MenuItem::new("Cut")
            .shortcut(CUT)
            .enabled(has_sel)
            .show(ui, popup)
            .left
            .clicked()
        {
            action = Some(MenuAction::Cut);
        }
        if MenuItem::new("Copy")
            .shortcut(COPY)
            .enabled(has_sel)
            .show(ui, popup)
            .left
            .clicked()
        {
            action = Some(MenuAction::Copy);
        }
        let cb_has = !clipboard::get().is_empty();
        if MenuItem::new("Paste")
            .shortcut(PASTE)
            .enabled(cb_has)
            .show(ui, popup)
            .left
            .clicked()
        {
            action = Some(MenuAction::Paste);
        }
        MenuItem::separator(ui);
        if MenuItem::new("Clear")
            .enabled(has_text)
            .show(ui, popup)
            .left
            .clicked()
        {
            action = Some(MenuAction::Clear);
        }
    });
    let Some(action) = action else {
        return false;
    };
    let mut ed = Editor::new(
        text,
        ui.state_mut::<TextEditState>(id),
        multiline,
        max_chars,
    );
    match action {
        MenuAction::Cut => ed.cut(),
        MenuAction::Copy => ed.copy(),
        MenuAction::Paste => ed.paste(&clipboard::get()),
        MenuAction::Clear => ed.clear(),
    }
    ed.edited
}

/// What [`TextEdit::show`] returns: the widget's [`Response`] (pointer / click /
/// hover — reachable directly via `Deref`) plus the edit-specific signals
/// computed *inside* `show()`. Callers read commit/focus state from here instead
/// of re-polling `ui` for focus and key presses, which is both terser and
/// authoritative (the editor knows what it did with the input this frame).
#[derive(Debug)]
pub struct TextEditResponse<'a> {
    /// The widget's pointer/click/hover [`Response`]. Also reachable through
    /// `Deref`, so `resp.left.clicked()` resolves here; use the field when you need
    /// the `Response` itself (`&resp.response`).
    pub response: Response<'a>,
    /// The buffer was edited this frame (characters inserted or removed).
    pub changed: bool,
    /// The user pressed Enter in a single-line editor — the conventional
    /// "accept" signal. Always `false` in multi-line mode (Enter inserts `\n`).
    pub submitted: bool,
    /// The editor took focus this frame.
    pub gained_focus: bool,
    /// The editor lost focus this frame (clicked away, another widget focused,
    /// or Escape) — the conventional "commit on blur" signal.
    pub lost_focus: bool,
}

impl<'a> std::ops::Deref for TextEditResponse<'a> {
    type Target = Response<'a>;
    fn deref(&self) -> &Self::Target {
        &self.response
    }
}

/// Result of one frame's input pass over a TextEdit: the caret byte,
/// the (sorted) selection range for the painter, and the edge signals
/// `show()` folds into [`TextEditResponse`].
struct InputResult {
    caret: usize,
    selection: Option<std::ops::Range<usize>>,
    /// Caret or selection differ from their pre-input values (compared
    /// against the pre-clamp snapshot, so an external buffer shrink
    /// that displaces the caret also reads as motion) — drives the
    /// blink-phase reset.
    caret_moved: bool,
    /// The state row's `prev_focused` before this pass — `show()`
    /// derives the gained/lost focus edges from it; Phase 2 rewrites
    /// the row afterwards.
    was_focused: bool,
    /// Escape asked to blur — applied by `show()` after the node closes.
    blur: bool,
    /// Enter accepted a single-line value this frame.
    submitted: bool,
    /// The buffer was mutated this frame (typing, delete, paste, cut,
    /// undo/redo). Reported by the mutation choke points, so it's
    /// content-accurate — a same-length overwrite still counts, unlike
    /// a length-delta proxy.
    edited: bool,
}

/// Process this frame's pointer + keyboard input for one TextEdit
/// widget and return the caret + selection to render plus the frame's
/// edge signals. Splitting this out of `show()` keeps the borrow
/// choreography contained: we touch `ui.state`, `ui.input`, and
/// `ui.ctx.shaper` here, but never the shape/tree storage.
fn handle_input(
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

#[cfg(test)]
mod tests;
