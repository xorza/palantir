//! Axis enum and axis-symmetric helpers for stack drivers, intrinsics and cache keys.

use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::layout::sizing::{SizeSpec, Sizing};
use glam::Vec2;

/// Which axis a layout distributes children along, or a query targets. `X` horizontal, `Y` vertical.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
#[repr(u8)]
pub enum Axis {
    /// Horizontal.
    X,
    /// Vertical.
    Y,
}

const _: () = assert!(
    Axis::X as u8 == 0 && Axis::Y as u8 == 1,
    "Axis::bit and Axis::from_bit pair on these discriminants",
);

impl Axis {
    /// The bit a packed field spends on the axis; inverse [`Self::from_bit`]. The encoding lives here alone.
    #[inline]
    pub(super) const fn bit(self) -> u16 {
        self as u16
    }

    #[inline]
    pub(super) fn from_bit(bit: u16) -> Self {
        match bit {
            0 => Self::X,
            1 => Self::Y,
            _ => unreachable!("packed axis bit {bit} is invalid"),
        }
    }

    /// The axis this one is not.
    pub(crate) const fn other(self) -> Self {
        match self {
            Self::X => Self::Y,
            Self::Y => Self::X,
        }
    }
    pub(crate) const fn main(self, s: Size) -> f32 {
        match self {
            Self::X => s.w,
            Self::Y => s.h,
        }
    }
    pub(crate) const fn cross(self, s: Size) -> f32 {
        match self {
            Self::X => s.h,
            Self::Y => s.w,
        }
    }
    pub(crate) const fn main_v(self, v: Vec2) -> f32 {
        match self {
            Self::X => v.x,
            Self::Y => v.y,
        }
    }
    pub(crate) const fn cross_v(self, v: Vec2) -> f32 {
        match self {
            Self::X => v.y,
            Self::Y => v.x,
        }
    }
    pub(crate) const fn main_sizing(self, s: SizeSpec) -> Sizing {
        match self {
            Self::X => s.w(),
            Self::Y => s.h(),
        }
    }
    /// Total spacing along this axis (left+right for X, top+bottom for Y).
    pub(crate) fn spacing(self, s: Spacing) -> f32 {
        self.main(s.sums())
    }
    /// Build a `Size` from main- and cross-axis lengths.
    pub(crate) const fn compose_size(self, main: f32, cross: f32) -> Size {
        match self {
            Self::X => Size::new(main, cross),
            Self::Y => Size::new(cross, main),
        }
    }
    /// A `Spacing` with `main` on main-axis sides and `cross` on cross-axis sides; inverse of [`Self::spacing`] (a `Splitter`'s grab bar overhangs along the split axis only).
    pub(crate) fn compose_spacing(self, main: f32, cross: f32) -> Spacing {
        match self {
            Self::X => Spacing::new(main, cross, main, cross),
            Self::Y => Spacing::new(cross, main, cross, main),
        }
    }
    /// Order a main/cross pair as grid APIs take them, `[rows, cols]`; `Axis::X` distributes along columns, so its main list is the column list.
    pub(crate) const fn rows_cols<T>(self, main: T, cross: T) -> [T; 2] {
        match self {
            Self::X => [cross, main],
            Self::Y => [main, cross],
        }
    }
    /// Build a `Vec2` from main- and cross-axis positions.
    pub(crate) const fn compose_point(self, main: f32, cross: f32) -> Vec2 {
        match self {
            Self::X => Vec2::new(main, cross),
            Self::Y => Vec2::new(cross, main),
        }
    }
    /// Build a `Rect` from its main- and cross-axis extents.
    pub(crate) const fn compose_rect(self, main: AxisExtent, cross: AxisExtent) -> Rect {
        match self {
            Self::X => Rect::new(main.start, cross.start, main.len, cross.len),
            Self::Y => Rect::new(cross.start, main.start, cross.len, main.len),
        }
    }
}

/// Where a rect starts along one axis and how far it reaches.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct AxisExtent {
    pub(crate) start: f32,
    pub(crate) len: f32,
}

impl AxisExtent {
    pub(crate) const fn new(start: f32, len: f32) -> Self {
        Self { start, len }
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::primitives::layout::axis::Axis;
    use crate::primitives::layout::sizing::{SizeSpec, Sizing};

    impl Axis {
        /// Build a `SizeSpec` from main- and cross-axis sizings; the [`Axis::compose_size`] of a sizing.
        pub(crate) const fn compose_sizing(self, main: Sizing, cross: Sizing) -> SizeSpec {
            match self {
                Self::X => SizeSpec::new(main, cross),
                Self::Y => SizeSpec::new(cross, main),
            }
        }
    }
}
