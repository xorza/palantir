//! The per-node placement column: explicit position, cell, size bounds,
//! and the node's place in the Tab order.

use crate::primitives::geometry::size::Size;
use crate::primitives::layout::grid_cell::GridCell;
use crate::primitives::math::domain;
use crate::primitives::math::float_hash::FloatHash;
use glam::Vec2;
use std::hash;
use std::hash::Hash;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BoundsExtras {
    pub(crate) position: Vec2,
    pub(crate) grid: GridCell,
    pub(crate) min_size: Size,
    pub(crate) max_size: Size,
    /// The Tab order key — see [`Configure::tab_index`](crate::Configure::tab_index).
    pub(crate) tab_index: i16,
}

impl Hash for BoundsExtras {
    #[inline]
    fn hash<H: hash::Hasher>(&self, h: &mut H) {
        self.position.hash_visual(h);
        self.grid.hash(h);
        self.min_size.hash_visual(h);
        self.max_size.hash_visual(h);
        self.tab_index.hash(h);
    }
}

impl BoundsExtras {
    pub(crate) const DEFAULT: Self = Self {
        position: Vec2::ZERO,
        grid: GridCell {
            row: 0,
            col: 0,
            row_span: 1,
            col_span: 1,
        },
        min_size: Size::ZERO,
        max_size: Size::INF,
        tab_index: 0,
    };

    #[inline]
    pub(crate) fn is_default(&self) -> bool {
        domain::is_approx_zero(self.position.x)
            && domain::is_approx_zero(self.position.y)
            && self.grid == Self::DEFAULT.grid
            && self.min_size.is_approx_zero()
            && self.max_size == Self::DEFAULT.max_size
            && self.tab_index == 0
    }
}

impl Default for BoundsExtras {
    fn default() -> Self {
        Self::DEFAULT
    }
}
