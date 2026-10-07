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
    pub(super) fn from_bit(bit: u16) -> Axis {
        match bit {
            0 => Axis::X,
            1 => Axis::Y,
            _ => unreachable!("packed axis bit {bit} is invalid"),
        }
    }

    /// The axis this one is not.
    pub(crate) const fn other(self) -> Axis {
        match self {
            Axis::X => Axis::Y,
            Axis::Y => Axis::X,
        }
    }
    pub(crate) const fn main(self, s: Size) -> f32 {
        match self {
            Axis::X => s.w,
            Axis::Y => s.h,
        }
    }
    pub(crate) const fn cross(self, s: Size) -> f32 {
        match self {
            Axis::X => s.h,
            Axis::Y => s.w,
        }
    }
    pub(crate) const fn main_v(self, v: Vec2) -> f32 {
        match self {
            Axis::X => v.x,
            Axis::Y => v.y,
        }
    }
    pub(crate) const fn cross_v(self, v: Vec2) -> f32 {
        match self {
            Axis::X => v.y,
            Axis::Y => v.x,
        }
    }
    pub(crate) const fn main_sizing(self, s: SizeSpec) -> Sizing {
        match self {
            Axis::X => s.w(),
            Axis::Y => s.h(),
        }
    }
    /// Total spacing along this axis (left+right for X, top+bottom for Y).
    pub(crate) fn spacing(self, s: Spacing) -> f32 {
        self.main(s.sums())
    }
    /// Build a `Size` from main- and cross-axis lengths.
    pub(crate) const fn compose_size(self, main: f32, cross: f32) -> Size {
        match self {
            Axis::X => Size::new(main, cross),
            Axis::Y => Size::new(cross, main),
        }
    }
    /// A `Spacing` with `main` on main-axis sides and `cross` on cross-axis sides; inverse of [`Self::spacing`] (a `Splitter`'s grab bar overhangs along the split axis only).
    pub(crate) fn compose_spacing(self, main: f32, cross: f32) -> Spacing {
        match self {
            Axis::X => Spacing::new(main, cross, main, cross),
            Axis::Y => Spacing::new(cross, main, cross, main),
        }
    }
    /// Order a main/cross pair as grid APIs take them, `[rows, cols]`; `Axis::X` distributes along columns, so its main list is the column list.
    pub(crate) const fn rows_cols<T>(self, main: T, cross: T) -> [T; 2] {
        match self {
            Axis::X => [cross, main],
            Axis::Y => [main, cross],
        }
    }
    /// Build a `Vec2` from main- and cross-axis positions.
    pub(crate) const fn compose_point(self, main: f32, cross: f32) -> Vec2 {
        match self {
            Axis::X => Vec2::new(main, cross),
            Axis::Y => Vec2::new(cross, main),
        }
    }
    /// Build a `Rect` from main- and cross-axis positions and lengths.
    pub(crate) const fn compose_rect(
        self,
        main_pos: f32,
        cross_pos: f32,
        main: f32,
        cross: f32,
    ) -> Rect {
        match self {
            Axis::X => Rect::new(main_pos, cross_pos, main, cross),
            Axis::Y => Rect::new(cross_pos, main_pos, cross, main),
        }
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
                Axis::X => SizeSpec::new(main, cross),
                Axis::Y => SizeSpec::new(cross, main),
            }
        }
    }
}
