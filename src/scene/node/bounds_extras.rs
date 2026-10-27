//! The per-node placement column: explicit position, cell, size bounds and Tab order.

use crate::primitives::geometry::size::Size;
use crate::primitives::layout::grid_cell::GridCell;
use crate::primitives::math::domain;
use crate::primitives::math::float_hash::FloatHash;
use glam::Vec2;
use std::hash;
use std::hash::Hash;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BoundsExtras {
    pub(crate) pos: Vec2,
    pub(crate) grid: GridCell,
    pub(crate) min_size: Size,
    pub(crate) max_size: Size,
    /// The Tab order key — see [`Configure::tab_idx`](crate::Configure::tab_idx).
    pub(crate) tab_idx: i16,
}

impl Hash for BoundsExtras {
    #[inline]
    fn hash<H: hash::Hasher>(&self, h: &mut H) {
        self.pos.hash_visual(h);
        self.grid.hash(h);
        self.min_size.hash_visual(h);
        self.max_size.hash_visual(h);
        self.tab_idx.hash(h);
    }
}

impl BoundsExtras {
    pub(crate) const DEFAULT: Self = Self {
        pos: Vec2::ZERO,
        grid: GridCell::at(0, 0),
        min_size: Size::ZERO,
        max_size: Size::INF,
        tab_idx: 0,
    };

    #[inline]
    pub(crate) fn is_default(&self) -> bool {
        domain::is_approx_zero(self.pos.x)
            && domain::is_approx_zero(self.pos.y)
            && self.grid == Self::DEFAULT.grid
            && self.min_size.is_approx_zero()
            && self.max_size == Self::DEFAULT.max_size
            && self.tab_idx == 0
    }
}

impl Default for BoundsExtras {
    fn default() -> Self {
        Self::DEFAULT
    }
}
