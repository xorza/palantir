use crate::forest::element::{Configure, Element, LayoutMode, Salt};
use crate::input::ResponseState;
use crate::input::sense::Sense;
use crate::layout::axis::Axis;
use crate::layout::scroll::ScrollLayoutState;
use crate::layout::types::clip_mode::ClipMode;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::corners::Corners;
use crate::primitives::rect::Rect;
use crate::primitives::shadow::Shadow;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::stroke::Stroke;
use crate::primitives::transform::TranslateScale;
use crate::primitives::widget_id::WidgetId;
use crate::ui::Ui;
use crate::widgets::Response;
use crate::widgets::theme::scrollbar::ScrollbarTheme;
use glam::Vec2;
use std::ops::RangeInclusive;

/// What kind of input triggers a zoom step. See [`ZoomConfig::modifier`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ZoomModifier {
    /// Hold `Ctrl` and turn the wheel. Default. Bare wheel pans as
    /// today. Ctrl is the zoom modifier on every platform (macOS Cmd
    /// is not honored — matches the shortcut layer).
    Ctrl,
    /// Plain wheel always zooms (rare; for image viewers without pan).
    Always,
    /// Wheel always pans; only pinch gestures zoom. Touch-first apps.
    PinchOnly,
}

/// Where the zoom step pivots — the point that stays fixed across the
/// scale change.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ZoomPivot {
    /// Pointer position (in widget-local coords). Default — the point
    /// under the cursor stays put across the zoom step.
    Pointer,
    /// Viewport center.
    Center,
}

/// Per-widget zoom configuration. Attach to a `Scroll::both` via
/// [`Scroll::with_zoom`] / [`Scroll::with_zoom_config`].
#[derive(Clone, Debug)]
pub struct ZoomConfig {
    /// Inclusive `[min, max]` zoom range. Default `0.1..=10.0`.
    pub range: RangeInclusive<f32>,
    /// Multiplicative factor per wheel notch; `step.powf(notches)`.
    /// Default `1.1` (10% per notch).
    pub step: f32,
    /// Wheel-vs-pinch routing. Default [`ZoomModifier::Ctrl`].
    pub modifier: ZoomModifier,
    /// Where the zoom step pivots. Default [`ZoomPivot::Pointer`].
    pub pivot: ZoomPivot,
}

impl Default for ZoomConfig {
    fn default() -> Self {
        Self {
            range: 0.1..=10.0,
            step: 1.03,
            modifier: ZoomModifier::Ctrl,
            pivot: ZoomPivot::Pointer,
        }
    }
}

// `ScrollLayoutState` lives on `LayoutEngine::scroll_states` rather
// than `StateMap` — it's a layout-derived concern, the scroll driver
// writes the layout fields during measure + arrange, and the widget
// reads/mutates the row at record time via [`Ui::scroll_state`].
//
// Bar drawing + reservation logic stay here as widget concerns; the
// layout primitive itself is unaware of scrollbars.

// ---------------------------------------------------------------------------
// Bar geometry helpers
// ---------------------------------------------------------------------------

