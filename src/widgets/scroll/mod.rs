//! The scrolling viewport: the widget, its retained offset and zoom ([`scroll_state`]),
//! bar geometry and drag handling ([`bars`]), and zoom policy ([`zoom_config`]).
//!
//! Every frame resolves against the previous frame's arranged geometry
//! ([`ScrollGeometry`]).

pub(crate) mod bars;
pub(crate) mod scroll_state;
pub(crate) mod zoom_config;

use crate::input::interaction::response_state::ResponseState;
use crate::input::sense::Sense;
use crate::input::zoom_factor::ZoomFactor;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::scroll_axes::ScrollAxes;
use crate::primitives::layout::sizing::{SizeSpec, Sizing};
use crate::primitives::math::domain::{self, vec2};
use crate::primitives::paint::chrome::Chrome;
use crate::scene::bar_geometry::BarGeometry;
use crate::scene::node::Node;
use crate::scene::scrollbars_def::ScrollbarsDef;
use crate::theme::scrollbar::ScrollbarTheme;
use crate::ui::Ui;
use crate::widget_core::configurator::Configurator;
use crate::widget_core::configure::Configure;
use crate::widget_core::response::{InnerResponse, Response, ResponseSnapshot};
use crate::widget_core::widget::Widget;
use crate::widgets::scroll::bars::{BarMode, Bars};
use crate::widgets::scroll::scroll_state::{PanBand, ScrollBounds, ScrollState, WheelPan};
use crate::widgets::scroll::zoom_config::{ZoomConfig, ZoomModifier, ZoomPivot};
use glam::Vec2;

/// What one scroll frame resolves against, read from last frame's layout before
/// any input applies, so pan, zoom and both bars agree on the box.
#[derive(Copy, Clone, Debug)]
struct ScrollGeometry {
    outer: Size,
    content: Size,
    content_margin: Spacing,
    /// The bar overlay's definition at the frame's starting offset and zoom; its
    /// gutter and the user's padding deflate the viewport.
    bars: ScrollbarsDef,
}

impl ScrollGeometry {
    fn bounds(&self) -> ScrollBounds {
        ScrollBounds {
            content: self.content,
            viewport: self.bars.viewport(self.outer),
            content_margin: self.content_margin,
        }
    }

    const fn bars_at(&self, state: &ScrollState) -> ScrollbarsDef {
        ScrollbarsDef {
            offset: state.offset(),
            zoom: state.zoom(),
            ..self.bars
        }
    }

    fn content_inset(&self) -> Vec2 {
        let [left, top, _, _] = self.bars.padding.to_array();
        Vec2::new(left, top)
    }

    /// Where the content starts inside the outer box (past the gutter): the origin a
    /// widget-local pivot is measured from.
    fn content_origin(&self) -> Vec2 {
        let [left, top, _, _] = self.bars.reserve.to_array();
        self.content_inset() + Vec2::new(left, top)
    }

    fn thumb(&self, axis: Axis, state: &ScrollState) -> Option<BarGeometry> {
        self.bars_at(state).thumb(axis, self.outer, self.content)
    }
}

#[derive(Copy, Clone, Debug)]
struct ScrollInput {
    pan_delta: Vec2,
    zoom_delta: f32,
    /// Widget-local point fixed across the zoom step, when one resolves.
    pivot: Option<Vec2>,
}

/// The two `Widget`s a `Scroll` records: an outer `ZStack` (sizing, placement,
/// interaction flags) and an inner viewport (`Scroll` layout, padding, clip, panel knobs).
#[derive(Debug)]
struct ScrollWrappers {
    outer: Widget,
    inner: Widget,
}

