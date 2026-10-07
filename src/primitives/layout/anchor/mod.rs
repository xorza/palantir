//! The anchored origin rule a side layer resolves after measure, and its side vocabulary.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::layout::align::AxisAlign;
use crate::primitives::layout::axis::Axis;
use crate::primitives::math::domain::{self, vec2};
use glam::Vec2;

/// Which side of the anchored rect the body sits on, outside it.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AnchorSide {
    Above,
    Below,
    LeftOf,
    RightOf,
}

impl AnchorSide {
    const fn axis(self) -> Axis {
        match self {
            Self::LeftOf | Self::RightOf => Axis::X,
            Self::Above | Self::Below => Axis::Y,
        }
    }

    const fn opposite(self) -> Self {
        match self {
            Self::Above => Self::Below,
            Self::Below => Self::Above,
            Self::LeftOf => Self::RightOf,
            Self::RightOf => Self::LeftOf,
        }
    }
}

/// Where an anchored body sits across its anchored side; it still shifts back inside the surface if pushed off.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AnchorAlign {
    /// Flush with the start edge; the default.
    #[default]
    Start,
    /// Centred on the anchored rect.
    Center,
    /// Flush with the end edge.
    End,
}

impl AnchorAlign {
    const fn axis_align(self) -> AxisAlign {
        match self {
            Self::Start => AxisAlign::Start,
            Self::Center => AxisAlign::Center,
            Self::End => AxisAlign::End,
        }
    }
}

/// Where a side layer lands next to the thing it belongs to, for
/// [`LayerScope::anchor`](crate::LayerScope::anchor). The origin resolves
/// after measure: the requested side if the body fits, else the opposite, else
/// shifted inside the surface. [`LayerScope::fixed_at`](crate::LayerScope::fixed_at) never moves.
#[derive(Clone, Copy, Debug)]
#[must_use]
pub struct Anchor {
    rect: Rect,
    side: AnchorSide,
    align: AnchorAlign,
    gap: f32,
}

impl Anchor {
    pub(crate) const fn new(rect: Rect, side: AnchorSide, align: AnchorAlign, gap: f32) -> Self {
        Self {
            rect,
            side,
            align,
            gap,
        }
    }

    /// Below a zero-sized rect at `point`, for an overlay raised at the pointer.
    ///
    /// # Panics
    ///
    /// Panics unless both axes of `point` are [offsets](crate::widget::domain::offset).
    #[track_caller]
    pub const fn at_point(point: Vec2) -> Self {
        let point = vec2::offset(point);
        Self::below(Rect::new(point.x, point.y, 0.0, 0.0))
    }

    /// Above `rect`, falling back to below it.
    ///
    /// # Panics
    ///
    /// Panics unless every component of `rect` is an [offset](crate::widget::domain::offset).
    #[track_caller]
    pub const fn above(rect: Rect) -> Self {
        rect.validate();
        Self::new(rect, AnchorSide::Above, AnchorAlign::Start, 0.0)
    }

    /// Below `rect`, falling back to above it.
    ///
    /// # Panics
    ///
    /// Panics unless every component of `rect` is an [offset](crate::widget::domain::offset).
    #[track_caller]
    pub const fn below(rect: Rect) -> Self {
        rect.validate();
        Self::new(rect, AnchorSide::Below, AnchorAlign::Start, 0.0)
    }

    /// Left of `rect`, falling back to its right.
    ///
    /// # Panics
    ///
    /// Panics unless every component of `rect` is an [offset](crate::widget::domain::offset).
    #[track_caller]
    pub const fn left_of(rect: Rect) -> Self {
        rect.validate();
        Self::new(rect, AnchorSide::LeftOf, AnchorAlign::Start, 0.0)
    }

    /// Right of `rect`, falling back to its left.
    ///
    /// # Panics
    ///
    /// Panics unless every component of `rect` is an [offset](crate::widget::domain::offset).
    #[track_caller]
    pub const fn right_of(rect: Rect) -> Self {
        rect.validate();
        Self::new(rect, AnchorSide::RightOf, AnchorAlign::Start, 0.0)
    }

    /// Where the body sits across its anchored side; [`AnchorAlign::Start`] by default.
    pub const fn with_align(mut self, align: AnchorAlign) -> Self {
        self.align = align;
        self
    }

    /// Holds the body `px` off the anchored rect; zero by default. `px`: a *length*.
    ///
    /// # Panics
    ///
    /// Panics unless `px` is a [length](crate::widget::domain::length).
    #[track_caller]
    pub const fn with_gap(mut self, px: f32) -> Self {
        self.gap = domain::length(px);
        self
    }

    pub(crate) fn resolve(self, measured: Size, bounds: Rect) -> Vec2 {
        let axis = self.side.axis();
        let primary_extent = axis.main(measured);
        let cross_extent = axis.cross(measured);
        let bounds_min = axis.main_v(bounds.min);
        let bounds_max = axis.main_v(bounds.max());
        let preferred = side_position(self.side, self.rect, primary_extent, self.gap);
        let fallback = side_position(self.side.opposite(), self.rect, primary_extent, self.gap);
        let primary = choose_side(preferred, fallback, primary_extent, bounds_min, bounds_max);
        let cross = align_cross(self.align, axis, self.rect, cross_extent, bounds);
        axis.compose_point(primary, cross)
    }
}

fn side_position(side: AnchorSide, rect: Rect, extent: f32, gap: f32) -> f32 {
    match side {
        AnchorSide::Above => rect.min.y - gap - extent,
        AnchorSide::Below => rect.max().y + gap,
        AnchorSide::LeftOf => rect.min.x - gap - extent,
        AnchorSide::RightOf => rect.max().x + gap,
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

fn align_cross(align: AnchorAlign, axis: Axis, rect: Rect, extent: f32, bounds: Rect) -> f32 {
    let rect_min = axis.cross_v(rect.min);
    let position = rect_min + align.axis_align().offset_in(axis.cross(rect.size), extent);
    let bounds_min = axis.cross_v(bounds.min);
    let bounds_max = axis.cross_v(bounds.max());
    position.clamp(bounds_min, (bounds_max - extent).max(bounds_min))
}

#[cfg(test)]
mod tests;
