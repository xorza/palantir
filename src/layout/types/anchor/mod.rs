//! The anchored origin rule a side layer resolves after measure, and the
//! side vocabulary it is written in.

use crate::layout::axis::Axis;
use crate::layout::types::align::AxisAlign;
use crate::primitives::approx::FloatHash;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use glam::Vec2;

/// Which side of the anchored rect the body sits on — outside it, not
/// on its edge. `Top` / `Bottom` mean an edge elsewhere in the crate
/// ([`SplitSide`](crate::SplitSide), [`VAlign`](crate::VAlign)), so the
/// four names here are relational and match the constructors that mint
/// them one for one.
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

/// Where a side layer lands next to the thing it belongs to.
///
/// Hand one to [`LayerScope::anchored`](crate::LayerScope::anchored). The
/// origin resolves *after* measure, from the body's own size against the
/// surface: the body takes the side you asked for when it fits there,
/// flips to the opposite side when it does not, and shifts back inside
/// the surface when neither side has room. That is what a dropdown does
/// near the bottom edge, and it is the whole reason this is a value the
/// layer resolves rather than a point you compute.
///
/// [`LayerScope::fixed_at`](crate::LayerScope::fixed_at) is the other
/// form: a top-left that never moves.
#[derive(Clone, Copy, Debug)]
pub struct Anchor {
    rect: Rect,
    side: AnchorSide,
    align: AxisAlign,
    gap: f32,
}

impl Anchor {
    /// Feed this anchor to a hasher under visual canonicalization.
    ///
    /// Inherent rather than an [`FloatHash`] impl: the trait's other half
    /// is the `Hash`/`PartialEq` agreement, and this type has neither. The
    /// one reader is the cascade fingerprint, which asks whether a
    /// placement would arrange to the same pixels.
    pub(crate) fn hash_visual<H: std::hash::Hasher>(&self, state: &mut H) {
        self.rect.hash_visual(state);
        state.write_u8(self.side as u8);
        state.write_u8(self.align as u8);
        self.gap.hash_visual(state);
    }

    pub(crate) const fn new(rect: Rect, side: AnchorSide, align: AxisAlign, gap: f32) -> Self {
        Self {
            rect,
            side,
            align,
            gap,
        }
    }

    /// Below a zero-sized rect at `point` — the point form, for an
    /// overlay raised at the pointer rather than off a widget's rect.
    /// Still flips and shifts, so a menu opened near the bottom edge
    /// comes up rather than off-screen.
    pub const fn at_point(point: Vec2) -> Self {
        Self::below(Rect::new(point.x, point.y, 0.0, 0.0))
    }

    /// Above `rect`, falling back to below it.
    pub const fn above(rect: Rect) -> Self {
        Self::new(rect, AnchorSide::Above, AxisAlign::Start, 0.0)
    }

    /// Below `rect`, falling back to above it.
    pub const fn below(rect: Rect) -> Self {
        Self::new(rect, AnchorSide::Below, AxisAlign::Start, 0.0)
    }

    /// Left of `rect`, falling back to its right.
    pub const fn left_of(rect: Rect) -> Self {
        Self::new(rect, AnchorSide::LeftOf, AxisAlign::Start, 0.0)
    }

    /// Right of `rect`, falling back to its left.
    pub const fn right_of(rect: Rect) -> Self {
        Self::new(rect, AnchorSide::RightOf, AxisAlign::Start, 0.0)
    }

    /// Hold the body this far off the anchored rect, in logical px.
    ///
    /// Zero by default, because a dropdown meets the trigger it drops out
    /// of. An overlay that reads as a separate object — a tooltip — sets
    /// its own.
    pub const fn gap(mut self, px: f32) -> Self {
        self.gap = px;
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

fn align_cross(align: AxisAlign, axis: Axis, rect: Rect, extent: f32, bounds: Rect) -> f32 {
    let rect_min = axis.cross_v(rect.min);
    let position = rect_min + align.offset_in(axis.cross(rect.size), extent);
    let bounds_min = axis.cross_v(bounds.min);
    let bounds_max = axis.cross_v(bounds.max());
    position.clamp(bounds_min, (bounds_max - extent).max(bounds_min))
}

#[cfg(test)]
mod tests;