impl ScrollWrappers {
    /// Split a user `Scroll` into outer/inner wrappers. The destructure is exhaustive,
    /// so a new `Node` field must be given a side. `Scroll::show` patches the
    /// per-frame inner fields and the ids.
    #[track_caller]
    fn split(widget: &Widget, axes: ScrollAxes) -> Self {
        let Node {
            mode: _,
            size,
            min_size,
            max_size,
            padding,
            margin,
            clip,
            gaps,
            justify,
            align,
            child_align,
            pos,
            grid,
            // Interaction only: an authored clip stays in `clip` until `Node::cols`.
            flags,
            visibility,
            transform,
            tab_idx,
        } = widget.node;
        assert!(
            transform == TranslateScale::IDENTITY,
            "a `Scroll` owns its descendants' transform: transform a parent instead",
        );

        let mut outer = Widget::zstack();
        outer.node = Node {
            size,
            min_size,
            max_size,
            margin,
            align,
            pos,
            grid,
            flags,
            visibility,
            tab_idx,
            ..outer.node
        };
        let mut inner = Widget::scroll(axes);
        inner.node = Node {
            size: Some(SizeSpec::new(Sizing::FILL, Sizing::FILL)),
            padding,
            clip,
            gaps,
            justify,
            child_align,
            ..inner.node
        };
        Self { outer, inner }
    }
}

/// Scroll viewport, built by [`Scroll::vertical`], [`Scroll::horizontal`] or
/// [`Scroll::both`].
///
/// Panned axes measure as `INF` so children report their natural extent. Wheel
/// input pans via a record-time `transform` using the previous frame's clamp. The
/// viewport senses the wheel only on axes its content overflows, so the other
/// axis reaches the container behind it; a [`zoomable`](Self::zoomable) one senses
/// both. Bar placement is chosen via [`BarMode`].
///
/// **A scroll owns its descendants' transform**, which is its pan and zoom. An
/// authored [`transform`](Configure::transform) is a contract violation: transform
/// a parent instead.
///
/// **In a stack, a scroll gives way to its siblings.** A `Hug` scroll can shrink to
/// nothing on its panned axis. A `Fill` scroll also grows into space no sibling
/// wants, except in a `Hug` stack, where it is empty.
#[derive(Debug)]
#[must_use = "a widget records nothing until `show`"]
pub struct Scroll<'a> {
    widget: Widget,
    axes: ScrollAxes,
    style: Option<&'a ScrollbarTheme>,
    zoom: Option<ZoomConfig>,
    chrome: Option<Chrome>,
    bar_mode: BarMode,
    content_margin: Spacing,
    pan_request: Vec2,
    zoom_request: ZoomFactor,
    reveal: Option<WidgetId>,
}

impl<'a> Scroll<'a> {
    /// Scrolls up and down only.
    #[track_caller]
    pub fn vertical() -> Self {
        Self::with_axes(ScrollAxes::VERTICAL)
    }

    /// Scrolls left and right only.
    #[track_caller]
    pub fn horizontal() -> Self {
        Self::with_axes(ScrollAxes::HORIZONTAL)
    }

    /// Scrolls on both axes; required by [`Self::zoomable`].
    #[track_caller]
    pub fn both() -> Self {
        Self::with_axes(ScrollAxes::BOTH)
    }

    #[track_caller]
    fn with_axes(axes: ScrollAxes) -> Self {
        Self {
            widget: Widget::scroll(axes).sense(Sense::SCROLL).clip_rect(),
            axes,
            style: None,
            zoom: None,
            chrome: None,
            bar_mode: BarMode::Reserved,
            content_margin: Spacing::default(),
            pan_request: Vec2::ZERO,
            zoom_request: ZoomFactor::ONE,
            reveal: None,
        }
    }

    /// Pan the viewport by `delta` logical pixels on the frame this is shown, through
    /// the wheel's clamp and rubber band. Reads no pointer or hover, and composes with
    /// a same-frame wheel and further calls.
    ///
    /// # Panics
    ///
    /// Panics unless both axes of `delta` are [offsets](crate::widget::domain::offset).
    #[track_caller]
    pub fn pan_by(mut self, delta: Vec2) -> Self {
        self.pan_request += vec2::offset(delta);
        self
    }

