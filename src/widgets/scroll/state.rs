//! Widget-owned scroll interaction state. Layout measurements enter
//! each input step as ephemeral [`ScrollBounds`] rather than becoming
//! another retained widget-state copy.

use crate::layout::axis::Axis;
use crate::layout::scrollbars::BarDomain;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use glam::Vec2;

#[derive(Clone, Copy, Debug)]
pub(crate) struct ScrollState {
    pub(crate) offset: Vec2,
    pub(super) zoom: f32,
    /// Where the live thumb drag started, so cumulative drag deltas
    /// compose against a stable snapshot rather than the moving offset.
    ///
    /// Held **in the bar's own domain** (`[0, max_off]`), not the
    /// offset's, and only for the driven axis — that is all a thumb can
    /// express or move.
    drag_anchor: Option<(Axis, f32)>,
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

#[derive(Clone, Copy, Debug)]
pub(super) struct ScrollBounds {
    pub(super) content: Size,
    pub(super) viewport: Size,
    pub(super) content_margin: Spacing,
}

#[derive(Clone, Copy, Debug)]
struct OffsetBounds {
    lo: Vec2,
    hi: Vec2,
}

/// What a thumb drag needs from its bar's resolved geometry. Named for
/// the same reason [`TrackPage`] is: the two are siblings applied one
/// after the other, and an anonymous `(f32, f32)` here reads as nothing
/// at all by the time it is destructured.
#[derive(Clone, Copy, Debug)]
pub(super) struct ThumbTravel {
    /// Content pixels bought per pixel of thumb travel.
    pub(super) factor: f32,
    /// The range the thumb can express — carried rather than a bare
    /// `max_off`, so the drag clamps through one definition instead of
    /// naming `0.0` itself.
    pub(super) domain: BarDomain,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct TrackPage {
    pub(super) click_main: f32,
    pub(super) thumb_offset: f32,
    pub(super) thumb_size: f32,
    pub(super) page_step: f32,
    pub(super) domain: BarDomain,
}

impl ScrollState {
    /// The offset range the wheel and the settle clamp work in:
    /// the overflow, **floored at zero first**, then widened by
    /// `content_margin` on each side.
    ///
    /// The flooring is the whole point. Taking
    /// `trailing.max(leading)` off the raw endpoints instead let the
    /// trailing end fall *below* the leading one once content fit
    /// inside the viewport — the band collapsed to the single value
    /// `-left * zoom`, so a scroll with a leading margin shoved content
    /// that fitted sideways by exactly that margin and pinned it there.
    /// `content_margin` is documented as invisible overscroll that
    /// leaves child layout alone, so the margin may only ever *widen*
    /// this range, never move its resting point: flooring first keeps
    /// `lo <= 0 <= hi`, and content that fits stays at 0.
    fn natural_bounds(&self, bounds: ScrollBounds) -> OffsetBounds {
        let [cml, cmt, cmr, cmb] = bounds.content_margin.as_array();
        let overflow = Vec2::new(
            bounds.content.w * self.zoom - bounds.viewport.w,
            bounds.content.h * self.zoom - bounds.viewport.h,
        )
        .max(Vec2::ZERO);
        OffsetBounds {
            lo: Vec2::new(-cml, -cmt) * self.zoom,
            hi: overflow + Vec2::new(cmr, cmb) * self.zoom,
        }
    }

    /// The wider band a *zoomable* scroll pans in, off the **raw**
    /// endpoints rather than [`Self::natural_bounds`]' floored ones.
    ///
    /// Pivot zoom may legitimately leave undersized content between the
    /// two, so the trailing end is deliberately not floored at zero here
    /// and the pair is taken as `min`/`max` — for content that fits, the
    /// raw trailing end sits *below* the leading one and the band is the
    /// inverted interval between them. That inversion is exactly what
    /// `natural_bounds` must not inherit, which is why the two do their
    /// own arithmetic instead of sharing a helper.
    fn zoom_rubber_band_bounds(&self, bounds: ScrollBounds) -> OffsetBounds {
        let [cml, cmt, cmr, cmb] = bounds.content_margin.as_array();
        let leading = Vec2::new(-cml, -cmt) * self.zoom;
        let trailing = Vec2::new(
            bounds.content.w * self.zoom - bounds.viewport.w + cmr * self.zoom,
            bounds.content.h * self.zoom - bounds.viewport.h + cmb * self.zoom,
        );
        OffsetBounds {
            lo: leading.min(trailing),
            hi: leading.max(trailing),
        }
    }

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
        if (dz_eff - 1.0).abs() > f32::EPSILON {
            self.offset = (self.offset + pivot) * dz_eff - pivot;
            self.zoom = new_zoom;
        }
    }

    pub(super) fn apply_wheel_pan(
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

    pub(super) fn clamp_to_natural(&mut self, bounds: ScrollBounds) {
        let bounds = self.natural_bounds(bounds);
        self.offset.x = self.offset.x.clamp(bounds.lo.x, bounds.hi.x);
        self.offset.y = self.offset.y.clamp(bounds.lo.y, bounds.hi.y);
    }

    pub(super) fn apply_thumb_drag(
        &mut self,
        axis: Axis,
        drag_started: bool,
        drag_delta: Option<Vec2>,
        travel: Option<ThumbTravel>,
    ) {
        if drag_started {
            // Projected into the bar domain at snapshot time. The thumb
            // can only express `[0, max_off]`, so anchoring at a raw
            // offset — which may sit below zero inside a
            // `content_margin` leading band — spent the first
            // `-offset / factor` px of the gesture climbing back to 0
            // with the thumb not moving at all.
            let start = axis.main_v(self.offset);
            self.drag_anchor = Some((axis, travel.map_or(start, |t| t.domain.clamp(start))));
        }
        let Some((anchor_axis, anchor)) = self.drag_anchor else {
            return;
        };
        if anchor_axis != axis {
            return;
        }
        let Some(delta) = drag_delta else {
            self.drag_anchor = None;
            return;
        };
        let Some(travel) = travel else {
            // The bar lost its geometry mid-drag — content started
            // fitting, or the track collapsed. `drag_delta` stays
            // cumulative from the press, so a resumed anchor would apply
            // the whole accumulated travel at once if geometry came
            // back under the same capture. Drop it; the next press
            // re-anchors.
            self.drag_anchor = None;
            return;
        };
        let target = anchor + axis.main_v(delta) * travel.factor;
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
        // Both directions clamp through the same domain: a page is a
        // bar interaction, so it lands where the thumb can follow it.
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
pub(super) mod internals {
    use crate::widgets::scroll::state::ScrollState;

    impl ScrollState {
        pub(crate) fn drag_anchor_is_none(&self) -> bool {
            self.drag_anchor.is_none()
        }
    }
}
