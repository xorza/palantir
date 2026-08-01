pub(crate) mod state;

use crate::input::response::ResponseState;
use crate::input::sense::Sense;
use crate::input::zoom;
use crate::layout::axis::Axis;
use crate::layout::scrollbars::{self, ScrollBarsDef};
use crate::layout::types::clip_mode::ClipMode;
use crate::layout::types::layout_mode::ScrollSpec;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::corners::Corners;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::transform::TranslateScale;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::{Configure, ConfigureNode, Node};
use crate::ui::Ui;
use crate::widgets::scroll::state::{ScrollBounds, ScrollState, ThumbTravel, TrackPage};
use crate::widgets::theme::scrollbar::ScrollbarTheme;
use crate::widgets::{InnerResponse, Response};
use glam::{BVec2, Vec2};
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
            zoom::is_valid(min) && zoom::is_valid(max) && min <= max,
            "{ZOOM_RANGE_ERROR}"
        );
        assert!(zoom::is_valid(step), "{ZOOM_STEP_ERROR}");
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

/// Cross-axis space stolen from children when an axis's bar is shown:
/// the bar's `width` plus a `gap` strip so the bar doesn't touch the
/// visible content. Returns 0 when the axis isn't panned.
#[inline]
fn bar_reservation(panned: bool, theme: &ScrollbarTheme) -> f32 {
    if panned { theme.width + theme.gap } else { 0.0 }
}

/// Cross-axis space the bars take out of the widget's box: the gutter
/// reserved on each panned axis, and the viewport left over for content.
#[derive(Copy, Clone, Debug)]
struct BarSpace {
    bar_viewport: Size,
    reserve_y: f32,
    reserve_x: f32,
}

fn bar_space(
    outer: Size,
    pan: BVec2,
    user_padding: Spacing,
    theme: &ScrollbarTheme,
    bar_mode: BarMode,
) -> BarSpace {
    // Only `Reserved` reserves the gutter on the pan axes. `Overlay`
    // paints the bar over content without reservation; `Hidden` has
    // no bar at all. Reservation is constant for `Reserved` (not
    // toggled by overflow) so a Hug ancestor doesn't shift between
    // frames; the bar thumb itself still appears conditionally on
    // `content > viewport`, decided by `layout::scrollbars` after
    // measure rather than here.
    let reserve = matches!(bar_mode, BarMode::Reserved);
    let reserve_y = bar_reservation(pan.y && reserve, theme);
    let reserve_x = bar_reservation(pan.x && reserve, theme);
    let bar_viewport = scrollbars::viewport(outer, reserve_y, reserve_x, user_padding);
    BarSpace {
        bar_viewport,
        reserve_y,
        reserve_x,
    }
}

/// Last frame's measured content extent for this scroll, keyed by the
/// **inner viewport** node because that is the `LayoutMode::Scroll` one.
/// `Size::ZERO` until the node has been through one layout pass.
///
/// Resolves through the cascade, so like `Ui::response_for` it answers
/// for the previous frame — which is the lag `Scroll` wants: the bars
/// describe the content the user is looking at.
fn previous_scroll_content(ui: &Ui, scroll_id: WidgetId) -> Size {
    let Some(endpoint) = ui.cascades.by_id.get(&scroll_id) else {
        return Size::ZERO;
    };
    ui.layout[endpoint.layer].scroll_content[endpoint.node.idx()]
}

/// What one scroll frame resolves against, all read from *last* frame's
/// layout before any of this frame's input applies. Taken once at the
/// top of [`Scroll::show`] so the pan, the zoom, and both bars agree on
/// the box they are working in.
#[derive(Copy, Clone, Debug)]
struct ScrollGeometry {
    /// Content extent the bars are a ratio of, before zoom.
    content: Size,
    /// The user's own padding — reserved out of the viewport here, and
    /// handed to the bar overlay's driver so it deflates by the same.
    padding: Spacing,
    space: BarSpace,
    bounds: ScrollBounds,
}

impl ScrollGeometry {
    /// Content extent at the current zoom. The bars measure against
    /// this rather than the raw extent so dragging a thumb inside a
    /// zoomed viewport tracks the cursor 1:1 with what's on screen.
    fn scaled_content(&self, zoom: f32) -> Size {
        Size::new(self.content.w * zoom, self.content.h * zoom)
    }
}