    /// Pan, on the frame this is shown, by the least distance that brings `child`'s last-frame
    /// rect into the viewport on each panned axis, as the web's `scrollIntoView({block:
    /// "nearest"})` and Qt's `ensureWidgetVisible` do: none while it is in view, and its start
    /// at the viewport's start when it is larger. Goes through [`Self::pan_by`]'s clamp, and
    /// does nothing before `child` has a rect.
    pub const fn reveal(mut self, child: WidgetId) -> Self {
        self.reveal = Some(child);
        self
    }

    /// Multiply the zoom by `factor` on the frame this is shown, through the wheel's
    /// clamp and [`ZoomPivot`]. Ignored without [`Self::zoomable`]; calls compose.
    ///
    /// # Panics
    ///
    /// Panics unless `factor` is [positive](crate::widget::domain::positive).
    #[track_caller]
    pub fn zoom_by(mut self, factor: f32) -> Self {
        let factor =
            ZoomFactor::new(domain::positive(factor)).expect("a positive value is a zoom factor");
        self.zoom_request = self.zoom_request.combine(factor);
        self
    }

    /// Per-instance override of [`crate::Theme`]'s `scrollbar`.
    pub fn style(mut self, s: impl Into<Option<&'a ScrollbarTheme>>) -> Self {
        self.style = s.into();
        self
    }

    /// Set the scrollbar layout mode; see [`BarMode`].
    pub const fn bar_mode(mut self, mode: BarMode) -> Self {
        self.bar_mode = mode;
        self
    }

    /// [`BarMode::Overlay`]: bars paint over content and reserve no gutter.
    pub const fn overlay_bars(self) -> Self {
        self.bar_mode(BarMode::Overlay)
    }

    /// [`BarMode::Hidden`]: no bars; pan, wheel and zoom still work.
    pub const fn hide_bars(self) -> Self {
        self.bar_mode(BarMode::Hidden)
    }

    /// Extends the offset clamp on each side without changing the recorded `content`
    /// size: invisible overscroll, no gutter. `left`/`top` open a negative-offset
    /// band, `right`/`bottom` the positive one.
    ///
    /// # Panics
    ///
    /// Panics unless every edge is a [length](crate::widget::domain::length).
    #[track_caller]
    pub fn content_margin(mut self, m: impl Into<Spacing>) -> Self {
        let m = m.into();
        for edge in m.to_array() {
            domain::length(edge);
        }
        self.content_margin = m;
        self
    }

    /// Let the viewport zoom under a default [`ZoomConfig`]; shorthand for
    /// [`Self::zoom_config`].
    ///
    /// # Panics
    ///
    /// As [`Self::zoom_config`].
    #[track_caller]
    pub fn zoomable(self) -> Self {
        self.zoom_config(ZoomConfig::default())
    }

    /// Let the viewport zoom under `config`'s range, step, modifier and pivot.
    ///
    /// # Panics
    ///
    /// Panics unless the scroll pans on both axes ([`Scroll::both`]): content escaping
    /// across a single axis has no way back.
    #[track_caller]
    pub fn zoom_config(mut self, config: ZoomConfig) -> Self {
        assert!(
            self.axes.pans(Axis::X) && self.axes.pans(Axis::Y),
            "a zoomable scroll must pan on both axes",
        );
        self.zoom = Some(config);
        self.add_sense(Sense::PINCH)
    }

