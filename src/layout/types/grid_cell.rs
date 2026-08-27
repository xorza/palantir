//! Where a child sits in a grid parent — its row, its column, and how far
//! it spans.

use crate::layout::axis::Axis;
use crate::primitives::span::Span;

/// Per-child placement inside a `Grid` parent. Inert when the parent is not a
/// `LayoutMode::Grid`. `(row, col)` is the top-left cell; `(row_span,
/// col_span)` extends the slot toward the bottom-right (defaults to 1×1).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GridCell {
    pub row: u16,
    pub col: u16,
    pub row_span: u16,
    pub col_span: u16,
}

impl std::hash::Hash for GridCell {
    /// One `write` of the packed 8-byte `[u16; 4]` rather than the
    /// derived four `write_u16`s — folded into every `BoundsExtras`
    /// node hash via `BoundsExtras::hash`.
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write(bytemuck::bytes_of(self));
    }
}

impl GridCell {
    /// Track-index span on `axis`: `(col, col_span)` for X,
    /// `(row, row_span)` for Y. Bundles the start/length pair the grid
    /// track math slices with, so the two can't be passed swapped.
    #[inline]
    pub(crate) fn track_span(&self, axis: Axis) -> Span {
        match axis {
            Axis::X => Span::new(self.col as u32, self.col_span as u32),
            Axis::Y => Span::new(self.row as u32, self.row_span as u32),
        }
    }

    /// Place this cell at track `main` along `axis`, leaving the cross
    /// axis where it is. The write half of [`Self::track_span`]: an
    /// axis-generic parent (a `Splitter`) lays its children out along
    /// one axis and shouldn't have to know which field that is.
    #[inline]
    pub(crate) fn set_main(&mut self, axis: Axis, main: u16) {
        match axis {
            Axis::X => self.col = main,
            Axis::Y => self.row = main,
        }
    }
}

impl Default for GridCell {
    fn default() -> Self {
        Self {
            row: 0,
            col: 0,
            row_span: 1,
            col_span: 1,
        }
    }
}
