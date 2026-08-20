pub(crate) mod bars;
pub(crate) mod state;
pub(crate) mod zoom_config;

use crate::input::response::ResponseState;
use crate::input::sense::Sense;
use crate::input::zoom;
use crate::layout::types::layout_mode::ScrollSpec;
use crate::layout::types::sizing::Sizing;
use crate::primitives::background::Background;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::{Configure, Node};
use crate::ui::Ui;
use crate::widgets::response::{InnerResponse, Response};
use crate::widgets::scroll::bars::{BarMode, BarSpace, Bars, bar_space};
use crate::widgets::scroll::state::{ScrollBounds, ScrollState};
use crate::widgets::scroll::zoom_config::{ZoomConfig, ZoomModifier, ZoomPivot};
use crate::widgets::theme::scrollbar::ScrollbarTheme;
use glam::{BVec2, Vec2};

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
    /// The builder's, carried so [`Self::bounds`] can hand the offset
    /// solver a whole [`ScrollBounds`].
    content_margin: Spacing,
}

impl ScrollGeometry {
    /// Last frame's measured content extent for this scroll, keyed by the
    /// **inner viewport** node because that is the `LayoutMode::Scroll`
    /// one. `Size::ZERO` until the node has been through one layout pass.
    ///
    /// Resolves through the cascade, so like `Ui::response_for` it answers
    /// for the previous frame — which is the lag `Scroll` wants: the bars
    /// describe the content the user is looking at.
    fn previous_content(ui: &Ui, scroll_id: WidgetId) -> Size {
        ui.cascade
            .endpoint(scroll_id)
            .map_or(Size::ZERO, |endpoint| ui.layout.scroll_content(endpoint))
    }

    /// Content extent at the current zoom. The bars measure against
    /// this rather than the raw extent so dragging a thumb inside a
    /// zoomed viewport tracks the cursor 1:1 with what's on screen.
    fn scaled_content(&self, zoom: f32) -> Size {
        Size::new(self.content.w * zoom, self.content.h * zoom)
    }

    /// What the offset solver works in, projected rather than stored.
    ///
    /// Both halves are already here — `content` is this struct's, and the
    /// viewport is the one the bars reserved — so holding a
    /// [`ScrollBounds`] beside them put the same two numbers in the
    /// struct twice, with nothing keeping the copies equal but the one
    /// construction site that set them together.
    fn bounds(&self) -> ScrollBounds {
        ScrollBounds {
            content: self.content,
            viewport: self.space.bar_viewport,
            content_margin: self.content_margin,
        }
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

/// The two wrapper `Node`s a `Scroll` records: an outer `ZStack`
/// that owns sizing / placement / sense / visibility and an inner
/// viewport that owns the `Scroll` layout mode, padding, and the panel
/// knobs (gap / justify / child_align).
#[derive(Debug)]
struct ScrollWrappers {
    outer: Node,
    inner: Node,
}

impl ScrollWrappers {
    /// Split a user `Scroll` node into its outer/inner wrappers.
    ///
    /// **This routes every `Node` field that should survive on a
    /// `Scroll`** — the destructure below binds every field with no `..`,
    /// so adding one to `Node` fails to compile here, forcing the decision
    /// whether it lands on `outer` (sizing/placement) or `inner`
    /// (layout/panel knobs).
    /// `Scroll::show` patches the remaining inner fields it computes per
    /// frame (`salt`, the reservation `margin`, layout fit flags,
    /// `clip` — read off `flags` before this runs — and the pan
    /// `transform`). The user salt stays on the `Widget` resolved in
    /// `Scroll::show`; neither wrapper carries it.
    fn split(node: Node) -> Self {
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
        Self { outer, inner }
    }
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
pub struct Scroll<'a> {
    node: Node,
    style: Option<&'a ScrollbarTheme>,
    zoom: Option<ZoomConfig>,
    chrome: Option<Background>,
    bar_mode: BarMode,
    content_margin: Spacing,
}

impl<'a> Scroll<'a> {
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

    #[track_caller]
    fn with_axes(spec: ScrollSpec) -> Self {
        Self {
            // Scroll requires clipping; default to `Rect` so callers that
            // don't override get the cheap scissor path. Callers can still
            // call `Configure::clip_rounded` to upgrade to a stencil mask.
            node: Node::scroll(spec).sense(Sense::SCROLL).clip_rect(),
            style: None,
            zoom: None,
            chrome: None,
            bar_mode: BarMode::Reserved,
            content_margin: Spacing::default(),
        }
    }

    style_setter!('a, ScrollbarTheme, scrollbar);

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
    pub fn hide_bars(self) -> Self {
        self.bar_mode(BarMode::Hidden)
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
        let text = &ui.theme().text;
        let line_px = text.line_height_for(text.font_size_px);
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

    /// This viewport's scrollbar bundle: the per-instance override if
    /// the caller set one, else the global slot.
    fn bars_theme<'u>(&'u self, ui: &'u Ui) -> &'u ScrollbarTheme {
        self.slot(ui.theme())
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
        let content = ScrollGeometry::previous_content(ui, scroll_id);
        let padding = self.node.padding.unwrap_or(Spacing::ZERO);
        let space = bar_space(outer, pan, padding, self.bars_theme(ui), self.bar_mode);
        ScrollGeometry {
            content,
            padding,
            space,
            content_margin: self.content_margin,
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
            geom.bounds(),
            pan.x,
            pan.y,
            input.pan_delta,
            preserve_zoom_underflow,
        );
        if !preserve_zoom_underflow {
            state.clamp_to_natural(geom.bounds());
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
    /// [`ScrollWrappers::split`] routes the *static* half: which user field
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
        let ScrollWrappers { outer, inner } = ScrollWrappers::split(self.node);

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
        // Raw pan/zoom, from the one place a viewport's transform is
        // derived — `TextEdit`'s text block reads the same method.
        inner.transform = state.transform();
        ScrollWrappers { outer, inner }
    }

    pub fn show<R>(self, ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> InnerResponse<'_, R> {
        // The caller's salt names the *outer wrapper*, but the node it
        // arrived on describes the viewport — `wrappers` splits it into
        // both, so neither is the node that came in, and the wrappers
        // can't be built until the id has unlocked this widget's state.
        // Identity resolves on its own; the outer wrapper stages onto it
        // below.
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
        let scroll_id = id.with("viewport");

        // Everything read off `ui` immutably, before the state borrow.
        let response = ui.response_for(id);
        let geom = self.measure(ui, scroll_id, pan, &response);
        let input = self.read_input(ui, &response);
        let bars = (self.bar_mode != BarMode::Hidden)
            .then(|| Bars::read(ui, scroll_id, self.bars_theme(ui)));

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

        InnerResponse {
            // Eager: the probe above already answered for this id, and the
            // caller almost always reads at least one field (drag delta,
            // scroll delta, hovered). Nothing the body records can move a
            // cascade or layout answer — both are frozen for the pass — so
            // the only field worth re-reading is `focused`, which the body
            // may have taken.
            response: Response::eager(
                id,
                ui,
                ResponseState {
                    focused: ui.focused_id() == Some(id),
                    ..response
                },
            ),
            inner: inner_value,
        }
    }
}

impl_background!(
    Scroll<'_>,
    "Chrome for the inner scroll surface — painted under the children, before \
     the scrollbar overlay. Unlike the other containers (`Panel`/`Grid`/`Popup`), \
     Scroll does **not** fall back to `theme.panel_background` when unset: an \
     unstyled scroll surface paints no background. Pass one explicitly to fill it.",
);
impl_configure!(Scroll<'_>);

#[cfg(test)]
mod tests;