    /// Route this frame's wheel / trackpad / pinch input into a pan delta and zoom
    /// step. The wheel pans or zooms, never both: a matching modifier turns notches
    /// into a zoom factor (positive `notches.y` zooms out) and suppresses the pan.
    /// `pivot` resolves only when a step lands, falling back to the viewport center
    /// when the pointer is off the widget or there is no rect yet.
    fn read_input(&self, ui: &Ui, response: &ResponseState) -> ScrollInput {
        // Line step for wheel to pixel conversion, from the theme's default text size.
        let line_px = ui.theme().text.font().line_height;
        let scroll = response.scroll;
        let pan_raw = scroll.pan(line_px);
        // A theme with no line metric contributes no notches.
        let notches_per_px = domain::share_of(1.0, line_px);
        let notches = scroll.lines + scroll.pixels * notches_per_px;
        // `ctrl` is the primary command modifier: Cmd on macOS, Ctrl elsewhere.
        let mods = ui.peek_modifiers();
        let wheel_zooms = self.zoom.is_some_and(|cfg| match cfg.modifier() {
            ZoomModifier::Ctrl => mods.ctrl,
            ZoomModifier::Always => true,
            ZoomModifier::PinchOnly => false,
        });
        let (pan_delta, wheel_factor) = match self.zoom.filter(|_| wheel_zooms) {
            Some(cfg) => (Vec2::ZERO, ZoomFactor::from_wheel(cfg.step(), notches.y)),
            None => (pan_raw, ZoomFactor::ONE),
        };
        // Authored requests join the pointer's; a zooming wheel leaves `pan_delta` zero.
        let pan_delta = pan_delta + self.pan_request;
        let zoom_delta = scroll.zoom.combine(wheel_factor).combine(self.zoom_request);

        let center = response
            .layout_rect
            .map(|r| Vec2::new(r.size.w * 0.5, r.size.h * 0.5));
        let zoom_changed = !domain::is_approx_zero(zoom_delta.get() - 1.0);
        let pivot = zoom_changed
            .then(
                || match self.zoom.map_or(ZoomPivot::Pointer, ZoomConfig::pivot) {
                    ZoomPivot::Pointer => response.pointer_local.or(center),
                    ZoomPivot::Center => center,
                },
            )
            .flatten();

        ScrollInput {
            pan_delta,
            zoom_delta: zoom_delta.get(),
            pivot,
        }
    }

    fn bars_theme<'u>(&'u self, ui: &'u Ui) -> &'u ScrollbarTheme {
        self.style.unwrap_or(&ui.theme().scrollbar)
    }

    /// The inner viewport's id, derived from the caller's node `id`.
    fn viewport_id(id: WidgetId) -> WidgetId {
        id.with("viewport")
    }

    fn measure(&self, ui: &Ui, id: WidgetId, response: &ResponseState) -> ScrollGeometry {
        let theme = self.bars_theme(ui);
        let start = ui.state::<ScrollState>(id).copied().unwrap_or_default();
        let viewport_id = Self::viewport_id(id);
        ScrollGeometry {
            outer: response.layout_rect.map_or(Size::ZERO, |r| r.size),
            content: ui.scroll_content(viewport_id),
            content_margin: self.content_margin,
            bars: ScrollbarsDef {
                content: viewport_id,
                offset: start.offset(),
                zoom: start.zoom(),
                axes: self.axes,
                reserve: self.bar_mode.gutter(self.axes, theme),
                padding: self.widget.authored_padding().unwrap_or(Spacing::ZERO),
                bar_thickness: theme.thickness,
                min_thumb: theme.min_thumb,
            },
        }
    }

    /// Fold one frame of routed input into the retained offset and zoom. Order
    /// matters: the stored zoom is brought inside this frame's config first, the
    /// pivot-anchored zoom moves the offset, so it runs before the pan, and the settled
    /// clamp applies only to a non-zoomable scroll.
    fn apply_input(&self, state: &mut ScrollState, input: ScrollInput, geometry: ScrollGeometry) {
        state.coerce_zoom(self.zoom);
        if let (Some(cfg), Some(pivot)) = (self.zoom, input.pivot) {
            state.apply_zoom(cfg, pivot - geometry.content_origin(), input.zoom_delta);
        }
        let band = if self.zoom.is_some() {
            PanBand::ZoomRubberBand
        } else {
            PanBand::Natural
        };
        state.apply_wheel_pan(WheelPan {
            bounds: geometry.bounds(),
            axes: self.axes,
            delta: input.pan_delta,
            band,
        });
        if band == PanBand::Natural {
            state.clamp_to_natural(geometry.bounds());
        }
    }