/// Cross-axis space stolen from children when an axis's bar is shown:
/// the bar's `width` plus a `gap` strip so the bar doesn't touch the
/// visible content. Returns 0 when the axis isn't panned.
#[inline]
fn bar_reservation(panned: bool, theme: &ScrollbarTheme) -> f32 {
    if panned { theme.width + theme.gap } else { 0.0 }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct BarGeometry {
    pub(crate) thumb_size: f32,
    pub(crate) thumb_offset: f32,
}

/// Compute thumb size/offset along the bar's main axis. Returns `None`
/// when the bar can't be drawn meaningfully (zero/negative track or no
/// scrollable range).
pub(crate) fn bar_geometry(
    viewport: f32,
    content: f32,
    offset: f32,
    track_len: f32,
    theme: &ScrollbarTheme,
) -> Option<BarGeometry> {
    if track_len <= 0.0 || content <= viewport {
        return None;
    }
    let raw = viewport / content * track_len;
    let thumb_size = raw.max(theme.min_thumb_px).min(track_len);
    let max_off = (content - viewport).max(f32::EPSILON);
    let travel = (track_len - thumb_size).max(0.0);
    let thumb_offset = (offset / max_off).clamp(0.0, 1.0) * travel;
    Some(BarGeometry {
        thumb_size,
        thumb_offset,
    })
}

/// Offset-independent bar layout derived from a scroll state row:
/// the cross-axis gutter reservations, the post-zoom content extent,
/// and the bar's main-axis length (= `outer − reservation − user
/// padding`). Both drag math and the renderer derive their bar
/// geometry from this — the only difference is which `offset` they
/// feed in to position the thumb.
#[derive(Copy, Clone, Debug)]
struct BarLayout {
    scaled_content: Size,
    bar_viewport: Size,
    reserve_y: f32,
    reserve_x: f32,
}

fn bar_layout(
    row: &ScrollLayoutState,
    pan: glam::BVec2,
    user_padding: Spacing,
    theme: &ScrollbarTheme,
    bar_mode: BarMode,
) -> BarLayout {
    let scaled_content = Size::new(row.content.w * row.zoom, row.content.h * row.zoom);
    // Only `Reserved` reserves the gutter on the pan axes. `Overlay`
    // paints the bar over content without reservation; `Hidden` has
    // no bar at all. Reservation is constant for `Reserved` (not
    // toggled by overflow) so a Hug ancestor doesn't shift between
    // frames; the bar thumb itself is still drawn conditionally on
    // `content > viewport` via `bar_plan`.
    let reserve = matches!(bar_mode, BarMode::Reserved);
    let reserve_y = bar_reservation(pan.y && reserve, theme);
    let reserve_x = bar_reservation(pan.x && reserve, theme);
    let bar_viewport = Size::new(
        (row.outer.w - reserve_y - user_padding.horiz()).max(0.0),
        (row.outer.h - reserve_x - user_padding.vert()).max(0.0),
    );
    BarLayout {
        scaled_content,
        bar_viewport,
        reserve_y,
        reserve_x,
    }
}

/// Per-axis bar plan: rendered rects for the track + thumb (both in
/// OUTER-local coords, so they land in the reserved gutter even when
/// the viewport has user padding). Built from the *post-drag* offset
/// so the visible thumb tracks the cursor 1:1.
#[derive(Copy, Clone, Debug)]
struct BarPlan {
    track_rect: Rect,
    thumb_rect: Rect,
}

fn bar_plan(
    bar_viewport: Size,
    outer: Size,
    content: Size,
    offset: Vec2,
    axis: Axis,
    panned: bool,
    theme: &ScrollbarTheme,
) -> Option<BarPlan> {
    if !panned {
        return None;
    }
    let main = axis.main(bar_viewport);
    let cross_outer = axis.cross(outer);
    let main_content = axis.main(content);
    let main_offset = axis.main_v(offset);
    let geom = bar_geometry(main, main_content, main_offset, main, theme)?;
    let cross_pos = cross_outer - theme.width;
    let track_rect = axis.compose_rect(0.0, cross_pos, main, theme.width);
    let thumb_rect = axis.compose_rect(geom.thumb_offset, cross_pos, geom.thumb_size, theme.width);
    Some(BarPlan {
        track_rect,
        thumb_rect,
    })
}

/// Emit one bar's worth of nodes onto the overlay Canvas: a track
/// leaf with `Sense::CLICK` (paging on press) and a thumb leaf with
/// `Sense::DRAG` painted on top. Both expressed in OUTER-local coords;
/// the overlay covers outer's full rect so position + local_rect line
/// up. Track is always a leaf even when `theme.track` alpha is 0 so
/// the click-to-page surface stays available — the gutter is reserved
/// either way and matches OS scrollbar conventions.
fn push_bar_nodes(
    ui: &mut Ui,
    plan: BarPlan,
    track_id: WidgetId,
    thumb_id: WidgetId,
    resp: ResponseState,
    theme: &ScrollbarTheme,
) {
    let radius = Corners::all(theme.radius);
    let mut track = Element::new(LayoutMode::Leaf);
    track.salt = Salt::Verbatim(track_id);
    track.size = (
        Sizing::Fixed(plan.track_rect.size.w),
        Sizing::Fixed(plan.track_rect.size.h),
    )
        .into();
    track.position = plan.track_rect.min;
    track.flags.set_sense(Sense::CLICK);
    if !theme.track.is_noop() {
        let chrome = Background {
            fill: theme.track.into(),
            stroke: Stroke::ZERO,
            corners: radius,
            shadow: Shadow::NONE,
        };
        ui.node(track_id, track, Some(&chrome), |_| {});
    } else {
        ui.node(track_id, track, None, |_| {});
    }

    let fill = if resp.drag_delta().is_some() || resp.pressed {
        theme.thumb_active
    } else if resp.hovered {
        theme.thumb_hover
    } else {
        theme.thumb
    };
    let mut thumb = Element::new(LayoutMode::Leaf);
    thumb.salt = Salt::Verbatim(thumb_id);
    thumb.size = (
        Sizing::Fixed(plan.thumb_rect.size.w),
        Sizing::Fixed(plan.thumb_rect.size.h),
    )
        .into();
    thumb.position = plan.thumb_rect.min;
    thumb.flags.set_sense(Sense::DRAG);
    let chrome = Background {
        fill: fill.into(),
        stroke: Stroke::ZERO,
        corners: radius,
        shadow: Shadow::NONE,
    };
    ui.node(thumb_id, thumb, Some(&chrome), |_| {});
}

// ---------------------------------------------------------------------------
// Scroll widget
// ---------------------------------------------------------------------------

/// How the scrollbars relate to the content area on the pan axes.
///
/// - [`Self::Reserved`] (default): the gutter always takes a strip of
///   the cross axis (`theme.scrollbar.width + gap`), and the bar is
///   drawn inside that gutter only when content overflows. The
///   reserved width is constant whether or not anything currently
///   overflows — so a Hug ancestor of the scroll doesn't shift when
///   overflow toggles.
/// - [`Self::Overlay`]: no gutter is reserved. The content gets the
///   full inner width, and the bar paints **over** the content's
///   far-edge strip when overflow happens. Modern macOS-style scroll
///   indicator behaviour.
/// - [`Self::Hidden`]: no bar, no gutter. Wheel / touchpad / drag
///   input still pans. Useful for canvas-style scopes (node graphs,
///   infinite boards) where indicators would be noise.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum BarMode {
    #[default]
    Reserved,
    Overlay,
    Hidden,
}

