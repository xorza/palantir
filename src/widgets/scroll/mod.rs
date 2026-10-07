//! The scrolling viewport: the widget, its retained offset and zoom ([`state`]),
//! bar geometry and drag handling ([`bars`]), and zoom policy ([`zoom_config`]).
//!
//! Every frame resolves against the previous frame's arranged geometry
//! ([`ScrollGeometry`]).

pub(crate) mod bars;
pub(crate) mod state;
pub(crate) mod zoom_config;

use crate::input::interaction::response_state::ResponseState;
use crate::input::sense::Sense;
use crate::input::zoom_factor::ZoomFactor;
use crate::layout::drivers::scrollbars::bar_geometry::BarGeometry;
use crate::layout::drivers::scrollbars::scrollbars_def::ScrollbarsDef;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::scroll_axes::ScrollAxes;
use crate::primitives::layout::sizing::Sizing;
use crate::primitives::math::domain::{self, vec2};
use crate::primitives::paint::background::Background;
use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widget_core::configure::ConfigureWidget;
use crate::widget_core::response::{InnerResponse, Response};
use crate::widget_core::widget::Widget;
use crate::widgets::scroll::bars::{BarMode, Bars};
use crate::widgets::scroll::state::{ScrollBounds, ScrollState};
use crate::widgets::scroll::zoom_config::{ZoomConfig, ZoomModifier, ZoomPivot};
use crate::widgets::theme::scrollbar::ScrollbarTheme;
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
            offset: state.offset,
            zoom: state.zoom,
            ..self.bars
        }
    }

    fn content_inset(&self) -> Vec2 {
        let [left, top, _, _] = self.bars.padding.as_array();
        Vec2::new(left, top)
    }

    /// Where the content starts inside the outer box (past the gutter): the origin a
    /// widget-local pivot is measured from.
    fn content_origin(&self) -> Vec2 {
        let [left, top, _, _] = self.bars.reserve.as_array();
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
/// sense, visibility) and an inner viewport (`Scroll` layout, padding, panel knobs).
#[derive(Debug)]
struct ScrollWrappers {
    outer: Widget,
    inner: Widget,
}

impl ScrollWrappers {
    /// Split a user `Scroll` into outer/inner wrappers. A new authoring field needs
    /// a side here. `Scroll::show` patches the per-frame inner fields and the ids.
    fn split(widget: &Widget, axes: ScrollAxes) -> Self {
        let mut outer = Widget::zstack()
            .sense(widget.authored_sense())
            .disabled(widget.authored_disabled())
            .focusable(widget.authored_focusable())
            .input_scope(widget.authored_input_scope());
        outer.configure().adopt_placement(widget);
        if let Some(size) = widget.authored_size() {
            outer.configure().size(size);
        }
        if let Some(min) = widget.authored_min_size() {
            outer.configure().min_size(min);
        }
        if let Some(max) = widget.authored_max_size() {
            outer.configure().max_size(max);
        }

        let mut inner = Widget::scroll(axes)
            .size((Sizing::FILL, Sizing::FILL))
            .justify(widget.authored_justify())
            .child_align(widget.authored_child_align());
        if let Some(padding) = widget.authored_padding() {
            inner.configure().padding(padding);
        }
        if let Some(gap) = widget.authored_gap() {
            inner.configure().gap(gap);
        }
        if let Some(line_gap) = widget.authored_line_gap() {
            inner.configure().line_gap(line_gap);
        }
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
    chrome: Option<Background>,
    bar_mode: BarMode,
    content_margin: Spacing,
    pan_request: Vec2,
    zoom_request: ZoomFactor,
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
        for edge in m.as_array() {
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
    /// `pivot` resolves only when a step lands, falling back to the viewport centre
    /// when the pointer is off the widget or there is no rect yet.
    fn read_input(&self, ui: &Ui, response: &ResponseState) -> ScrollInput {
        // Line step for wheel to pixel conversion, from the theme's default text size.
        let line_px = ui.theme().text.font().line_height;
        let scroll = response.scroll;
        let pan_raw = scroll.pan(line_px);
        // A theme with no line metric contributes no notches.
        let notches_per_px = domain::share_of(1.0, line_px);
        let notches = scroll.lines + scroll.pixels * notches_per_px;
        // Ctrl is the zoom modifier on every platform (macOS Cmd is not honored).
        let mods = ui.peek_modifiers();
        let wheel_zooms = self.zoom.as_ref().is_some_and(|cfg| match cfg.modifier {
            ZoomModifier::Ctrl => mods.ctrl,
            ZoomModifier::Always => true,
            ZoomModifier::PinchOnly => false,
        });
        let (pan_delta, wheel_factor) = match self.zoom.as_ref().filter(|_| wheel_zooms) {
            Some(cfg) => (Vec2::ZERO, ZoomFactor::from_wheel(cfg.step, notches.y)),
            None => (pan_raw, ZoomFactor::ONE),
        };
        // Authored requests join the pointer's; a zooming wheel leaves `pan_delta` zero.
        let pan_delta = pan_delta + self.pan_request;
        let zoom_delta = scroll.zoom.combine(wheel_factor).combine(self.zoom_request);

        let centre = response
            .layout_rect
            .map(|r| Vec2::new(r.size.w * 0.5, r.size.h * 0.5));
        let zoom_changed = !domain::is_approx_zero(zoom_delta.get() - 1.0);
        let pivot = zoom_changed
            .then(
                || match self.zoom.as_ref().map_or(ZoomPivot::Pointer, |c| c.pivot) {
                    ZoomPivot::Pointer => response.pointer_local.or(centre),
                    ZoomPivot::Center => centre,
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

    fn measure(
        &self,
        ui: &Ui,
        id: WidgetId,
        scroll_id: WidgetId,
        response: &ResponseState,
    ) -> ScrollGeometry {
        let theme = self.bars_theme(ui);
        let start = ui.state::<ScrollState>(id).copied().unwrap_or_default();
        ScrollGeometry {
            outer: response.layout_rect.map_or(Size::ZERO, |r| r.size),
            content: ui.scroll_content(scroll_id),
            content_margin: self.content_margin,
            bars: ScrollbarsDef {
                content: scroll_id,
                offset: start.offset,
                zoom: start.zoom,
                axes: self.axes,
                reserve: self.bar_mode.gutter(self.axes, theme),
                padding: self.widget.authored_padding().unwrap_or(Spacing::ZERO),
                bar_thickness: theme.thickness,
                min_thumb: theme.min_thumb,
            },
        }
    }

    /// Fold one frame of routed input into the retained offset and zoom. Order
    /// matters: the pivot-anchored zoom moves the offset, so it runs before the pan,
    /// and the settled clamp applies only to a non-zoomable scroll.
    fn apply_input(&self, state: &mut ScrollState, input: ScrollInput, geom: ScrollGeometry) {
        if let (Some(cfg), Some(pivot)) = (self.zoom.as_ref(), input.pivot) {
            state.apply_zoom(
                *cfg.range.start(),
                *cfg.range.end(),
                pivot - geom.content_origin(),
                input.zoom_delta,
            );
        }
        let preserve_zoom_underflow = self.zoom.is_some();
        state.apply_wheel_pan(
            geom.bounds(),
            self.axes.pans(Axis::X),
            self.axes.pans(Axis::Y),
            input.pan_delta,
            preserve_zoom_underflow,
        );
        if !preserve_zoom_underflow {
            state.clamp_to_natural(geom.bounds());
        }
    }

    /// The outer/inner pair actually recorded. The gutter lives on `inner.margin`, not
    /// outer's padding, so the overlay can reach into it. [`ScrollWrappers::split`]
    /// routes the static half; this adds the per-frame half.
    fn wrappers(
        &self,
        scroll_id: WidgetId,
        geom: ScrollGeometry,
        state: ScrollState,
    ) -> ScrollWrappers {
        // Inner owns the clip, pan transform, padding and `Scroll` layout mode. A `Hug`
        // panned axis sets a fit bit so the scroll sizes to its content (capped by
        // `max_size`/available); `Fill`/`Fixed` stay content-independent.
        let user = self.widget.authored_size().unwrap_or_default();
        let axes = self.axes.fit_content(user.w().is_hug(), user.h().is_hug());
        let ScrollWrappers { outer, inner } = ScrollWrappers::split(&self.widget, axes);
        let mut inner = inner.id(scroll_id);
        inner
            .configure()
            .margin(geom.bars.reserve)
            .transform(state.transform(geom.content_inset()));
        // `with_axes` set `ClipMode::Rect`; nothing unsets it, so `None` never runs.
        if let Some(clip) = self.widget.authored_clip() {
            inner.configure().clip(clip);
        }
        ScrollWrappers { outer, inner }
    }

    /// Record the viewport, its `body`, and the scrollbars. The response is for the
    /// caller's node, wrapping both surface and bars.
    pub fn show<R>(mut self, ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> InnerResponse<'_, R> {
        // The caller's salt names the outer wrapper but the arriving node is the
        // viewport, and the wrappers need the id first: resolve here, outer takes it later.
        let id = self.widget.resolve(ui);
        // Input routes by `Sense::SCROLL` on the outer ZStack, so wheel over the gutter pans.
        let scroll_id = id.with("viewport");

        let response = ui.response_for(id);
        let geom = self.measure(ui, id, scroll_id, &response);
        let input = self.read_input(ui, &response);
        let bars = (self.bar_mode != BarMode::Hidden)
            .then(|| Bars::read(ui, scroll_id, self.bars_theme(ui)));

        let state = ui.with_state::<ScrollState, _>(id, |_, state| {
            self.apply_input(state, input, geom);
            if let Some(bars) = &bars {
                bars.drive(state, geom);
            }
            *state
        });
        // The wheel senses only pannable axes; a zoomable viewport, or one with no
        // arranged box yet, keeps both.
        let pan_x = self.axes.pans(Axis::X);
        let pan_y = self.axes.pans(Axis::Y);
        let pans = if self.zoom.is_some() {
            Sense::SCROLL
        } else if response.layout_rect.is_none() {
            let mut declared = Sense::NONE;
            declared.set(Sense::SCROLL_X, pan_x);
            declared.set(Sense::SCROLL_Y, pan_y);
            declared
        } else {
            state.wheel_sense(geom.bounds(), pan_x, pan_y)
        };
        let sense = self.widget.authored_sense();
        self.widget
            .configure()
            .sense(sense.difference(Sense::SCROLL.difference(pans)));

        let ScrollWrappers { outer, inner } = self.wrappers(scroll_id, geom, state);
        let inner_chrome = self.chrome;
        let inner_value = outer.id(id).record(ui, None, |ui| {
            let inner_value = inner.record(ui, inner_chrome.as_ref(), body);
            if let Some(bars) = &bars {
                bars.record(ui, geom.bars_at(&state));
            }
            inner_value
        });

        InnerResponse {
            // Eager: the caller usually reads a field, and only `focused` can change in the body.
            response: Response::new(
                id,
                ui,
                ResponseState {
                    focused: ui.focus() == Some(id),
                    ..response
                },
            ),
            inner: inner_value,
        }
    }
}

impl Scroll<'_> {
    /// Paint `background` as the inner surface's chrome, under children and bars.
    /// Unlike `Panel`/`Grid`/`Popup`, unset does not fall back to
    /// `theme.panel_background`.
    ///
    /// # Panics
    ///
    /// Panics unless `background` holds the kinds [`Background`](crate::Background) lists.
    #[track_caller]
    pub const fn background(mut self, background: Background) -> Self {
        background.validate();
        self.chrome = Some(background);
        self
    }

    /// Paint `background` unless the caller set one. An explicit [`Self::background`]
    /// wins in either order.
    ///
    /// # Panics
    ///
    /// Panics unless `background` holds the kinds [`Background`](crate::Background) lists.
    #[track_caller]
    pub const fn default_background(mut self, background: Background) -> Self {
        background.validate();
        if self.chrome.is_none() {
            self.chrome = Some(background);
        }
        self
    }
}

impl Configure for Scroll<'_> {
    #[inline]
    fn configure(&mut self) -> ConfigureWidget<'_> {
        self.widget.configure()
    }
}

#[cfg(test)]
mod tests;
