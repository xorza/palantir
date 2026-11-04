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
enum AnchorSide {
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

/// What an anchor places the body beside: a rect it keeps clear, or a point, as the pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AnchorTarget {
    Rect,
    Point,
}

/// Where an anchored body sits across its anchored side; it still shifts back inside the surface if pushed off.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AnchorAlign {
    /// Flush with the start edge; the default.
    #[default]
    Start,
    /// Centered on the anchored rect.
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
/// [`LayerScope::anchor`](crate::LayerScope::anchor). The body measures against the room the
/// roomier side leaves, and the origin resolves after measure: the requested side if the body
/// fits, else the opposite, else shifted inside the surface. [`LayerScope::fixed_at`](crate::LayerScope::fixed_at) never moves.
#[derive(Clone, Copy, Debug, PartialEq)]
#[must_use]
pub struct Anchor {
    rect: Rect,
    target: AnchorTarget,
    side: AnchorSide,
    align: AnchorAlign,
    gap: f32,
}

impl Anchor {
    /// Beside `rect` on `side`, flush with its start edge and touching it.
    #[track_caller]
    const fn on_side(rect: Rect, side: AnchorSide) -> Self {
        rect.validate();
        Self {
            rect,
            target: AnchorTarget::Rect,
            side,
            align: AnchorAlign::Start,
            gap: 0.0,
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
        Self {
            target: AnchorTarget::Point,
            ..Self::below(Rect::new(point.x, point.y, 0.0, 0.0))
        }
    }

    /// Whether [`Self::at_point`] made the anchor. A rect that collapsed to no size stays a rect,
    /// so a list under a button that shrank still places each frame.
    pub(crate) const fn is_point(self) -> bool {
        matches!(self.target, AnchorTarget::Point)
    }

    /// Above `rect`, falling back to below it.
    ///
    /// # Panics
    ///
    /// Panics unless every component of `rect` is an [offset](crate::widget::domain::offset).
    #[track_caller]
    pub const fn above(rect: Rect) -> Self {
        Self::on_side(rect, AnchorSide::Above)
    }

    /// Below `rect`, falling back to above it.
    ///
    /// # Panics
    ///
    /// Panics unless every component of `rect` is an [offset](crate::widget::domain::offset).
    #[track_caller]
    pub const fn below(rect: Rect) -> Self {
        Self::on_side(rect, AnchorSide::Below)
    }

    /// Left of `rect`, falling back to its right.
    ///
    /// # Panics
    ///
    /// Panics unless every component of `rect` is an [offset](crate::widget::domain::offset).
    #[track_caller]
    pub const fn left_of(rect: Rect) -> Self {
        Self::on_side(rect, AnchorSide::LeftOf)
    }

    /// Right of `rect`, falling back to its left.
    ///
    /// # Panics
    ///
    /// Panics unless every component of `rect` is an [offset](crate::widget::domain::offset).
    #[track_caller]
    pub const fn right_of(rect: Rect) -> Self {
        Self::on_side(rect, AnchorSide::RightOf)
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

    /// The extent a body may measure to inside `bounds`: across its side, the whole of `bounds`;
    /// along it, the room the roomier of its two sides leaves. As Floating UI's `flip` and `size`
    /// bound a floating element to the side it picks, a body too long for either side measures
    /// to the roomier one, which [`Self::resolve`] then places it on, and scrolls there rather
    /// than covering the rect it belongs to. A body that fits a side measures as it would against
    /// all of `bounds`, so the side it lands on is the same.
    pub(crate) fn available(self, bounds: Rect) -> Size {
        let axis = self.side.axis();
        let room = self
            .room(self.side, bounds)
            .max(self.room(self.side.opposite(), bounds));
        axis.compose_size(room, axis.cross(bounds.size))
    }

    /// The room `side` of the rect leaves inside `bounds`, `gap` off it; zero for a rect at or
    /// past that edge.
    fn room(self, side: AnchorSide, bounds: Rect) -> f32 {
        let room = match side {
            AnchorSide::Above => self.rect.min.y - self.gap - bounds.min.y,
            AnchorSide::Below => bounds.max().y - (self.rect.max().y + self.gap),
            AnchorSide::LeftOf => self.rect.min.x - self.gap - bounds.min.x,
            AnchorSide::RightOf => bounds.max().x - (self.rect.max().x + self.gap),
        };
        room.max(0.0)
    }

    /// The origin of a body of `measured` size inside `bounds`.
    pub(crate) fn resolve(self, measured: Size, bounds: Rect) -> Vec2 {
        let axis = self.side.axis();
        axis.compose_point(
            self.main_pos(axis.main(measured), bounds),
            self.cross_pos(axis.cross(measured), bounds),
        )
    }

    /// The body's near edge along the side axis: the requested side if a body of `extent` fits inside `bounds`, else the opposite side, else the requested side shifted inside.
    fn main_pos(self, extent: f32, bounds: Rect) -> f32 {
        let axis = self.side.axis();
        let lo = axis.main_v(bounds.min);
        let hi = axis.main_v(bounds.max());
        let fits = |pos: f32| pos >= lo && pos + extent <= hi;
        let preferred = self.side_pos(self.side, extent);
        if fits(preferred) {
            return preferred;
        }
        let fallback = self.side_pos(self.side.opposite(), extent);
        if fits(fallback) {
            return fallback;
        }
        preferred.clamp(lo, (hi - extent).max(lo))
    }

    /// The near edge of a body of `extent` on `side`, `gap` off the rect.
    const fn side_pos(self, side: AnchorSide, extent: f32) -> f32 {
        match side {
            AnchorSide::Above => self.rect.min.y - self.gap - extent,
            AnchorSide::Below => self.rect.max().y + self.gap,
            AnchorSide::LeftOf => self.rect.min.x - self.gap - extent,
            AnchorSide::RightOf => self.rect.max().x + self.gap,
        }
    }

    /// The body's near edge across the side axis: aligned on the rect, then shifted inside `bounds`.
    fn cross_pos(self, extent: f32, bounds: Rect) -> f32 {
        let axis = self.side.axis();
        let pos = axis.cross_v(self.rect.min)
            + self
                .align
                .axis_align()
                .offset_in(axis.cross(self.rect.size), extent);
        let lo = axis.cross_v(bounds.min);
        let hi = axis.cross_v(bounds.max());
        pos.clamp(lo, (hi - extent).max(lo))
    }
}

#[cfg(test)]
mod tests;