    /// The outer/inner pair actually recorded. The gutter lives on `inner.margin`, not
    /// outer's padding, so the overlay can reach into it. [`ScrollWrappers::split`]
    /// routes the static half; this adds the per-frame half.
    fn wrappers(
        &self,
        scroll_id: WidgetId,
        geometry: ScrollGeometry,
        state: ScrollState,
    ) -> ScrollWrappers {
        // Inner owns the clip, pan transform, padding and `Scroll` layout mode. A `Hug`
        // panned axis sets a fit bit so the scroll sizes to its content (capped by
        // `max_size`/available); `Fill`/`Fixed` stay content-independent.
        let user = self.widget.authored_size().unwrap_or_default();
        let axes = self.axes.fit_content(user);
        let ScrollWrappers { outer, inner } = ScrollWrappers::split(&self.widget, axes);
        let mut inner = inner.id(scroll_id);
        inner
            .configure()
            .margin(geometry.bars.reserve)
            .transform(state.transform(geometry.content_inset()));
        ScrollWrappers { outer, inner }
    }

    /// Record the viewport, its `body`, and the scrollbars. The response is for the
    /// caller's node, wrapping both surface and bars.
    ///
    /// # Panics
    ///
    /// Panics if the caller set a [`transform`](Configure::transform): a scroll owns its
    /// descendants' transform.
    #[track_caller]
    pub fn show<R>(mut self, ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> InnerResponse<'_, R> {
        // The caller's salt names the outer wrapper but the arriving node is the
        // viewport, and the wrappers need the id first: resolve here, outer takes it later.
        // Through `Widget::response`, so a scroll disabled this frame reads no wheel.
        let snapshot = self.widget.response(ui);
        let ResponseSnapshot {
            id,
            state: response,
        } = snapshot;
        // Input routes by `Sense::SCROLL` on the outer ZStack, so wheel over the gutter pans.
        let scroll_id = Self::viewport_id(id);

        let geometry = self.measure(ui, id, &response);
        if let Some(child) = self.reveal
            && let (Some(outer), Some(rect)) =
                (response.layout_rect, ui.response_for(child).layout_rect)
        {
            // Both rects are pre-transform, so the child's place in the content is its offset
            // from the content origin, scaled by the zoom the frame's offset is measured at.
            let ScrollbarsDef { offset, zoom, .. } = geometry.bars;
            let content = (rect.min - outer.min - geometry.content_origin()) * zoom;
            let viewport = geometry.bounds().viewport;
            let along = |axis: Axis| {
                if !self.axes.pans(axis) {
                    return 0.0;
                }
                reveal_pan(
                    axis.main_v(content),
                    axis.main(rect.size) * zoom,
                    axis.main_v(offset),
                    axis.main(viewport),
                )
            };
            self.pan_request += Vec2::new(along(Axis::X), along(Axis::Y));
        }
        let input = self.read_input(ui, &response);
        let bars = (self.bar_mode != BarMode::Hidden)
            .then(|| Bars::read(ui, &snapshot, self.bars_theme(ui)));

        let state = ui.with_state::<ScrollState, _>(id, |_, state| {
            self.apply_input(state, input, geometry);
            if let Some(bars) = &bars {
                bars.drive(state, geometry);
            }
            *state
        });
        // The wheel senses only pannable axes; a zoomable viewport, or one with no
        // arranged box yet, keeps both.
        let pans = if self.zoom.is_some() {
            Sense::SCROLL
        } else if response.layout_rect.is_none() {
            let mut declared = Sense::NONE;
            declared.set(Sense::SCROLL_X, self.axes.pans(Axis::X));
            declared.set(Sense::SCROLL_Y, self.axes.pans(Axis::Y));
            declared
        } else {
            state.wheel_sense(geometry.bounds(), self.axes)
        };
        let sense = self.widget.authored_sense();
        self.widget
            .configure()
            .sense(sense.difference(Sense::SCROLL.difference(pans)));

        let ScrollWrappers { outer, inner } = self.wrappers(scroll_id, geometry, state);
        let inner_chrome = self.chrome;
        let inner_value = outer.id(id).record(ui, None, |ui| {
            let inner_value = inner.record(ui, inner_chrome.as_ref(), body);
            if let Some(bars) = &bars {
                bars.record(ui, geometry.bars_at(&state));
            }
            inner_value
        });

        InnerResponse {
            // Eager: the caller usually reads a field, and only `focused` can change in the body.
            response: Response::new(
                ui,
                ResponseSnapshot {
                    id,
                    state: ResponseState {
                        focused: ui.focus() == Some(id),
                        ..response
                    },
                },
            ),
            inner: inner_value,
        }
    }
}

