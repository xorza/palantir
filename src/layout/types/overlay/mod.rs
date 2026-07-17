use crate::layout::axis::Axis;
use crate::layout::types::align::AxisAlign;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use glam::Vec2;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OverlaySide {
    Above,
    Below,
    Left,
    Right,
}

impl OverlaySide {
    const fn axis(self) -> Axis {
        match self {
            Self::Left | Self::Right => Axis::X,
            Self::Above | Self::Below => Axis::Y,
        }
    }

    const fn opposite(self) -> Self {
        match self {
            Self::Above => Self::Below,
            Self::Below => Self::Above,
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }
}

/// Measured side-layer placement relative to an anchor rectangle.
#[derive(Clone, Copy, Debug)]
pub(crate) struct OverlayPosition {
    pub(crate) anchor: Rect,
    pub(crate) side: OverlaySide,
    pub(crate) align: AxisAlign,
    pub(crate) gap: f32,
}

impl OverlayPosition {
    pub(crate) const fn new(anchor: Rect, side: OverlaySide, align: AxisAlign, gap: f32) -> Self {
        Self {
            anchor,
            side,
            align,
            gap,
        }
    }

    pub(crate) const fn at_point(anchor: Vec2) -> Self {
        Self::below(Rect::new(anchor.x, anchor.y, 0.0, 0.0), 0.0)
    }

    pub(crate) const fn above(anchor: Rect, gap: f32) -> Self {
        Self::new(anchor, OverlaySide::Above, AxisAlign::Start, gap)
    }

    pub(crate) const fn below(anchor: Rect, gap: f32) -> Self {
        Self::new(anchor, OverlaySide::Below, AxisAlign::Start, gap)
    }

    pub(crate) const fn left_of(anchor: Rect, gap: f32) -> Self {
        Self::new(anchor, OverlaySide::Left, AxisAlign::Start, gap)
    }

    pub(crate) const fn right_of(anchor: Rect, gap: f32) -> Self {
        Self::new(anchor, OverlaySide::Right, AxisAlign::Start, gap)
    }

    pub(crate) fn resolve(self, measured: Size, bounds: Rect) -> Vec2 {
        let axis = self.side.axis();
        let primary_extent = axis.main(measured);
        let cross_extent = axis.cross(measured);
        let bounds_min = axis.main_v(bounds.min);
        let bounds_max = axis.main_v(bounds.max());
        let preferred = side_position(self.side, self.anchor, primary_extent, self.gap);
        let fallback = side_position(self.side.opposite(), self.anchor, primary_extent, self.gap);
        let primary = choose_side(preferred, fallback, primary_extent, bounds_min, bounds_max);
        let cross = align_cross(self.align, axis, self.anchor, cross_extent, bounds);
        axis.compose_point(primary, cross)
    }
}

fn side_position(side: OverlaySide, anchor: Rect, extent: f32, gap: f32) -> f32 {
    match side {
        OverlaySide::Above => anchor.min.y - gap - extent,
        OverlaySide::Below => anchor.max().y + gap,
        OverlaySide::Left => anchor.min.x - gap - extent,
        OverlaySide::Right => anchor.max().x + gap,
    }
}

fn choose_side(
    preferred: f32,
    fallback: f32,
    extent: f32,
    bounds_min: f32,
    bounds_max: f32,
) -> f32 {
    let fits = |position: f32| position >= bounds_min && position + extent <= bounds_max;
    if fits(preferred) {
        preferred
    } else if fits(fallback) {
        fallback
    } else {
        preferred.clamp(bounds_min, (bounds_max - extent).max(bounds_min))
    }
}

fn align_cross(align: AxisAlign, axis: Axis, anchor: Rect, extent: f32, bounds: Rect) -> f32 {
    let anchor_min = axis.cross_v(anchor.min);
    let anchor_max = axis.cross_v(anchor.max());
    let position = match align {
        AxisAlign::Center => (anchor_min + anchor_max - extent) * 0.5,
        AxisAlign::End => anchor_max - extent,
        AxisAlign::Auto | AxisAlign::Start | AxisAlign::Stretch => anchor_min,
    };
    let bounds_min = axis.cross_v(bounds.min);
    let bounds_max = axis.cross_v(bounds.max());
    position.clamp(bounds_min, (bounds_max - extent).max(bounds_min))
}

#[cfg(test)]
mod tests;