/// The two wrapper `Element`s a `Scroll` records: an outer `ZStack`
/// that owns sizing / placement / sense / visibility and an inner
/// viewport that owns the `Scroll` layout mode, padding, and the panel
/// knobs (gap / justify / child_align).
struct ScrollWrappers {
    outer: Element,
    inner: Element,
}

/// Split a user `Scroll` element into its outer/inner wrappers.
///
/// **This routes every `Element` field that should survive on a
/// `Scroll`** — adding a field means deciding whether it lands on
/// `outer` (sizing/placement) or `inner` (layout/panel knobs);
/// forgetting it drops the field silently on `Scroll` with no compile
/// error. `Scroll::show` patches the remaining inner fields it computes
/// per frame (`salt`, the reservation `margin`, `mode_payload` fit bits,
/// `clip`, and the pan `transform`).
fn scroll_wrappers(element: Element) -> ScrollWrappers {
    let mut outer = Element::new(LayoutMode::ZStack);
    outer.salt = element.salt;
    outer.size = element.size;
    outer.min_size = element.min_size;
    outer.max_size = element.max_size;
    outer.margin = element.margin;
    outer.align = element.align;
    outer.position = element.position;
    outer.grid = element.grid;
    outer.flags.set_sense(element.flags.sense());
    outer.flags.set_disabled(element.flags.is_disabled());
    outer.flags.set_focusable(element.flags.is_focusable());
    outer.visibility = element.visibility;

    let mut inner = Element::new(element.mode);
    inner.mode_payload = element.mode_payload;
    // Inner fills the outer wrapper; the outer carries the user's
    // `Sizing` and drives the actual size.
    inner.size = (Sizing::FILL, Sizing::FILL).into();
    inner.padding = element.padding;
    inner.gaps = element.gaps;
    inner.justify = element.justify;
    inner.child_align = element.child_align;
    ScrollWrappers { outer, inner }
}

/// Scroll viewport. Three flavors via constructor:
/// - [`Scroll::vertical`]: pans on Y, lays children out as a `VStack`.
/// - [`Scroll::horizontal`]: pans on X, lays children out as an
///   `HStack`.
/// - [`Scroll::both`]: pans on both axes, lays children out as a
///   `ZStack` measured with both axes unbounded.
///
/// All three measure the panned axes as `INF` so children report
/// their full natural extent; the viewport itself takes whatever its
/// parent gave it. Wheel / touchpad input over the viewport pans
/// children via a `transform` applied at record time using the
/// previous frame's clamp. The scrollbar's relationship to the
/// content area — reserved gutter, overlay, or hidden — is selected
/// via [`BarMode`].
pub struct Scroll {
    element: Element,
    zoom: Option<ZoomConfig>,
    chrome: Option<Background>,
    bar_mode: BarMode,
    content_margin: Spacing,
}

impl Scroll {
    #[track_caller]
    pub fn vertical() -> Self {
        Self::with_axes(LayoutMode::SCROLL_PAN_Y)
    }

    #[track_caller]
    pub fn horizontal() -> Self {
        Self::with_axes(LayoutMode::SCROLL_PAN_X)
    }