impl Scroll<'_> {
    /// Paint `chrome` as the inner surface's chrome, under children and bars.
    /// Unlike `Panel`/`Grid`/`Popup`, unset does not fall back to
    /// `theme.panel_chrome`.
    ///
    /// # Panics
    ///
    /// Panics unless `chrome` holds the kinds [`Chrome`] lists.
    #[track_caller]
    pub const fn chrome(mut self, chrome: Chrome) -> Self {
        chrome.validate();
        self.chrome = Some(chrome);
        self
    }

    /// Paint `chrome` unless the caller set one. An explicit [`Self::chrome`]
    /// wins in either order.
    ///
    /// # Panics
    ///
    /// Panics unless `chrome` holds the kinds [`Chrome`] lists.
    #[track_caller]
    pub const fn default_chrome(mut self, chrome: Chrome) -> Self {
        chrome.validate();
        if self.chrome.is_none() {
            self.chrome = Some(chrome);
        }
        self
    }
}

impl Scroll<'_> {
    /// Where the scroll with id `id` is panned to: the viewport's top-left in content
    /// pixels at its zoom. Zero before its first frame.
    pub fn offset(ui: &Ui, id: WidgetId) -> Vec2 {
        ui.state::<ScrollState>(id)
            .map_or(Vec2::ZERO, ScrollState::offset)
    }

    /// Pan the scroll with id `id` to `offset`, read as [`Self::offset`] reads it, from its
    /// next frame. A scroll that does not zoom clamps it into the range its content allows.
    ///
    /// # Panics
    ///
    /// Panics unless both axes of `offset` are [offsets](crate::widget::domain::offset).
    #[track_caller]
    pub fn set_offset(ui: &mut Ui, id: WidgetId, offset: Vec2) {
        let offset = vec2::offset(offset);
        ui.with_state::<ScrollState, _>(id, |_, state| state.set_offset(offset));
    }

    /// The zoom of the scroll with id `id`: one before its first frame, and for a scroll
    /// that does not zoom.
    pub fn zoom(ui: &Ui, id: WidgetId) -> f32 {
        ui.state::<ScrollState>(id).map_or(1.0, ScrollState::zoom)
    }

    /// Zoom the scroll with id `id` to `zoom`, read as [`Self::zoom`] reads it, holding the
    /// content point at the viewport's corner still. From its next frame the scroll clamps it
    /// into its [`ZoomConfig`]'s range, and one that does not zoom returns to 1.
    ///
    /// # Panics
    ///
    /// Panics unless `zoom` is [positive](crate::widget::domain::positive).
    #[track_caller]
    pub fn set_zoom(ui: &mut Ui, id: WidgetId, zoom: f32) {
        let zoom = domain::positive(zoom);
        ui.with_state::<ScrollState, _>(id, |_, state| state.set_zoom(zoom));
    }
}

/// The pan on one axis that brings a span starting at `start`, `len` long, into a view at
/// `offset`, `view` long: none while it is in view, and its start at the view's when it is
/// larger.
fn reveal_pan(start: f32, len: f32, offset: f32, view: f32) -> f32 {
    let end = start + len;
    if start < offset || len > view {
        start - offset
    } else if end > offset + view {
        end - (offset + view)
    } else {
        0.0
    }
}

impl Configure for Scroll<'_> {
    #[inline]
    fn configure(&mut self) -> Configurator<'_> {
        self.widget.configure()
    }
}

#[cfg(test)]
mod tests;
