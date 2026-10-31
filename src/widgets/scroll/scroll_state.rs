//! Widget-owned scroll interaction state. Layout measurements enter each
//! input step as ephemeral [`ScrollBounds`], not retained state.

use crate::input::interaction::drag::Drag;
use crate::input::sense::Sense;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::scroll_axes::ScrollAxes;
use crate::primitives::math::domain;
use crate::scene::bar_geometry::BarGeometry;
use crate::widgets::scroll::zoom_config::ZoomConfig;
use glam::Vec2;
use std::ops::Range;

/// Where a viewport is scrolled to, and the interaction state that moves it.
///
/// Every viewport stores its offset here, `Scroll` and `TextEdit`'s text
/// viewport alike: the offset, the band [`Self::clamp_to_natural`] holds it
/// in, and the content transform are one implementation.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ScrollState {
    offset: Vec2,
    zoom: f32,
    /// The live thumb drag's origin, or `None` between drags.
    drag_anchor: Option<DragAnchor>,
}

/// Where a live thumb drag started, so cumulative drag deltas compose against
/// a stable snapshot rather than the moving offset.
#[derive(Clone, Copy, Debug)]
struct DragAnchor {
    /// The one axis this drag drives.
    axis: Axis,
    /// The origin in the bar's domain (`[0, max_off]`), not the offset's.
    start: f32,
}

impl Default for ScrollState {
    fn default() -> Self {
        Self {
            offset: Vec2::ZERO,
            zoom: 1.0,
            drag_anchor: None,
        }
    }
}

/// The box an offset is solved in: content, viewport, and how far past either
/// edge the offset may roam.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ScrollBounds {
    pub(crate) content: Size,
    pub(crate) viewport: Size,
    pub(crate) content_margin: Spacing,
}

/// Which band a wheel pan may roam in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PanBand {
    /// The overflow floored at zero, widened by the content margin: the band a
    /// settled offset is clamped into.
    Natural,
    /// The raw endpoints a zoomable scroll pans between, keeping the underflow a
    /// pivot zoom leaves on content smaller than the viewport.
    ZoomRubberBand,
}

/// One frame's wheel pan: how far, along which axes, inside which band.
#[derive(Clone, Copy, Debug)]
pub(crate) struct WheelPan {
    pub(crate) bounds: ScrollBounds,
    pub(crate) axes: ScrollAxes,
    pub(crate) delta: Vec2,
    pub(crate) band: PanBand,
}

#[derive(Clone, Copy, Debug)]
struct OffsetBounds {
    lo: Vec2,
    hi: Vec2,
}

/// The offset range a scrollbar can express: `[0, max_off]`.
///
/// Narrower than the wheel's range: `content_margin` opens a band below zero
/// that a thumb does not show
/// ([`Scroll::content_margin`](crate::Scroll::content_margin)). Each
/// interaction path naming the ends itself is how a drag anchored in the
/// wheel's domain and clamped in the bar's goes unnoticed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct BarDomain {
    max_off: f32,
}

impl BarDomain {
    /// The range `[0, max_off]`.
    #[inline]
    pub(super) const fn new(max_off: f32) -> Self {
        Self { max_off }
    }

    /// Pull `offset` into the range. The one place either end is named.
    #[inline]
    pub(super) const fn clamp(self, offset: f32) -> f32 {
        offset.clamp(0.0, self.max_off)
    }
}

/// What a thumb drag needs from its bar's resolved geometry.
#[derive(Clone, Copy, Debug)]
pub(super) struct ThumbTravel {
    /// Content pixels per pixel of thumb travel.
    pub(super) factor: f32,
    /// The range the thumb can express, so the drag clamps through one definition.
    pub(super) domain: BarDomain,
}

impl ThumbTravel {
    /// The drag mapping for the bar `thumb`. The denominator is the travel of
    /// the geometry that placed the thumb, not the raw track, which differs
    /// by the track's floor.
    pub(super) const fn of(thumb: BarGeometry) -> Self {
        Self {
            factor: domain::share_of(thumb.max_offset, thumb.travel),
            domain: BarDomain::new(thumb.max_offset),
        }
    }
}

impl ScrollState {
    /// The viewport's top-left in content pixels at [`Self::zoom`].
    pub(crate) const fn offset(&self) -> Vec2 {
        self.offset
    }

    /// The content's scale; `1` for a viewport that does not zoom.
    pub(crate) const fn zoom(&self) -> f32 {
        self.zoom
    }