    #[track_caller]
    pub fn both() -> Self {
        Self::with_axes(LayoutMode::SCROLL_PAN_X | LayoutMode::SCROLL_PAN_Y)
    }

    #[track_caller]
    fn with_axes(pan_mask: u16) -> Self {
        let mut element = Element::new(LayoutMode::Scroll);
        element.mode_payload = pan_mask;
        // Both bits: `SCROLL` for pan, `PINCH` for touchpad zoom.
        // Zoom is gated again at consumption time by
        // `self.zoom.is_some()`, but the routing has to be on
        // regardless so the pinch factor reaches us in the first
        // place. Cheap — one bit on the sense flags.
        element.flags.set_sense(Sense::SCROLL | Sense::PINCH);
        // Scroll requires clipping; default to `Rect` so callers that
        // don't override get the cheap scissor path. Callers can still
        // call `Configure::clip_rounded` to upgrade to a stencil mask.
        element.flags.set_clip(ClipMode::Rect);
        Self {
            element,
            zoom: None,
            chrome: None,
            bar_mode: BarMode::Reserved,
            content_margin: Spacing::default(),
        }
    }

    /// Set the scrollbar layout mode. See [`BarMode`].
    pub fn bar_mode(mut self, mode: BarMode) -> Self {
        self.bar_mode = mode;
        self
    }

    /// Sugar for `bar_mode(BarMode::Overlay)` — bar paints over
    /// content when overflowing, no gutter reservation.
    pub fn overlay_bars(self) -> Self {
        self.bar_mode(BarMode::Overlay)
    }

    /// Sugar for `bar_mode(BarMode::Hidden)` — no track, no thumb, no
    /// cross-axis reservation. Pan/wheel/zoom input still work; the
    /// viewport just doesn't paint indicators. Useful for canvas-style
    /// scopes (node graphs, infinite boards) where the bars would be
    /// noise.
    pub fn hide_bars(mut self) -> Self {
        self.bar_mode = BarMode::Hidden;
        self
    }

    /// Extends the offset clamp on each side without touching the
    /// recorded `content` size — bars still reflect the real
    /// content, and child layout is unaffected. Think of it as
    /// invisible overscroll: the user can wheel/drag past the
    /// content edge by the per-side amount, but a bar thumb wouldn't
    /// show extra travel and no padding/gutter is reserved. Use for
    /// canvas-style scopes (node graphs, infinite boards) that want
    /// pan slack past the children's bounding box. Per-side values
    /// come from `Spacing` (`left`/`top` open a negative-offset
    /// band; `right`/`bottom` extend the positive band) — set them
    /// dynamically per frame from your own content's bounding box if
    /// you need the slack to track a moving leading edge.
    pub fn content_margin(mut self, m: impl Into<Spacing>) -> Self {
        self.content_margin = m.into();
        self
    }

    /// Enable pivot-anchored zoom with a default [`ZoomConfig`]. Asserts
    /// at record time that the scroll pans on both axes (built via
    /// [`Scroll::both`]) — uniform scale on a single-axis scroll has no
    /// clean answer (cross-axis content escapes the viewport with no way
    /// to reach it). Caller bug, hard error.
    pub fn with_zoom(mut self) -> Self {
        self.zoom = Some(ZoomConfig::default());
        self
    }

    /// Enable zoom with explicit config. See [`Self::with_zoom`].
    pub fn with_zoom_config(mut self, cfg: ZoomConfig) -> Self {
        self.zoom = Some(cfg);
        self
    }

    pub fn show<R>(
        self,
        ui: &mut Ui,
        body: impl FnOnce(&mut Ui) -> R,
    ) -> crate::widgets::InnerResponse<'_, R> {
        let id = ui.make_persistent_id(self.element.salt);
        let mode = self.element.mode;
        let pan_payload = self.element.mode_payload;
        assert!(
            matches!(mode, LayoutMode::Scroll),
            "Scroll widget must carry LayoutMode::Scroll",
        );
        let pan = LayoutMode::pan_mask_from_payload(pan_payload);
        if self.zoom.is_some() {
            assert!(
                pan.x && pan.y,
                "Scroll::with_zoom requires Scroll::both — single-axis scroll has no clean zoom semantics",
            );
        }

