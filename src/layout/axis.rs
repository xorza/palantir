//! Axis enum + axis-symmetric helpers used by stack drivers and the
//! intrinsic query. Lifted out of `stack` so non-stack code (intrinsics,
//! cache keys) can refer to it.

use crate::layout::types::{sizing::Sizes, sizing::Sizing};
use crate::primitives::{rect::Rect, size::Size, spacing::Spacing};
use glam::{BVec2, Vec2};

/// Which axis a layout distributes children along (or which axis a query
/// targets). `X` = horizontal, `Y` = vertical.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Axis {
    X,
    Y,
}

impl Axis {
    pub(crate) fn main(self, s: Size) -> f32 {
        match self {
            Axis::X => s.w,
            Axis::Y => s.h,
        }
    }
    pub(crate) fn cross(self, s: Size) -> f32 {
        match self {
            Axis::X => s.h,
            Axis::Y => s.w,
        }
    }
    pub(crate) fn main_v(self, v: Vec2) -> f32 {
        match self {
            Axis::X => v.x,
            Axis::Y => v.y,
        }
    }
    /// This axis's lane of a boolean mask — `Scroll` reads its pan mask
    /// per axis the same way it reads its offset.
    pub(crate) fn main_b(self, v: BVec2) -> bool {
        match self {
            Axis::X => v.x,
            Axis::Y => v.y,
        }
    }
    pub(crate) fn cross_v(self, v: Vec2) -> f32 {
        match self {
            Axis::X => v.y,
            Axis::Y => v.x,
        }
    }
    pub(crate) fn main_sizing(self, s: Sizes) -> Sizing {
        match self {
            Axis::X => s.w(),
            Axis::Y => s.h(),
        }
    }
    /// Total spacing along this axis (left+right for X, top+bottom for Y).
    pub(crate) fn spacing(self, s: Spacing) -> f32 {
        match self {
            Axis::X => s.horiz(),
            Axis::Y => s.vert(),
        }
    }
    /// Build a `Size` from main- and cross-axis lengths.
    pub(crate) fn compose_size(self, main: f32, cross: f32) -> Size {
        match self {
            Axis::X => Size::new(main, cross),
            Axis::Y => Size::new(cross, main),
        }
    }
    /// Build a `Spacing` whose main-axis sides carry `main` and whose
    /// cross-axis sides carry `cross` — the inverse of [`Self::spacing`],
    /// and the shape an axis-generic inset wants (a `Splitter`'s grab bar
    /// overhangs its seam along the split axis only).
    pub(crate) fn compose_spacing(self, main: f32, cross: f32) -> Spacing {
        match self {
            Axis::X => Spacing::new(main, cross, main, cross),
            Axis::Y => Spacing::new(cross, main, cross, main),
        }
    }
    /// Order a main/cross pair the way the grid APIs take them:
    /// `[rows, cols]`. `Axis::X` distributes along columns, so its main
    /// list *is* the column list; `Axis::Y` distributes along rows.
    pub(crate) fn rows_cols<T>(self, main: T, cross: T) -> [T; 2] {
        match self {
            Axis::X => [cross, main],
            Axis::Y => [main, cross],
        }
    }
    /// Build a `Vec2` from main- and cross-axis positions.
    pub(crate) fn compose_point(self, main: f32, cross: f32) -> Vec2 {
        match self {
            Axis::X => Vec2::new(main, cross),
            Axis::Y => Vec2::new(cross, main),
        }
    }
    /// Build a `Rect` from main- and cross-axis positions and lengths.
    pub(crate) fn compose_rect(self, main_pos: f32, cross_pos: f32, main: f32, cross: f32) -> Rect {
        match self {
            Axis::X => Rect::new(main_pos, cross_pos, main, cross),
            Axis::Y => Rect::new(cross_pos, main_pos, cross, main),
        }
    }
}