/// This frame's wheel / trackpad / pinch input after routing.
#[derive(Copy, Clone, Debug)]
struct ScrollInput {
    pan_delta: Vec2,
    zoom_delta: f32,
    /// Widget-local point that stays fixed across the zoom step.
    /// `Some` exactly when a step lands and an anchor could be resolved.
    pivot: Option<Vec2>,
}

/// One scrollbar axis: the two leaves the overlay records for it, and
/// last frame's interaction on each.
#[derive(Copy, Clone, Debug)]
struct BarAxis {
    track_id: WidgetId,
    thumb_id: WidgetId,
    track: ResponseState,
    thumb: ResponseState,
}

impl BarAxis {
    /// Emit this axis's two nodes onto the overlay: a track leaf with
    /// `Sense::CLICK` (paging on press) and a thumb leaf with
    /// `Sense::DRAG` painted on top. Neither carries a size or a
    /// position — the overlay is a [`crate::layout::scrollbars`]
    /// container, and its arrange assigns both rects once measure has
    /// produced the content extent they are a ratio of.
    ///
    /// Both are recorded unconditionally, even on an axis showing no
    /// bar: arrange collapses those to zero extent. Recording them
    /// either way is what keeps the child list the same shape across an
    /// overflow toggle, which is what lets the driver address children
    /// positionally.
    ///
    /// Track stays a leaf even when `theme.track` alpha is 0 so the
    /// click-to-page surface remains — the gutter is reserved either
    /// way, matching OS scrollbar conventions.
    fn record(&self, ui: &mut Ui, theme: &ScrollbarTheme) {
        let radius = Corners::all(theme.radius);
        let track = Node::leaf().id(self.track_id).sense(Sense::CLICK);
        if !theme.track.is_noop() {
            let chrome = Background::rounded(theme.track, radius);
            ui.widget(track).record(ui, Some(&chrome), |_| {});
        } else {
            ui.widget(track).record(ui, None, |_| {});
        }

        let fill = if self.thumb.left.drag.delta().is_some() || self.thumb.pressed() {
            theme.thumb_active
        } else if self.thumb.hovered {
            theme.thumb_hover
        } else {
            theme.thumb
        };
        let thumb = Node::leaf().id(self.thumb_id).sense(Sense::DRAG);
        let chrome = Background::rounded(fill, radius);
        ui.widget(thumb).record(ui, Some(&chrome), |_| {});
    }
}

/// One axis's bar resolved against the offset at the moment it was
/// taken. Absent (`Bars::resolve` returning `None`) means the content
/// fits that axis and no thumb shows.
#[derive(Copy, Clone, Debug)]
struct ResolvedBar {
    /// Main-axis length of the track — also the page step, since a
    /// click past the thumb pages by one viewport.
    track_main: f32,
    /// Post-zoom content extent on the main axis.
    content_main: f32,
    thumb_offset: f32,
    thumb_size: f32,
}

impl ResolvedBar {
    /// Offset at which the content's trailing edge meets the track's.
    fn max_off(&self) -> f32 {
        (self.content_main - self.track_main).max(0.0)
    }

    fn travel(&self) -> ThumbTravel {
        ThumbTravel {
            factor: self.max_off() / (self.track_main - self.thumb_size).max(f32::EPSILON),
            max_off: self.max_off(),
        }
    }

    fn page_at(&self, click_main: f32) -> TrackPage {
        TrackPage {
            click_main,
            thumb_offset: self.thumb_offset,
            thumb_size: self.thumb_size,
            page_step: self.track_main,
            max_off: self.max_off(),
        }
    }
}

/// Both scrollbars: their ids, last frame's interaction on each, and the
/// theme they paint with. Read in full *before* the `&mut` state borrow
/// that acts on them, because reading a response borrows all of `Ui`.
#[derive(Debug)]
struct Bars {
    theme: ScrollbarTheme,
    v: BarAxis,
    h: BarAxis,
}

impl Bars {
    fn read(ui: &Ui, scroll_id: WidgetId) -> Self {
        let axis = |track: &str, thumb: &str| {
            let (track_id, thumb_id) = (scroll_id.with(track), scroll_id.with(thumb));
            BarAxis {
                track_id,
                thumb_id,
                track: ui.response_for(track_id),
                thumb: ui.response_for(thumb_id),
            }
        };
        Self {
            theme: ui.theme.scrollbar.clone(),
            v: axis("__vtrack", "__vthumb"),
            h: axis("__htrack", "__hthumb"),
        }
    }