        // Record-time clamp uses last frame's `viewport`/`content`/
        // `offset`. The matching re-clamp in `scroll::arrange`
        // corrects with fresh numbers post-arrange. Off-axis offsets
        // stay at 0 for single-axis scrolls.
        //
        // Input routes by `Sense::SCROLL`, which sits on the outer
        // ZStack (so wheel events over the bar gutter still pan the
        // viewport). Layout state, however, is keyed by the inner
        // viewport node's id — that's the `LayoutMode::Scroll` node
        // the driver writes to.
        let scroll_id = id.with("__viewport");
        // Font-derived line step for wheel→pixel conversion. Pulls
        // `theme.text` (the default font config) rather than scanning
        // children for a dominant font — that's a future polish; for
        // now the active theme's text size is a good proxy and stays
        // consistent with what the user is reading.
        let line_px = ui.theme.text.line_height_for(ui.theme.text.font_size_px);
        let pan_delta_raw = ui.input.scroll_delta_for(id, line_px);
        let wheel_notches = ui.input.scroll_notches_for(id, line_px);
        let pinch_delta = ui.input.zoom_delta_for(id);
        let mods = ui.input.modifiers;
        // Gate on `mods.ctrl` only — Ctrl is the zoom modifier on every
        // platform (macOS Cmd not honored), and `alt`-wheel shouldn't
        // zoom.
        let wheel_zoom_gate = self.zoom.as_ref().is_some_and(|cfg| match cfg.modifier {
            ZoomModifier::Ctrl => mods.ctrl,
            ZoomModifier::Always => true,
            ZoomModifier::PinchOnly => false,
        });
        // Route the wheel: when the gate matches, the notches become
        // a multiplicative zoom factor; pan is suppressed for the same
        // frame. `wheel_notches` already combines classic-wheel lines
        // and touchpad-pixel→virtual-notch (via `line_px`) so ctrl
        // held over a touchpad pinch-via-scroll zooms at the same rate
        // it would have panned. Positive notches.y means scroll-down
        // which by convention zooms *out* (factor < 1).
        let (pan_delta, wheel_zoom_factor) = if wheel_zoom_gate {
            let cfg = self.zoom.as_ref().unwrap();
            (Vec2::ZERO, cfg.step.powf(-wheel_notches.y))
        } else {
            (pan_delta_raw, 1.0_f32)
        };
        let zoom_delta = pinch_delta * wheel_zoom_factor;
        // Pivot in widget-local coords (outer rect origin). On the
        // first frame the response rect is None — fall back to viewport
        // center, which makes the zoom *feel* anchored even before
        // pointer-tracked anchoring kicks in.
        let resp_rect = ui.response_for(id).rect;
        let widget_origin = resp_rect.map(|r| r.min);
        let widget_size = resp_rect.map(|r| r.size);
        let pivot_local = if (zoom_delta - 1.0).abs() > f32::EPSILON {
            let pointer_local = ui.input.pointer_pos.zip(widget_origin).map(|(p, o)| p - o);
            let cfg_pivot = self
                .zoom
                .as_ref()
                .map(|c| c.pivot)
                .unwrap_or(ZoomPivot::Pointer);
            match (cfg_pivot, pointer_local, widget_size) {
                (ZoomPivot::Pointer, Some(p), _) => Some(p),
                (_, _, Some(sz)) => Some(Vec2::new(sz.w * 0.5, sz.h * 0.5)),
                _ => None,
            }
        } else {
            None
        };
        // Thumb-drag input. Read drag state of each thumb leaf
        // *before* taking the `&mut` borrow on `scroll_states` —
        // `response_for` walks `cascades`, which lives next to the
        // scroll-state map on `Ui`. On the first frame the thumbs
        // were recorded, the cascade doesn't see them yet → all
        // fields default. Same one-frame settle as other scroll
        // bookkeeping.
        let theme = ui.theme.scrollbar.clone();
        let thumb_id_v = scroll_id.with("__vthumb");
        let thumb_id_h = scroll_id.with("__hthumb");
        let track_id_v = scroll_id.with("__vtrack");
        let track_id_h = scroll_id.with("__htrack");
        let resp_v = ui.response_for(thumb_id_v);
        let resp_h = ui.response_for(thumb_id_h);
        let resp_track_v = ui.response_for(track_id_v);
        let resp_track_h = ui.response_for(track_id_h);
        let pointer = ui.input.pointer_pos;

