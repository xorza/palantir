//! The per-node placement column: explicit position, cell, and size bounds.

use crate::layout::types::grid_cell::GridCell;
use crate::primitives::approx::{self, FloatHash};
use crate::primitives::size::Size;
use glam::Vec2;
use std::hash::Hash;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BoundsExtras {
    pub(crate) position: Vec2,
    pub(crate) grid: GridCell,
    pub(crate) min_size: Size,
    pub(crate) max_size: Size,
}

impl Hash for BoundsExtras {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.position.hash_visual(h);
        self.grid.hash(h);
        self.min_size.hash_visual(h);
        self.max_size.hash_visual(h);
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
    };

    #[inline]
    pub(crate) fn is_default(&self) -> bool {
        approx::approx_zero(self.position.x)
            && approx::approx_zero(self.position.y)
            && self.grid == Self::DEFAULT.grid
            && self.min_size.approx_zero()
            && self.max_size == Self::DEFAULT.max_size
    }
}

impl Default for BoundsExtras {
    fn default() -> Self {
        Self::DEFAULT
    }
}