    /// The axes in the order the layout driver addresses their nodes:
    /// vertical track + thumb, then horizontal.
    fn axes(&self) -> [(Axis, &BarAxis); 2] {
        [(Axis::Y, &self.v), (Axis::X, &self.h)]
    }

    /// This axis's thumb against `offset`, or `None` when the content
    /// fits and no thumb shows.
    fn resolve(
        &self,
        axis: Axis,
        geom: ScrollGeometry,
        scaled: Size,
        offset: f32,
    ) -> Option<ResolvedBar> {
        let track_main = axis.main(geom.space.bar_viewport);
        let content_main = axis.main(scaled);
        let g = scrollbars::bar_geometry(
            track_main,
            content_main,
            offset,
            track_main,
            self.theme.min_thumb_px,
        )?;
        Some(ResolvedBar {
            track_main,
            content_main,
            thumb_offset: g.thumb_offset,
            thumb_size: g.thumb_size,
        })
    }

    /// Fold this frame's bar interaction into the offset: thumb drags
    /// first, then track pages.
    ///
    /// Two passes, not one per axis: a page click reads the offset a
    /// same-frame drag on the *other* axis already moved, and the drag
    /// anchor is a single slot shared by both axes. Resolving each bar
    /// immediately before it is applied is what keeps the thumb tracking
    /// the cursor within the frame.
    fn drive(&self, state: &mut ScrollState, geom: ScrollGeometry, pan: BVec2) {
        let scaled = geom.scaled_content(state.zoom);
        for (axis, bar) in self.axes() {
            if !axis.main_b(pan) {
                continue;
            }
            let travel = self
                .resolve(axis, geom, scaled, axis.main_v(state.offset))
                .map(|resolved| resolved.travel());
            state.apply_thumb_drag(
                axis,
                bar.thumb.left.drag.started(),
                bar.thumb.left.drag.delta(),
                travel,
            );
        }
        for (axis, bar) in self.axes() {
            if !axis.main_b(pan) || !bar.track.left.clicked() {
                continue;
            }
            let Some(pointer_local) = bar.track.pointer_local else {
                continue;
            };
            let page = self
                .resolve(axis, geom, scaled, axis.main_v(state.offset))
                .map(|resolved| resolved.page_at(axis.main_v(pointer_local)));
            state.apply_track_page(axis, page);
        }
    }

    /// Record the bar overlay as a sibling of the viewport: a
    /// `scrollbars` container filling the outer rect, holding the four
    /// leaves in the fixed order its driver addresses them by. Painted
    /// after the viewport via record order, hit-tested above it via
    /// cascade order.
    fn record(
        &self,
        ui: &mut Ui,
        scroll_id: WidgetId,
        state: ScrollState,
        geom: ScrollGeometry,
        pan: BVec2,
    ) {
        // The viewport was opened on the line above, so this pass's id
        // map already holds its node — the handle the driver needs to
        // reach `scroll_content`.
        let content = ui.forest.current_node(scroll_id);
        let def_id = ui.forest.push_scrollbars_def(ScrollBarsDef {
            content,
            offset: state.offset,
            zoom: state.zoom,
            pan,
            reserve_y: geom.space.reserve_y,
            reserve_x: geom.space.reserve_x,
            padding: geom.padding,
            bar_width: self.theme.width,
            min_thumb: self.theme.min_thumb_px,
        });
        let overlay = Node::scroll_bars(def_id)
            .id(scroll_id.with("__bars"))
            .size((Sizing::FILL, Sizing::FILL));
        ui.widget(overlay).record(ui, None, |ui| {
            for (_, bar) in self.axes() {
                bar.record(ui, &self.theme);
            }
        });
    }
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
/// `Scroll`** — the destructure below binds every field with no `..`, so
/// adding one to `Node` fails to compile here, forcing the decision
/// whether it lands on `outer` (sizing/placement) or `inner`
/// (layout/panel knobs).
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
        // Re-derived by `Scroll::show` once the wrappers exist: it copies
        // `clip` from the user node onto `inner` and replaces `transform`
        // with the pan offset. Named rather than elided — a `..` here would
        // let a newly added `Node` field vanish silently, which is exactly
        // what this destructure exists to prevent.
        clip: _,
        transform: _,
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