    /// Pan to `offset`, read as [`Self::offset`] reads it. Unclamped: a settled viewport
    /// clamps it on its next input step.
    pub(crate) const fn set_offset(&mut self, offset: Vec2) {
        self.offset = offset;
    }

    /// How far the content reaches past the viewport at this zoom, per axis,
    /// before any fits-in-viewport policy. Both bands start here.
    #[inline]
    fn raw_overflow(&self, bounds: ScrollBounds) -> Vec2 {
        let content = bounds.content.scaled_by(self.zoom);
        Vec2::new(content.w - bounds.viewport.w, content.h - bounds.viewport.h)
    }

    /// The offset range the wheel and settle clamp work in: the overflow,
    /// floored at zero first, then widened by `content_margin` each side.
    ///
    /// Flooring first keeps `lo <= 0 <= hi`, so the margin only widens the
    /// range. Taking `trailing.max(leading)` off the raw endpoints let the
    /// band collapse to `-left * zoom` once content fit, shoving it sideways
    /// by the margin.
    fn natural_bounds(&self, bounds: ScrollBounds) -> OffsetBounds {
        let [cml, cmt, cmr, cmb] = bounds.content_margin.to_array();
        let overflow = self.raw_overflow(bounds).max(Vec2::ZERO);
        OffsetBounds {
            lo: Vec2::new(-cml, -cmt) * self.zoom,
            hi: overflow + Vec2::new(cmr, cmb) * self.zoom,
        }
    }

    /// The wider band a zoomable scroll pans in, off the raw endpoints rather
    /// than [`Self::natural_bounds`]' floored ones. Pivot zoom may leave
    /// undersized content between the two, so the trailing end is not floored
    /// and the pair is taken as `min`/`max`: for content that fits, the band
    /// is the inverted interval, which `natural_bounds` must not inherit.
    fn zoom_rubber_band_bounds(&self, bounds: ScrollBounds) -> OffsetBounds {
        let [cml, cmt, cmr, cmb] = bounds.content_margin.to_array();
        let leading = Vec2::new(-cml, -cmt) * self.zoom;
        let trailing = self.raw_overflow(bounds) + Vec2::new(cmr, cmb) * self.zoom;
        OffsetBounds {
            lo: leading.min(trailing),
            hi: leading.max(trailing),
        }
    }

    /// Bring the stored zoom inside what `config` allows, `1` without one, holding the
    /// content point at the viewport's corner still. A zoom outlives the config it was
    /// set under: the caller can narrow the range or drop it between frames.
    pub(super) fn coerce_zoom(&mut self, config: Option<ZoomConfig>) {
        self.set_zoom(config.map_or(1.0, |config| config.clamp(self.zoom)));
    }

    /// Scale to `zoom`, holding the content point at the viewport's corner still.
    pub(crate) fn set_zoom(&mut self, zoom: f32) {
        debug_assert!(zoom > 0.0, "a zoom is positive: {zoom}");
        if zoom != self.zoom {
            self.offset *= zoom / self.zoom;
            self.zoom = zoom;
        }
    }

    /// Scale by `zoom_delta`, clamped into `config`'s range, holding the content point
    /// under `pivot` (measured from the content origin) still. The stored zoom is already
    /// inside that range ([`Self::coerce_zoom`]), so positive.
    pub(super) fn apply_zoom(&mut self, config: ZoomConfig, pivot: Vec2, zoom_delta: f32) {
        let new_zoom = config.clamp(self.zoom * zoom_delta);
        let dz_eff = new_zoom / self.zoom;
        if !domain::is_approx_zero(dz_eff - 1.0) {
            self.offset = (self.offset + pivot) * dz_eff - pivot;
            self.zoom = new_zoom;
        }
    }

    /// The wheel axes a viewport panning `axes` can move along inside
    /// `bounds`: where content overflows at this zoom, or a content margin
    /// widens the range. An axis it cannot pan lets the wheel reach the
    /// container behind.
    pub(crate) fn wheel_sense(&self, bounds: ScrollBounds, axes: ScrollAxes) -> Sense {
        let range = self.natural_bounds(bounds);
        let mut sense = Sense::NONE;
        sense.set(
            Sense::SCROLL_X,
            axes.pans(Axis::X) && range.hi.x > range.lo.x,
        );
        sense.set(
            Sense::SCROLL_Y,
            axes.pans(Axis::Y) && range.hi.y > range.lo.y,
        );
        sense
    }

