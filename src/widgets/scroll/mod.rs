use crate::input;
use crate::input::response::ResponseState;
use crate::input::sense::Sense;
use crate::layout::axis::Axis;
use crate::layout::scroll::{ScrollLayoutState, TrackPage};
use crate::layout::types::clip_mode::ClipMode;
use crate::layout::types::layout_mode::ScrollSpec;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::corners::Corners;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::transform::TranslateScale;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::{Configure, ConfigureNode, Node};
use crate::ui::Ui;
use crate::widgets::theme::scrollbar::ScrollbarTheme;
use crate::widgets::{InnerResponse, Response};
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
    range: RangeInclusive<f32>,
    step: f32,
    /// Wheel-vs-pinch routing. Default [`ZoomModifier::Ctrl`].
    pub modifier: ZoomModifier,
    /// Where the zoom step pivots. Default [`ZoomPivot::Pointer`].
    pub pivot: ZoomPivot,
}

const ZOOM_RANGE_ERROR: &str = "zoom range must satisfy 0 < min <= max with finite bounds";
const ZOOM_STEP_ERROR: &str = "zoom step must be finite and positive";

impl ZoomConfig {
    /// Configure the inclusive zoom range and multiplicative wheel factor.
    ///
    /// # Panics
    ///
    /// Panics unless both range bounds are finite, `0 < min <= max`, and
    /// `step` is finite and positive.
    #[track_caller]
    pub fn new(range: RangeInclusive<f32>, step: f32) -> Self {
        let min = *range.start();
        let max = *range.end();
        assert!(
            input::zoom_factor_is_valid(min) && input::zoom_factor_is_valid(max) && min <= max,
            "{ZOOM_RANGE_ERROR}"
        );
        assert!(input::zoom_factor_is_valid(step), "{ZOOM_STEP_ERROR}");
        Self {
            range,
            step,
            modifier: ZoomModifier::Ctrl,
            pivot: ZoomPivot::Pointer,
        }
    }
}

impl Default for ZoomConfig {
    fn default() -> Self {
        Self::new(0.1..=10.0, 1.03)
    }
}

// `ScrollLayoutState` lives on `LayoutEngine::scroll_states` rather
// than `StateMap` — it's a layout-derived concern, the scroll driver
// writes the layout fields during measure + arrange, and the widget
// reads/mutates the row at record time via [`Ui::scroll_state`].
//
// Bar drawing + reservation logic stay here as widget concerns; the
// layout primitive itself is unaware of scrollbars.

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

#[derive(Debug)]
struct BarResponses {
    theme: ScrollbarTheme,
    thumb_id_v: WidgetId,
    thumb_id_h: WidgetId,
    track_id_v: WidgetId,
    track_id_h: WidgetId,
    resp_v: ResponseState,
    resp_h: ResponseState,
    resp_track_v: ResponseState,
    resp_track_h: ResponseState,
}

impl BarResponses {
    fn read(ui: &Ui, scroll_id: WidgetId) -> Self {
        let thumb_id_v = scroll_id.with("__vthumb");
        let thumb_id_h = scroll_id.with("__hthumb");
        let track_id_v = scroll_id.with("__vtrack");
        let track_id_h = scroll_id.with("__htrack");
        Self {
            theme: ui.theme.scrollbar.clone(),
            thumb_id_v,
            thumb_id_h,
            track_id_v,
            track_id_h,
            resp_v: ui.response_for(thumb_id_v),
            resp_h: ui.response_for(thumb_id_h),
            resp_track_v: ui.response_for(track_id_v),
            resp_track_h: ui.response_for(track_id_h),
        }
    }
}

#[derive(Copy, Clone, Debug)]
struct BarPlans {
    vertical: Option<BarPlan>,
    horizontal: Option<BarPlan>,
}

#[derive(Debug)]
struct BarFrame {
    responses: BarResponses,
    layout: BarLayout,
    plans: BarPlans,
}