    /// Route this frame's wheel / trackpad / pinch input over the
    /// viewport into a pan delta and a zoom step.
    ///
    /// The wheel does one or the other, never both: when the configured
    /// modifier matches, its notches become a multiplicative zoom factor
    /// and the pan is suppressed for that frame. The notch count already
    /// folds classic-wheel lines and touchpad pixels together (via the
    /// theme's line height), so ctrl held over a touchpad
    /// pinch-via-scroll zooms at the rate it would have panned. Positive
    /// `notches.y` is scroll-down, which by convention zooms *out*
    /// (factor < 1).
    ///
    /// `pivot` — the point that stays fixed across the step, in
    /// widget-local coords — resolves only when a step actually lands.
    /// It falls back to the viewport centre when the pointer is off the
    /// widget, and on the first frame where there is no rect yet, so the
    /// zoom still *feels* anchored before pointer tracking kicks in.
    fn read_input(&self, ui: &Ui, response: &ResponseState) -> ScrollInput {
        // Font-derived line step for wheel→pixel conversion. Pulls
        // `theme.text` (the default font config) rather than scanning
        // children for a dominant font — that's a future polish; for
        // now the active theme's text size is a good proxy and stays
        // consistent with what the user is reading.
        let line_px = ui.theme.text.line_height_for(ui.theme.text.font_size_px);
        let scroll = response.scroll;
        let pan_raw = scroll.pixels + scroll.lines * line_px;
        let notches = scroll.lines + scroll.pixels / line_px.max(f32::EPSILON);
        // Gate on `mods.ctrl` only — Ctrl is the zoom modifier on every
        // platform (macOS Cmd not honored), and `alt`-wheel shouldn't
        // zoom.
        let mods = ui.peek_modifiers();
        let wheel_zooms = self.zoom.as_ref().is_some_and(|cfg| match cfg.modifier {
            ZoomModifier::Ctrl => mods.ctrl,
            ZoomModifier::Always => true,
            ZoomModifier::PinchOnly => false,
        });
        let (pan_delta, wheel_factor) = match self.zoom.as_ref().filter(|_| wheel_zooms) {
            Some(cfg) => (Vec2::ZERO, zoom::from_wheel(cfg.step, notches.y)),
            None => (pan_raw, 1.0_f32),
        };
        let zoom_delta = zoom::combine(scroll.zoom, wheel_factor);

        let centre = response
            .layout_rect
            .map(|r| Vec2::new(r.size.w * 0.5, r.size.h * 0.5));
        let pivot = ((zoom_delta - 1.0).abs() > f32::EPSILON)
            .then(
                || match self.zoom.as_ref().map_or(ZoomPivot::Pointer, |c| c.pivot) {
                    ZoomPivot::Pointer => response.pointer_local.or(centre),
                    ZoomPivot::Center => centre,
                },
            )
            .flatten();

        ScrollInput {
            pan_delta,
            zoom_delta,
            pivot,
        }
    }

    /// Last frame's measurements, in the shape every later step reads
    /// them: the content extent, the gutter the bars reserve, and the
    /// offset bounds that follow from both.
    fn measure(
        &self,
        ui: &Ui,
        scroll_id: WidgetId,
        pan: BVec2,
        response: &ResponseState,
    ) -> ScrollGeometry {
        let outer = response.layout_rect.map_or(Size::ZERO, |r| r.size);
        let content = previous_scroll_content(ui, scroll_id);
        let padding = self.node.padding.unwrap_or(Spacing::ZERO);
        let space = bar_space(outer, pan, padding, &ui.theme.scrollbar, self.bar_mode);
        ScrollGeometry {
            content,
            padding,
            space,
            bounds: ScrollBounds {
                content,
                viewport: space.bar_viewport,
                content_margin: self.content_margin,
            },
        }
    }

