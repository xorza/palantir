//! Where a child sits in a grid parent — its row, its column, and how far
//! it spans.

use crate::common::span::Span;
use crate::primitives::layout::axis::Axis;
use std::hash;

/// Per-child placement inside a `Grid` parent. Inert when the parent is not a
/// `LayoutMode::Grid`. `(row, col)` is the top-left cell; `(row_span,
/// col_span)` extends the slot toward the bottom-right (defaults to 1×1).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
#[must_use]
pub struct GridCell {
    /// Zero-based row.
    pub row: u16,
    /// Zero-based column.
    pub col: u16,
    /// Rows covered, at least one.
    pub row_span: u16,
    /// Columns covered, at least one.
    pub col_span: u16,
}

impl hash::Hash for GridCell {
    /// One `write` of the packed 8-byte `[u16; 4]` rather than the
    /// derived four `write_u16`s — folded into every `BoundsExtras`
    /// node hash via `BoundsExtras::hash`.
    #[inline]
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        state.write(bytemuck::bytes_of(self));
    }
}

impl GridCell {
    /// The cell at `(row, col)`, one track wide and one tall.
    pub const fn at(row: u16, col: u16) -> Self {
        Self {
            row,
            col,
            row_span: 1,
            col_span: 1,
        }
    }

    /// This cell widened to cover `row_span` rows and `col_span`
    /// columns. Both floor at one — a zero-track span names no cell.
    pub const fn with_span(self, row_span: u16, col_span: u16) -> Self {
        Self {
            row_span: if row_span > 1 { row_span } else { 1 },
            col_span: if col_span > 1 { col_span } else { 1 },
            ..self
        }
    }

    /// Track-index span on `axis`: `(col, col_span)` for X,
    /// `(row, row_span)` for Y. Bundles the start/length pair the grid
    /// track math slices with, so the two can't be passed swapped.
    #[inline]
    pub(crate) fn track_span(self, axis: Axis) -> Span {
        match axis {
            Axis::X => Span::new(u32::from(self.col), u32::from(self.col_span)),
            Axis::Y => Span::new(u32::from(self.row), u32::from(self.row_span)),
        }
    }

    /// The cell at track `main` along `axis` and the first track across
    /// it, one track each way. The write half of `track_span`: an
    /// axis-generic parent (a `Splitter`) lays its children out along one
    /// axis and shouldn't have to know which field that is.
    pub const fn along(axis: Axis, main: u16) -> Self {
        match axis {
            Axis::X => Self::at(0, main),
            Axis::Y => Self::at(main, 0),
        }
    }
}

/// A bare `(row, col)` names the common cell — one track each way — so
/// the placement most children want stays a pair at the call site.
impl From<(u16, u16)> for GridCell {
    fn from((row, col): (u16, u16)) -> Self {
        Self::at(row, col)
    }
}

impl Default for GridCell {
    fn default() -> Self {
        Self::at(0, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn along_puts_main_on_the_axis_named_and_cross_on_the_first_track() {
        assert_eq!(GridCell::along(Axis::X, 2), GridCell::at(0, 2));
        assert_eq!(GridCell::along(Axis::Y, 2), GridCell::at(2, 0));
        assert_eq!(
            GridCell::along(Axis::X, 2).track_span(Axis::X),
            GridCell::at(0, 2).track_span(Axis::X),
        );
    }
}