        let scroll = {
            let row = ui.layout_engine.scroll_states.entry(scroll_id).or_default();
            // Forward the builder-set content margin to the layout
            // driver — measure inflates `content` by these totals so
            // overflow / slack / bar math sees the padded extent.
            row.content_margin = self.content_margin;
            // 1) Zoom step (pivot-anchored). Clamp `new_zoom` to
            //    `cfg.range`, derive the effective `dz_eff`, then
            //    update `offset` so the pivot point in widget-local
            //    coords stays fixed across the scale change.
            if let (Some(cfg), Some(p)) = (self.zoom.as_ref(), pivot_local) {
                let new_zoom = (row.zoom * zoom_delta).clamp(*cfg.range.start(), *cfg.range.end());
                let dz_eff = if row.zoom > 0.0 {
                    new_zoom / row.zoom
                } else {
                    1.0
                };
                if (dz_eff - 1.0).abs() > f32::EPSILON {
                    row.offset = (row.offset + p) * dz_eff - p;
                    row.zoom = new_zoom;
                }
            }
            // 2) Pan from the wheel delta. Only clamp when pan_delta is
            //    actually nonzero — pure-zoom frames must leave the
            //    pivot-anchored offset alone (otherwise repeated tiny
            //    pinches near a content edge would each snap offset
            //    back into the natural range and drift the world point
            //    under the cursor).
            //
            //    Natural range is `[min(0, slack), max(0, slack)]`:
            //    `[0, slack]` for overflow, `[slack, 0]` for underflow.
            //    Pivot-anchored zoom can legitimately leave `offset`
            //    outside that range — e.g. user zooms out below slack=0
            //    (offset goes negative to anchor the cursor), then
            //    zooms back in so slack flips positive while offset is
            //    still negative. A wheel-pan that frame must NOT yank
            //    `offset` back to `[0, slack]` (that's the visible
            //    "snap to top" when the bar reappears). Extend the
            //    clamp range to include the current offset so pan
            //    further out-of-range is blocked but pan toward the
            //    natural range works — the user scrolls back gradually,
            //    never with a one-frame yank.
            // Natural offset range = `[0, content - viewport]` per
            // axis (scaled by `zoom`), extended on each side by the
            // user-set `content_margin`. Margin is invisible
            // overscroll — doesn't show up in bars, just lets the
            // user wheel/drag past the natural bounds.
            let [cml, cmt, cmr, cmb] = row.content_margin.as_array();
            let neg_x = cml * row.zoom;
            let neg_y = cmt * row.zoom;
            let slack_x = row.content.w * row.zoom - row.viewport.w;
            let slack_y = row.content.h * row.zoom - row.viewport.h;
            let min_x = -neg_x;
            let max_x = slack_x + cmr * row.zoom;
            let min_y = -neg_y;
            let max_y = slack_y + cmb * row.zoom;
            if pan.x && pan_delta.x != 0.0 {
                let lo = row.offset.x.min(min_x.min(max_x));
                let hi = row.offset.x.max(min_x.max(max_x));
                row.offset.x = (row.offset.x + pan_delta.x).clamp(lo, hi);
            }
            if pan.y && pan_delta.y != 0.0 {
                let lo = row.offset.y.min(min_y.min(max_y));
                let hi = row.offset.y.max(min_y.max(max_y));
                row.offset.y = (row.offset.y + pan_delta.y).clamp(lo, hi);
            }

            // 2b) Settled clamp for non-zoomable scrolls. The
            //    out-of-range preservation above exists only for
            //    pivot-anchored zoom — a scroll with no zoom configured
            //    has no legitimate reason to hold `offset` outside the
            //    natural range, so the pan-gated clamp leaves a stale
            //    offset stranded when `content` *shrinks* with no wheel
            //    input (collapse a node, filter a list): nothing pulls
            //    the viewport back from the now-empty tail. Clamp every
            //    frame here. `[min, max]` already folds in
            //    `content_margin`, so intentional margin overscroll is
            //    preserved; only a genuine over-range offset snaps back.
            //    Zoomable scrolls skip this and keep the gradual
            //    pan-return behaviour the zoom path depends on.
            if self.zoom.is_none() {
                row.offset.x = row.offset.x.clamp(min_x.min(max_x), min_x.max(max_x));
                row.offset.y = row.offset.y.clamp(min_y.min(max_y), min_y.max(max_y));
            }

            // 3) Thumb-drag pan. Snapshot `offset` on the
            //    `drag_started` edge; subsequent frames compose
            //    `offset.main = anchor.main + drag_delta.main *
            //    factor` where `factor = max_off / (track - thumb)`.
            //    Cumulative `drag_delta` against a stable anchor
            //    keeps the math idempotent across re-records; the
            //    alternative ("offset += this-frame-delta") would
            //    double-apply because `drag_delta` is the total
            //    travel since press, not the per-frame increment.
            //    Bars use the *scaled* content extent so dragging
            //    inside a zoomed-in viewport tracks the cursor at
            //    1:1 with the visible thumb.
            let bl = bar_layout(row, pan, self.element.padding, &theme, self.bar_mode);
            for (axis, resp) in [(Axis::Y, resp_v), (Axis::X, resp_h)] {
                let panned = match axis {
                    Axis::Y => pan.y,
                    Axis::X => pan.x,
                };
                if !panned {
                    continue;
                }
                if resp.drag_started() {
                    row.drag_anchor = Some((axis, row.offset));
                }
                let Some((anchor_axis, anchor)) = row.drag_anchor else {
                    continue;
                };
                if anchor_axis != axis {
                    continue;
                }
                let Some(delta) = resp.drag_delta() else {
                    // Drag ended on this thumb — drop the anchor so
                    // the next press starts a fresh snapshot.
                    row.drag_anchor = None;
                    continue;
                };
                let track_main = axis.main(bl.bar_viewport);
                let main_content = axis.main(bl.scaled_content);
                let Some(geom) = bar_geometry(
                    track_main,
                    main_content,
                    axis.main_v(row.offset),
                    track_main,
                    &theme,
                ) else {
                    continue;
                };
                let travel = (track_main - geom.thumb_size).max(f32::EPSILON);
                let max_off = (main_content - track_main).max(0.0);
                let factor = max_off / travel;
                let target = axis.main_v(anchor) + axis.main_v(delta) * factor;
                let clamped = target.clamp(0.0, max_off);
                match axis {
                    Axis::X => row.offset.x = clamped,
                    Axis::Y => row.offset.y = clamped,
                }
            }

            // 4) Click-on-track to page. Press on the empty track
            //    above/below the thumb pages the offset by one viewport
            //    in the click direction. The track's main-axis origin
            //    is 0 in outer-local coords, so the click position
            //    along the bar is `pointer.main - outer_origin.main`.
            //    Pointer-position is the current pointer (release-
            //    frame); good enough since clicks fire on release and
            //    the pointer hasn't moved far. Clamped to `[0,
            //    max_off]` — same range as thumb-drag, not the wheel's
            //    extended range, because click-paging is always a
            //    toward-natural motion.
            let panned_axes = [
                (Axis::Y, resp_track_v, pan.y),
                (Axis::X, resp_track_h, pan.x),
            ];
            for (axis, resp_track, panned) in panned_axes {
                if !panned || !resp_track.clicked {
                    continue;
                }
                let (Some(ptr), Some(origin)) = (pointer, widget_origin) else {
                    continue;
                };
                // page step = bar_viewport.main (one visible page).
                let page_step = axis.main(bl.bar_viewport);
                let main_content = axis.main(bl.scaled_content);
                let Some(geom) = bar_geometry(
                    page_step,
                    main_content,
                    axis.main_v(row.offset),
                    page_step,
                    &theme,
                ) else {
                    continue;
                };
                let click_main = axis.main_v(ptr) - axis.main_v(origin);
                let max_off = (main_content - page_step).max(0.0);
                let cur = axis.main_v(row.offset);
                let next = if click_main < geom.thumb_offset {
                    (cur - page_step).max(0.0)
                } else if click_main > geom.thumb_offset + geom.thumb_size {
                    (cur + page_step).min(max_off)
                } else {
                    cur
                };
                match axis {
                    Axis::X => row.offset.x = next,
                    Axis::Y => row.offset.y = next,
                }
            }
            *row
        };