    /// Fold one frame of routed input into the retained offset and zoom.
    ///
    /// Order is load-bearing: the pivot-anchored zoom step moves the
    /// offset, so it runs before the pan. The settled clamp then applies
    /// only to a non-zoomable scroll — a zoomable one keeps the
    /// out-of-range drift the pivot path composes against.
    fn apply_input(
        &self,
        state: &mut ScrollState,
        input: ScrollInput,
        geom: ScrollGeometry,
        pan: BVec2,
    ) {
        if let (Some(cfg), Some(pivot)) = (self.zoom.as_ref(), input.pivot) {
            state.apply_zoom(
                *cfg.range.start(),
                *cfg.range.end(),
                pivot,
                input.zoom_delta,
            );
        }
        let preserve_zoom_underflow = self.zoom.is_some();
        state.apply_wheel_pan(
            geom.bounds,
            pan.x,
            pan.y,
            input.pan_delta,
            preserve_zoom_underflow,
        );
        if !preserve_zoom_underflow {
            state.clamp_to_natural(geom.bounds);
        }
    }

    /// The outer/inner pair actually recorded.
    ///
    /// Outer is a bare ZStack holding the inner viewport plus the bar
    /// overlay. The reservation gutter lives on `inner.margin` — not on
    /// outer's padding — so the overlay, a sibling of inner under the
    /// same ZStack, can reach into the gutter strip with absolute
    /// positions.
    ///
    /// `scroll_wrappers` routes the *static* half: which user field
    /// lands on which wrapper. Everything patched here is per-frame —
    /// the fit bits the user's `Sizing` implies, the viewport id, the
    /// reservation margin, the clip read back off the user node, and the
    /// pan/zoom transform.
    fn wrappers(
        &self,
        scroll_id: WidgetId,
        pan: BVec2,
        space: BarSpace,
        state: ScrollState,
    ) -> ScrollWrappers {
        let ScrollWrappers { outer, inner } = scroll_wrappers(self.node);

        // Inner viewport owns the clip, the pan transform, the user-set
        // padding (encoder deflates the clip mask by it), and the
        // `Scroll` layout mode that runs children with INF on panned
        // axes. ZStack arrange deflates `Sizing::fill` by margin, so
        // inner's rendered rect = outer.rect minus the reserved strip on
        // the cross axes.
        //
        // Encode the user's per-axis `Sizing` into the viewport's fit
        // bits: a `Hug` panned axis makes the driver report its content
        // extent, so the scroll sizes to content like any other `Hug`
        // widget (bounded by `max_size`/available, scrolling past the
        // cap); `Fill`/`Fixed` keep the content-independent viewport.
        let user = self.node.size.unwrap_or_default();
        let fit = BVec2::new(pan.x && user.w().is_hug(), pan.y && user.h().is_hug());
        let mut inner = inner.id(scroll_id);
        inner.set_scroll_spec(self.node.scroll_spec().with_fit(fit));
        inner.margin = Some(Spacing::new(0.0, 0.0, space.reserve_y, space.reserve_x));
        // `with_axes` set `ClipMode::Rect` by default; caller configuration
        // can replace it with rounded clipping or no clipping.
        inner.clip = self.node.clip;
        // Raw pan/zoom — cascade anchors the scale at the inner's own
        // `layout_rect.min` (`TranslateScale::anchored_at`), so we
        // don't pre-bake the origin compensation. Translation is just
        // the user's scroll offset, negated (scroll right shifts
        // content left).
        if state.offset != Vec2::ZERO || (state.zoom - 1.0).abs() > f32::EPSILON {
            inner.transform = TranslateScale::new(-state.offset, state.zoom);
        }
        ScrollWrappers { outer, inner }
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
        // Input routes by `Sense::SCROLL`, which sits on the outer
        // ZStack, so wheel events over the bar gutter still pan the
        // viewport.
        let scroll_id = id.with("__viewport");

        // Everything read off `ui` immutably, before the state borrow.
        let response = ui.response_for(id);
        let geom = self.measure(ui, scroll_id, pan, &response);
        let input = self.read_input(ui, &response);
        let bars = (self.bar_mode != BarMode::Hidden).then(|| Bars::read(ui, scroll_id));

        let state = {
            let state = ui.state_mut::<ScrollState>(id);
            self.apply_input(state, input, geom, pan);
            if let Some(bars) = &bars {
                bars.drive(state, geom, pan);
            }
            *state
        };

        let ScrollWrappers { outer, inner } = self.wrappers(scroll_id, pan, geom.space, state);
        let inner_chrome = self.chrome;
        widget.node = outer;
        let inner_value = widget.record(ui, None, |ui| {
            let inner_value = ui.widget(inner).record(ui, inner_chrome.as_ref(), body);
            if let Some(bars) = &bars {
                bars.record(ui, scroll_id, state, geom, pan);
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
