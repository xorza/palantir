//! Where a child sits in a grid parent.

use crate::common::span::Span;
use crate::primitives::layout::axis::Axis;
use crate::primitives::math::domain;
use std::hash;

/// Per-child placement in a `Grid` parent; inert otherwise. `(row, col)` is the top-left cell, spans default to 1.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
#[must_use]
pub struct GridCell {
    pub(crate) row: u16,
    pub(crate) col: u16,
    pub(crate) row_span: u16,
    pub(crate) col_span: u16,
}

impl hash::Hash for GridCell {
    /// One `write` of the packed `[u16; 4]` instead of four `write_u16`s; hot in `BoundsExtras::hash`.
    #[inline]
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        state.write(bytemuck::bytes_of(self));
    }
}

impl GridCell {
    /// One cell at `row`, `col`.
    pub const fn at(row: u16, col: u16) -> Self {
        Self {
            row,
            col,
            row_span: 1,
            col_span: 1,
        }
    }

    /// This cell widened to `row_span` rows and `col_span` columns, both *counts*.
    ///
    /// # Panics
    ///
    /// Panics unless both spans are [counts](crate::widget::domain::count).
    #[track_caller]
    pub const fn with_span(self, row_span: u16, col_span: u16) -> Self {
        domain::count(row_span as u32);
        domain::count(col_span as u32);
        Self {
            row_span,
            col_span,
            ..self
        }
    }

    /// First row.
    pub const fn row(self) -> u16 {
        self.row
    }

    /// First column.
    pub const fn col(self) -> u16 {
        self.col
    }

    /// Rows spanned.
    pub const fn row_span(self) -> u16 {
        self.row_span
    }

    /// Columns spanned.
    pub const fn col_span(self) -> u16 {
        self.col_span
    }

    /// Track-index span on `axis`: `(col, col_span)` for X, `(row, row_span)` for Y.
    #[inline]
    pub(crate) fn track_span(self, axis: Axis) -> Span {
        match axis {
            Axis::X => Span::new(u32::from(self.col), u32::from(self.col_span)),
            Axis::Y => Span::new(u32::from(self.row), u32::from(self.row_span)),
        }
    }

    /// The cell at track `main` along `axis`, first track across. The write half of `track_span`.
    pub const fn along(axis: Axis, main: u16) -> Self {
        match axis {
            Axis::X => Self::at(0, main),
            Axis::Y => Self::at(main, 0),
        }
    }
}

/// A bare `(row, col)` is a one-by-one cell.
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