        if !scroll.seen {
            // Cold-mount: state is default, so `bar_plan` below will
            // see `content = 0`, decide "no overflow", and skip the
            // thumb. After this pass's arrange the row is filled in
            // with measured content + overflow; requesting a relayout
            // re-records with the right thumb visibility on pass B.
            // The viewport size itself is already correct on pass A
            // because the gutter reservation is constant — only the
            // thumb-or-no-thumb decision is stale.
            ui.request_relayout();
        }
        let outer_size = scroll.outer;
        let zoom = scroll.zoom;
        let offset = scroll.offset;
        // Reservation + post-zoom content + bar-main length, all
        // derived from `outer - reservation - user_padding` rather
        // than the cached `viewport`. The cached viewport lags by
        // one arrange pass during cold-mount; this derivation is
        // stable at record time. Same helper feeds drag math inside
        // the state-mutation block above.
        let bl = bar_layout(&scroll, pan, self.element.padding, &theme, self.bar_mode);
        let scaled_content = bl.scaled_content;
        let bar_viewport = bl.bar_viewport;
        let reserve_y = bl.reserve_y;
        let reserve_x = bl.reserve_x;

        // Outer: bare ZStack that holds the inner viewport + a bar
        // overlay. The reservation gutter lives on `inner.margin` —
        // not on outer's padding — so the bar overlay (sibling of
        // inner under the same ZStack) can reach into the gutter
        // strip with absolute positions. The field routing
        // (outer = sizing/placement, inner = layout/panel knobs) lives
        // in `Element::into_scroll_wrappers`; the per-frame computed
        // fields below patch `inner`.
        let ScrollWrappers { outer, mut inner } = scroll_wrappers(self.element);

