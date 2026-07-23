//! Widget-owned scroll interaction state. Layout measurements enter
//! each input step as ephemeral [`ScrollBounds`] rather than becoming
//! another retained widget-state copy.

use crate::layout::axis::Axis;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use glam::Vec2;

#[derive(Clone, Copy, Debug)]
pub(crate) struct ScrollState {
    pub(crate) offset: Vec2,
    pub(crate) zoom: f32,
    /// Cumulative drag deltas compose against this stable snapshot.
    pub(crate) drag_anchor: Option<(Axis, Vec2)>,
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
pub(crate) struct ScrollBounds {
    pub(crate) content: Size,
    pub(crate) viewport: Size,
    pub(crate) content_margin: Spacing,
}

#[derive(Clone, Copy, Debug)]
struct OffsetEndpoints {
    leading: Vec2,
    trailing: Vec2,
}

#[derive(Clone, Copy, Debug)]
struct OffsetBounds {
    lo: Vec2,
    hi: Vec2,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TrackPage {
    pub(crate) click_main: f32,
    pub(crate) thumb_offset: f32,
    pub(crate) thumb_size: f32,
    pub(crate) page_step: f32,
    pub(crate) max_off: f32,
}

impl ScrollState {
    fn offset_endpoints(&self, bounds: ScrollBounds) -> OffsetEndpoints {
        let [cml, cmt, cmr, cmb] = bounds.content_margin.as_array();
        OffsetEndpoints {
            leading: Vec2::new(-cml * self.zoom, -cmt * self.zoom),
            trailing: Vec2::new(
                bounds.content.w * self.zoom - bounds.viewport.w + cmr * self.zoom,
                bounds.content.h * self.zoom - bounds.viewport.h + cmb * self.zoom,
            ),
        }
    }

    fn natural_bounds(&self, bounds: ScrollBounds) -> OffsetBounds {
        let endpoints = self.offset_endpoints(bounds);
        // Undersized content settles at its leading edge.
        OffsetBounds {
            lo: endpoints.leading,
            hi: endpoints.trailing.max(endpoints.leading),
        }
    }

    fn zoom_rubber_band_bounds(&self, bounds: ScrollBounds) -> OffsetBounds {
        let endpoints = self.offset_endpoints(bounds);
        // Pivot zoom may legitimately place undersized content between
        // the raw endpoints.
        OffsetBounds {
            lo: endpoints.leading.min(endpoints.trailing),
            hi: endpoints.leading.max(endpoints.trailing),
        }
    }

    pub(crate) fn apply_zoom(
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

    pub(crate) fn apply_thumb_drag(
        &mut self,
        axis: Axis,
        drag_started: bool,
        drag_delta: Option<Vec2>,
        geometry: Option<(f32, f32)>,
    ) {
        if drag_started {
            self.drag_anchor = Some((axis, self.offset));
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
        let Some((factor, max_offset)) = geometry else {
            return;
        };
        let target = axis.main_v(anchor) + axis.main_v(delta) * factor;
        let clamped = target.clamp(0.0, max_offset);
        match axis {
            Axis::X => self.offset.x = clamped,
            Axis::Y => self.offset.y = clamped,
        }
    }

    pub(crate) fn apply_track_page(&mut self, axis: Axis, page: Option<TrackPage>) {
        let Some(page) = page else {
            return;
        };
        let current = axis.main_v(self.offset);
        let next = if page.click_main < page.thumb_offset {
            (current - page.page_step).max(0.0)
        } else if page.click_main > page.thumb_offset + page.thumb_size {
            (current + page.page_step).min(page.max_off)
        } else {
            current
        };
        match axis {
            Axis::X => self.offset.x = next,
            Axis::Y => self.offset.y = next,
        }
    }
}
