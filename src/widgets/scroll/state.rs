//! Widget-owned scroll interaction state. Layout measurements enter each
//! input step as ephemeral [`ScrollBounds`], not retained state.

use crate::input::sense::Sense;
use crate::layout::drivers::scrollbars::bar_geometry::BarGeometry;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::layout::axis::Axis;
use crate::primitives::math::domain;
use glam::Vec2;

/// Where a viewport is scrolled to, and the interaction state that moves it.
///
/// Every viewport stores its offset here, `Scroll` and `TextEdit`'s text
/// viewport alike: the offset, the band [`Self::clamp_to_natural`] holds it
/// in, and the content transform are one implementation.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ScrollState {
    pub(crate) offset: Vec2,
    pub(super) zoom: f32,
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

#[derive(Clone, Copy, Debug)]
pub(super) struct TrackPage {
    pub(super) click_main: f32,
    pub(super) thumb_offset: f32,
    pub(super) thumb_size: f32,
    pub(super) page_step: f32,
    pub(super) domain: BarDomain,
}

impl TrackPage {
    /// A click at `click_main` along the track of the bar `thumb`; a page is
    /// one track length.
    pub(super) const fn at(thumb: BarGeometry, click_main: f32) -> Self {
        Self {
            click_main,
            thumb_offset: thumb.thumb_offset,
            thumb_size: thumb.thumb_size,
            page_step: thumb.track,
            domain: BarDomain::new(thumb.max_offset),
        }
    }
}

impl ScrollState {
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
        let [cml, cmt, cmr, cmb] = bounds.content_margin.as_array();
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
        let [cml, cmt, cmr, cmb] = bounds.content_margin.as_array();
        let leading = Vec2::new(-cml, -cmt) * self.zoom;
        let trailing = self.raw_overflow(bounds) + Vec2::new(cmr, cmb) * self.zoom;
        OffsetBounds {
            lo: leading.min(trailing),
            hi: leading.max(trailing),
        }
    }

    /// Scale by `zoom_delta`, clamped, holding the content point under
    /// `pivot` (measured from the content origin) still.
    pub(super) fn apply_zoom(
        &mut self,
        min_zoom: f32,
        max_zoom: f32,
        pivot: Vec2,
        zoom_delta: f32,
    ) {
        let new_zoom = (self.zoom * zoom_delta).clamp(min_zoom, max_zoom);
        let dz_eff = if self.zoom > 0.0 {
            new_zoom / self.zoom
        } else {
            1.0
        };
        if !domain::is_approx_zero(dz_eff - 1.0) {
            self.offset = (self.offset + pivot) * dz_eff - pivot;
            self.zoom = new_zoom;
        }
    }

    /// The wheel axes a viewport panning `pan_x` / `pan_y` can move along
    /// inside `bounds`: where content overflows at this zoom, or a content
    /// margin widens the range. An axis it cannot pan lets the wheel reach the
    /// container behind.
    pub(crate) fn wheel_sense(&self, bounds: ScrollBounds, pan_x: bool, pan_y: bool) -> Sense {
        let range = self.natural_bounds(bounds);
        let mut sense = Sense::NONE;
        sense.set(Sense::SCROLL_X, pan_x && range.hi.x > range.lo.x);
        sense.set(Sense::SCROLL_Y, pan_y && range.hi.y > range.lo.y);
        sense
    }

    pub(crate) fn apply_wheel_pan(
        &mut self,
        bounds: ScrollBounds,
        pan_x: bool,
        pan_y: bool,
        pan_delta: Vec2,
        preserve_zoom_underflow: bool,
    ) {
        let bounds = if preserve_zoom_underflow {
            self.zoom_rubber_band_bounds(bounds)
        } else {
            self.natural_bounds(bounds)
        };
        if pan_x && pan_delta.x != 0.0 {
            let lo = self.offset.x.min(bounds.lo.x);
            let hi = self.offset.x.max(bounds.hi.x);
            self.offset.x = (self.offset.x + pan_delta.x).clamp(lo, hi);
        }
        if pan_y && pan_delta.y != 0.0 {
            let lo = self.offset.y.min(bounds.lo.y);
            let hi = self.offset.y.max(bounds.hi.y);
            self.offset.y = (self.offset.y + pan_delta.y).clamp(lo, hi);
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

    pub(super) fn apply_thumb_drag(
        &mut self,
        axis: Axis,
        drag_started: bool,
        drag_delta: Option<Vec2>,
        travel: Option<ThumbTravel>,
    ) {
        if drag_started {
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
        let Some(delta) = drag_delta else {
            self.drag_anchor = None;
            return;
        };
        let Some(travel) = travel else {
            // The bar lost its geometry mid-drag. `drag_delta` is cumulative
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

    pub(super) fn apply_track_page(&mut self, axis: Axis, page: Option<TrackPage>) {
        let Some(page) = page else {
            return;
        };
        let current = axis.main_v(self.offset);
        // Both directions clamp through the bar domain so the thumb can follow.
        let next = if page.click_main < page.thumb_offset {
            page.domain.clamp(current - page.page_step)
        } else if page.click_main > page.thumb_offset + page.thumb_size {
            page.domain.clamp(current + page.page_step)
        } else {
            current
        };
        match axis {
            Axis::X => self.offset.x = next,
            Axis::Y => self.offset.y = next,
        }
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::widgets::scroll::state::ScrollState;

    impl ScrollState {
        pub(crate) fn drag_anchor_is_none(&self) -> bool {
            self.drag_anchor.is_none()
        }
    }
}