        // Inner viewport owns the clip, the pan transform, the user-set
        // padding (encoder deflates the clip mask by it), and the
        // `Scroll` layout mode that runs children with INF on panned
        // axes. The reservation gutter is its margin — ZStack arrange
        // deflates `Sizing::Fill` by margin, so inner's rendered rect =
        // outer.rect minus the reserved strip on the cross axes.
        //
        // Encode the user's per-axis `Sizing` into the viewport's fit
        // bits: a `Hug` panned axis makes the driver report its content
        // extent, so the scroll sizes to content like any other `Hug`
        // widget (bounded by `max_size`/available, scrolling past the
        // cap); `Fill`/`Fixed` keep the content-independent viewport.
        let user = self.element.size;
        let mut inner_payload = self.element.mode_payload;
        if pan.x && matches!(user.w(), Sizing::Hug) {
            inner_payload |= LayoutMode::SCROLL_FIT_X;
        }
        if pan.y && matches!(user.h(), Sizing::Hug) {
            inner_payload |= LayoutMode::SCROLL_FIT_Y;
        }
        inner.mode_payload = inner_payload;
        inner.salt = Salt::Verbatim(scroll_id);
        inner.margin = Spacing::new(0.0, 0.0, reserve_y, reserve_x);
        let inner_chrome = self.chrome;
        // Scroll is always clipped — `with_axes` set `ClipMode::Rect`
        // by default; if the caller upgraded to `Rounded` via
        // `Configure::clip_rounded`, that wins.
        let user_clip = self.element.flags.clip_mode();
        inner
            .flags
            .set_clip(if matches!(user_clip, ClipMode::None) {
                ClipMode::Rect
            } else {
                user_clip
            });
        // Raw pan/zoom — cascade anchors the scale at the inner's own
        // `layout_rect.min` (`TranslateScale::anchored_at`), so we
        // don't pre-bake the origin compensation. Translation is just
        // the user's scroll offset, negated (scroll right shifts
        // content left).
        if offset != Vec2::ZERO || (zoom - 1.0).abs() > f32::EPSILON {
            inner.transform = TranslateScale::new(-offset, zoom);
        }

        let plan_v = bar_plan(
            bar_viewport,
            outer_size,
            scaled_content,
            offset,
            Axis::Y,
            pan.y,
            &theme,
        );
        let plan_h = bar_plan(
            bar_viewport,
            outer_size,
            scaled_content,
            offset,
            Axis::X,
            pan.x,
            &theme,
        );

        let inner_value = ui.node(id, outer, None, |ui| {
            let inner_value = ui.node(scroll_id, inner, inner_chrome.as_ref(), body);
            // Bar overlay: Canvas sibling of inner, Fill on both axes
            // → covers outer's full rect. Tracks attach as shapes on
            // the overlay (paint first); thumbs are Sense::DRAG leaves
            // positioned absolutely on top. Painted after inner via
            // record order, hit-tested above inner via cascade order.
            if !matches!(self.bar_mode, BarMode::Hidden) && (plan_v.is_some() || plan_h.is_some()) {
                let bars_id = scroll_id.with("__bars");
                let mut overlay = Element::new(LayoutMode::Canvas);
                overlay.salt = Salt::Verbatim(bars_id);
                overlay.size = (Sizing::FILL, Sizing::FILL).into();
                ui.node(bars_id, overlay, None, |ui| {
                    if let Some(p) = plan_v {
                        push_bar_nodes(ui, p, track_id_v, thumb_id_v, resp_v, &theme);
                    }
                    if let Some(p) = plan_h {
                        push_bar_nodes(ui, p, track_id_h, thumb_id_h, resp_h, &theme);
                    }
                });
            }
            inner_value
        });

        let resp_state = ui.response_for(id);
        crate::widgets::InnerResponse {
            // Eager: Scroll already paid for `response_for` here
            // and the caller almost always reads at least one field
            // (drag delta, scroll delta, hovered). Hand the cached
            // state through.
            response: Response::eager(id, ui, resp_state),
            inner: inner_value,
        }
    }
}

impl Scroll {
    /// Paint chrome for the inner scroll surface (background under
    /// children, painted before the scrollbar overlay).
    pub fn background(mut self, bg: Background) -> Self {
        self.chrome = Some(bg);
        self
    }
}

impl Configure for Scroll {
    fn element_mut(&mut self) -> &mut Element {
        &mut self.element
    }
}