#[derive(Debug)]
struct ScrollFrame {
    scroll: ScrollLayoutState,
    bars: Option<BarFrame>,
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
    let track = Node::leaf()
        .id(track_id)
        .size((
            Sizing::fixed(plan.track_rect.size.w),
            Sizing::fixed(plan.track_rect.size.h),
        ))
        .position(plan.track_rect.min)
        .sense(Sense::CLICK);
    if !theme.track.is_noop() {
        let chrome = Background::rounded(theme.track, radius);
        ui.widget(track).record(ui, Some(&chrome), |_| {});
    } else {
        ui.widget(track).record(ui, None, |_| {});
    }

    let fill = if resp.left.drag.delta().is_some() || resp.pressed() {
        theme.thumb_active
    } else if resp.hovered {
        theme.thumb_hover
    } else {
        theme.thumb
    };
    let thumb = Node::leaf()
        .id(thumb_id)
        .size((
            Sizing::fixed(plan.thumb_rect.size.w),
            Sizing::fixed(plan.thumb_rect.size.h),
        ))
        .position(plan.thumb_rect.min)
        .sense(Sense::DRAG);
    let chrome = Background::rounded(fill, radius);
    ui.widget(thumb).record(ui, Some(&chrome), |_| {});
}

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

/// The two wrapper `Node`s a `Scroll` records: an outer `ZStack`
/// that owns sizing / placement / sense / visibility and an inner
/// viewport that owns the `Scroll` layout mode, padding, and the panel
/// knobs (gap / justify / child_align).
#[derive(Debug)]
struct ScrollWrappers {
    outer: Node,
    inner: Node,
}