    /// Move by `pan.delta` on each panned axis, clamped into `pan.band`. An
    /// offset already outside the band is not pulled in, only kept from going
    /// further out.
    pub(crate) fn apply_wheel_pan(&mut self, pan: WheelPan) {
        let bounds = match pan.band {
            PanBand::Natural => self.natural_bounds(pan.bounds),
            PanBand::ZoomRubberBand => self.zoom_rubber_band_bounds(pan.bounds),
        };
        if pan.axes.pans(Axis::X) && pan.delta.x != 0.0 {
            let lo = self.offset.x.min(bounds.lo.x);
            let hi = self.offset.x.max(bounds.hi.x);
            self.offset.x = (self.offset.x + pan.delta.x).clamp(lo, hi);
        }
        if pan.axes.pans(Axis::Y) && pan.delta.y != 0.0 {
            let lo = self.offset.y.min(bounds.lo.y);
            let hi = self.offset.y.max(bounds.hi.y);
            self.offset.y = (self.offset.y + pan.delta.y).clamp(lo, hi);
        }
    }

    /// Pan along `axis` by the least distance that brings `span` inside the `extent`
    /// the viewport shows from the offset; a span longer than the extent shows its start.
    pub(crate) fn reveal(&mut self, axis: Axis, span: Range<f32>, extent: f32) {
        let offset = match axis {
            Axis::X => &mut self.offset.x,
            Axis::Y => &mut self.offset.y,
        };
        if span.start < *offset {
            *offset = span.start;
        } else if span.end > *offset + extent {
            *offset = span.end - extent;
        }
    }

    pub(crate) fn clamp_to_natural(&mut self, bounds: ScrollBounds) {
        let bounds = self.natural_bounds(bounds);
        self.offset.x = self.offset.x.clamp(bounds.lo.x, bounds.hi.x);
        self.offset.y = self.offset.y.clamp(bounds.lo.y, bounds.hi.y);
    }

    /// The transform a viewport applies to its content: the offset negated, at
    /// the current zoom, scaled about `content_origin`. Cascade anchors scale
    /// at the node's `layout_rect.min`, but content starts inside the padding;
    /// scaling about the node's corner would grow that padding with zoom.
    /// Scaling about the content origin keeps offset 0 at the content's start.
    pub(crate) fn transform(&self, content_origin: Vec2) -> TranslateScale {
        TranslateScale::new(content_origin * (1.0 - self.zoom) - self.offset, self.zoom)
    }

    pub(super) fn apply_thumb_drag(&mut self, axis: Axis, drag: Drag, travel: Option<ThumbTravel>) {
        if drag.started() {
            // Projected into the bar domain: the thumb expresses only
            // `[0, max_off]`, so a raw offset below zero (in a `content_margin`
            // band) would spend the gesture's start climbing back to 0.
            let start = axis.main_v(self.offset);
            self.drag_anchor = Some(DragAnchor {
                axis,
                start: travel.map_or(start, |t| t.domain.clamp(start)),
            });
        }
        let Some(anchor) = self.drag_anchor.filter(|a| a.axis == axis) else {
            return;
        };
        let Some(delta) = drag.delta() else {
            self.drag_anchor = None;
            return;
        };
        let Some(travel) = travel else {
            // The bar lost its geometry mid-drag. A drag's delta is cumulative
            // from the press, so a resumed anchor would apply all the travel
            // at once; drop it and re-anchor on the next press.
            self.drag_anchor = None;
            return;
        };
        let target = anchor.start + axis.main_v(delta) * travel.factor;
        let clamped = travel.domain.clamp(target);
        match axis {
            Axis::X => self.offset.x = clamped,
            Axis::Y => self.offset.y = clamped,
        }
    }

    /// A click at `click_main` along the track of the bar `thumb` pages toward it by one
    /// track length; a click on the thumb does nothing.
    pub(super) fn apply_track_page(&mut self, axis: Axis, thumb: BarGeometry, click_main: f32) {
        let cur = axis.main_v(self.offset);
        // Both directions clamp through the bar domain so the thumb can follow.
        let domain = BarDomain::new(thumb.max_offset);
        let next = if click_main < thumb.thumb_offset {
            domain.clamp(cur - thumb.track)
        } else if click_main > thumb.thumb_offset + thumb.thumb_size {
            domain.clamp(cur + thumb.track)
        } else {
            cur
        };
        match axis {
            Axis::X => self.offset.x = next,
            Axis::Y => self.offset.y = next,
        }
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::widgets::scroll::scroll_state::ScrollState;

    impl ScrollState {
        pub(crate) fn drag_anchor_is_none(&self) -> bool {
            self.drag_anchor.is_none()
        }
    }
}