/// Split a user `Scroll` node into its outer/inner wrappers.
///
/// **This routes every `Node` field that should survive on a
/// `Scroll`** — the exhaustive destructure means adding a `ode`
/// field fails to compile here, forcing the decision whether it lands
/// on `outer` (sizing/placement) or `inner` (layout/panel knobs).
/// `Scroll::show` patches the remaining inner fields it computes per
/// frame (`salt`, the reservation `margin`, layout fit flags,
/// `clip` — read off `flags` before this runs — and the pan
/// `transform`). The user salt stays on the `Widget` resolved in
/// `Scroll::show`; neither wrapper carries it.
fn scroll_wrappers(node: Node) -> ScrollWrappers {
    let scroll_spec = node.scroll_spec();
    let Node {
        salt: _,
        mode: _,
        size,
        min_size,
        max_size,
        padding,
        margin,
        gaps,
        justify,
        align,
        child_align,
        position,
        grid,
        flags,
        visibility,
        // Scroll owns its mode/transform, and no fallback runs after this split.
        ..
    } = node;

    let mut outer = Node::zstack();
    outer.size = size;
    outer.min_size = min_size;
    outer.max_size = max_size;
    outer.margin = margin;
    outer.align = align;
    outer.position = position;
    outer.grid = grid;
    outer.flags.set_sense(flags.sense());
    outer.flags.set_disabled(flags.is_disabled());
    outer.flags.set_focusable(flags.is_focusable());
    outer.visibility = visibility;

    let mut inner = Node::scroll(scroll_spec);
    // Inner fills the outer wrapper; the outer carries the user's
    // `Sizing` and drives the actual size.
    inner.size = Some((Sizing::FILL, Sizing::FILL).into());
    inner.padding = padding;
    inner.gaps = gaps;
    inner.justify = justify;
    inner.child_align = child_align;
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
#[derive(Debug)]
pub struct Scroll {
    node: Node,
    zoom: Option<ZoomConfig>,
    chrome: Option<Background>,
    bar_mode: BarMode,
    content_margin: Spacing,
}

impl Scroll {
    #[track_caller]
    pub fn vertical() -> Self {
        Self::with_axes(ScrollSpec::VERTICAL)
    }

    #[track_caller]
    pub fn horizontal() -> Self {
        Self::with_axes(ScrollSpec::HORIZONTAL)
    }

    #[track_caller]
    pub fn both() -> Self {
        Self::with_axes(ScrollSpec::BOTH)
    }

    /// Paint chrome for the inner scroll surface (background under
    /// children, painted before the scrollbar overlay).
    ///
    /// Unlike the other containers (`Panel`/`Grid`/`Popup`), Scroll does
    /// **not** fall back to `theme.panel_background` when unset — an
    /// unstyled scroll surface paints no background. Pass one explicitly
    /// to fill it.
    pub fn background(mut self, bg: Background) -> Self {
        self.chrome = Some(bg);
        self
    }

    #[track_caller]
    fn with_axes(spec: ScrollSpec) -> Self {
        let mut node = Node::scroll(spec);
        node.flags.set_sense(Sense::SCROLL);
        // Scroll requires clipping; default to `Rect` so callers that
        // don't override get the cheap scissor path. Callers can still
        // call `Configure::clip_rounded` to upgrade to a stencil mask.
        node.clip = Some(ClipMode::Rect);
        Self {
            node,
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
    /// to reach it). Debug builds reject the caller bug.
    pub fn with_zoom(self) -> Self {
        self.with_zoom_config(ZoomConfig::default())
    }

    /// Enable zoom with explicit config. See [`Self::with_zoom`].
    pub fn with_zoom_config(mut self, cfg: ZoomConfig) -> Self {
        self.zoom = Some(cfg);
        let sense = self.node.flags.sense() | Sense::PINCH;
        self.sense(sense)
    }

    pub fn show<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> InnerResponse<'_, R> {
        let mut widget = ui.widget(self.node);
        let id = widget.id();
        let pan = self.node.scroll_spec().pan_mask();
        if self.zoom.is_some() {
            debug_assert!(
                pan.x && pan.y,
                "Scroll::with_zoom requires Scroll::both — single-axis scroll has no clean zoom semantics",
            );
        }

        // Record-time clamp uses last frame's `viewport`/`content`/
        // `offset`. Off-axis offsets stay at 0 for single-axis scrolls.
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
        let scroll_delta = ui.input.scroll_delta_for(id);
        let pan_delta_raw = scroll_delta.pixels + scroll_delta.lines * line_px;
        let wheel_notches = scroll_delta.lines + scroll_delta.pixels / line_px.max(f32::EPSILON);
        let pinch_delta = scroll_delta.zoom;
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
            (
                Vec2::ZERO,
                input::wheel_zoom_factor(cfg.step, wheel_notches.y),
            )
        } else {
            (pan_delta_raw, 1.0_f32)
        };
        let zoom_delta = input::combine_zoom_factors(pinch_delta, wheel_zoom_factor);
        // Pivot in widget-local coords (outer rect origin). On the
        // first frame the response rect is None — fall back to viewport
        // center, which makes the zoom *feel* anchored even before
        // pointer-tracked anchoring kicks in.
        let outer_response = ui.response_for(id);
        let widget_size = outer_response.layout_rect.map(|r| r.size);
        let pivot_local = if (zoom_delta - 1.0).abs() > f32::EPSILON {
            let cfg_pivot = self
                .zoom
                .as_ref()
                .map(|c| c.pivot)
                .unwrap_or(ZoomPivot::Pointer);
            match (cfg_pivot, outer_response.pointer_local, widget_size) {
                (ZoomPivot::Pointer, Some(p), _) => Some(p),
                (_, _, Some(sz)) => Some(Vec2::new(sz.w * 0.5, sz.h * 0.5)),
                _ => None,
            }
        } else {
            None
        };
        let bar_responses = if self.bar_mode == BarMode::Hidden {
            None
        } else {
            Some(BarResponses::read(ui, scroll_id))
        };

        let frame = {
            let row = ui.layout_engine.scroll_states.entry(scroll_id).or_default();
            // Keep margin separate from measured content so overflow
            // and bars continue to reflect the real content bounds.
            row.content_margin = self.content_margin;
            // The offset/zoom-mutation math lives on `ScrollLayoutState`
            // (the type that owns `offset`); the widget computes the
            // per-frame inputs + the theme-derived bar geometry here and
            // calls the row methods to apply them.
            // 1) Pivot-anchored zoom step.
            if let (Some(cfg), Some(p)) = (self.zoom.as_ref(), pivot_local) {
                row.apply_zoom(*cfg.range.start(), *cfg.range.end(), p, zoom_delta);
            }
            // 2) Wheel pan, then 2b) the settled clamp for non-zoomable
            //    scrolls (zoomable ones keep the out-of-range drift the
            //    pivot path depends on).
            let preserve_zoom_underflow = self.zoom.is_some();
            row.apply_wheel_pan(pan.x, pan.y, pan_delta, preserve_zoom_underflow);
            if !preserve_zoom_underflow {
                row.clamp_to_natural();
            }
            let bars = bar_responses.map(|responses| {
                // Bars use the *scaled* content extent so dragging inside a
                // zoomed viewport tracks the cursor 1:1 with the visible thumb.
                let bl = bar_layout(
                    row,
                    pan,
                    self.node.padding.unwrap_or(Spacing::ZERO),
                    &responses.theme,
                    self.bar_mode,
                );
                for (axis, resp) in [(Axis::Y, responses.resp_v), (Axis::X, responses.resp_h)] {
                    let panned = match axis {
                        Axis::Y => pan.y,
                        Axis::X => pan.x,
                    };
                    if !panned {
                        continue;
                    }
                    let track_main = axis.main(bl.bar_viewport);
                    let main_content = axis.main(bl.scaled_content);
                    let geom = bar_geometry(
                        track_main,
                        main_content,
                        axis.main_v(row.offset),
                        track_main,
                        &responses.theme,
                    )
                    .map(|g| {
                        let travel = (track_main - g.thumb_size).max(f32::EPSILON);
                        let max_off = (main_content - track_main).max(0.0);
                        (max_off / travel, max_off)
                    });
                    row.apply_thumb_drag(
                        axis,
                        resp.left.drag.started(),
                        resp.left.drag.delta(),
                        geom,
                    );
                }
                for (axis, resp_track, panned) in [
                    (Axis::Y, responses.resp_track_v, pan.y),
                    (Axis::X, responses.resp_track_h, pan.x),
                ] {
                    if !panned || !resp_track.left.clicked() {
                        continue;
                    }
                    let Some(pointer_local) = resp_track.pointer_local else {
                        continue;
                    };
                    let page_step = axis.main(bl.bar_viewport);
                    let main_content = axis.main(bl.scaled_content);
                    let page = bar_geometry(
                        page_step,
                        main_content,
                        axis.main_v(row.offset),
                        page_step,
                        &responses.theme,
                    )
                    .map(|g| TrackPage {
                        click_main: axis.main_v(pointer_local),
                        thumb_offset: g.thumb_offset,
                        thumb_size: g.thumb_size,
                        page_step,
                        max_off: (main_content - page_step).max(0.0),
                    });
                    row.apply_track_page(axis, page);
                }
                let plans = BarPlans {
                    vertical: bar_plan(
                        bl.bar_viewport,
                        row.outer,
                        bl.scaled_content,
                        row.offset,
                        Axis::Y,
                        pan.y,
                        &responses.theme,
                    ),
                    horizontal: bar_plan(
                        bl.bar_viewport,
                        row.outer,
                        bl.scaled_content,
                        row.offset,
                        Axis::X,
                        pan.x,
                        &responses.theme,
                    ),
                };
                BarFrame {
                    responses,
                    layout: bl,
                    plans,
                }
            });
            ScrollFrame { scroll: *row, bars }
        };

        if frame.bars.is_some() && !frame.scroll.seen {
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
        let zoom = frame.scroll.zoom;
        let offset = frame.scroll.offset;
        let (reserve_y, reserve_x) = frame
            .bars
            .as_ref()
            .map(|bars| (bars.layout.reserve_y, bars.layout.reserve_x))
            .unwrap_or_default();

        // Outer: bare ZStack that holds the inner viewport + a bar
        // overlay. The reservation gutter lives on `inner.margin` —
        // not on outer's padding — so the bar overlay (sibling of
        // inner under the same ZStack) can reach into the gutter
        // strip with absolute positions. The field routing
        // (outer = sizing/placement, inner = layout/panel knobs) lives
        // in `scroll_wrappers`; the per-frame computed fields below
        // patch `inner`.
        let ScrollWrappers { outer, mut inner } = scroll_wrappers(self.node);

        // Inner viewport owns the clip, the pan transform, the user-set
        // padding (encoder deflates the clip mask by it), and the
        // `Scroll` layout mode that runs children with INF on panned
        // axes. The reservation gutter is its margin — ZStack arrange
        // deflates `Sizing::fill` by margin, so inner's rendered rect =
        // outer.rect minus the reserved strip on the cross axes.
        //
        // Encode the user's per-axis `Sizing` into the viewport's fit
        // bits: a `Hug` panned axis makes the driver report its content
        // extent, so the scroll sizes to content like any other `Hug`
        // widget (bounded by `max_size`/available, scrolling past the
        // cap); `Fill`/`Fixed` keep the content-independent viewport.
        let user = self.node.size.unwrap_or_default();
        let fit = glam::BVec2::new(pan.x && user.w().is_hug(), pan.y && user.h().is_hug());
        inner.set_scroll_spec(self.node.scroll_spec().with_fit(fit));
        let mut inner = inner.id(scroll_id);
        inner.margin = Some(Spacing::new(0.0, 0.0, reserve_y, reserve_x));
        let inner_chrome = self.chrome;
        // `with_axes` set `ClipMode::Rect` by default; caller configuration
        // can replace it with rounded clipping or no clipping.
        inner.clip = self.node.clip;
        // Raw pan/zoom — cascade anchors the scale at the inner's own
        // `layout_rect.min` (`TranslateScale::anchored_at`), so we
        // don't pre-bake the origin compensation. Translation is just
        // the user's scroll offset, negated (scroll right shifts
        // content left).
        if offset != Vec2::ZERO || (zoom - 1.0).abs() > f32::EPSILON {
            inner.transform = TranslateScale::new(-offset, zoom);
        }

        widget.node = outer;
        let inner_value = widget.record(ui, None, |ui| {
            let inner_value = ui.widget(inner).record(ui, inner_chrome.as_ref(), body);
            // Bar overlay: Canvas sibling of inner, Fill on both axes
            // → covers outer's full rect. Tracks attach as shapes on
            // the overlay (paint first); thumbs are Sense::DRAG leaves
            // positioned absolutely on top. Painted after inner via
            // record order, hit-tested above inner via cascade order.
            if let Some(bars) = frame
                .bars
                .filter(|bars| bars.plans.vertical.is_some() || bars.plans.horizontal.is_some())
            {
                let overlay = Node::canvas()
                    .id(scroll_id.with("__bars"))
                    .size((Sizing::FILL, Sizing::FILL));
                ui.widget(overlay).record(ui, None, |ui| {
                    if let Some(p) = bars.plans.vertical {
                        push_bar_nodes(
                            ui,
                            p,
                            bars.responses.track_id_v,
                            bars.responses.thumb_id_v,
                            bars.responses.resp_v,
                            &bars.responses.theme,
                        );
                    }
                    if let Some(p) = bars.plans.horizontal {
                        push_bar_nodes(
                            ui,
                            p,
                            bars.responses.track_id_h,
                            bars.responses.thumb_id_h,
                            bars.responses.resp_h,
                            &bars.responses.theme,
                        );
                    }
                });
            }
            inner_value
        });

        let resp_state = ui.response_for(id);
        InnerResponse {
            // Eager: Scroll already paid for `response_for` here
            // and the caller almost always reads at least one field
            // (drag delta, scroll delta, hovered). Hand the cached
            // state through.
            response: Response::eager(id, ui, resp_state),
            inner: inner_value,
        }
    }
}

impl Configure for Scroll {
    fn node_mut(&mut self) -> ConfigureNode<'_> {
        self.node.node_mut()
    }
}

#[cfg(test)]
mod tests;
