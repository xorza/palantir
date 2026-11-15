//! Where a child sits in a grid parent.

use crate::common::span::Span;
use crate::primitives::layout::axis::Axis;
use std::hash;
use std::num::{NonZeroU16, NonZeroU32};

/// Per-child placement in a `Grid` parent; inert otherwise. `(row, col)` is the top-left cell, spans default to 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use]
pub struct GridCell {
    row: u16,
    col: u16,
    row_span: NonZeroU16,
    col_span: NonZeroU16,
}

impl hash::Hash for GridCell {
    /// One `write_u64` of the four fields packed instead of four `write_u16`s; hot in
    /// `BoundsExtras::hash`.
    #[inline]
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        state.write_u64(
            u64::from(self.row)
                | (u64::from(self.col) << 16)
                | (u64::from(self.row_span.get()) << 32)
                | (u64::from(self.col_span.get()) << 48),
        );
    }
}

impl GridCell {
    /// One cell at `row`, `col`.
    pub const fn at(row: u16, col: u16) -> Self {
        Self {
            row,
            col,
            row_span: NonZeroU16::MIN,
            col_span: NonZeroU16::MIN,
        }
    }

    /// This cell widened to `row_span` rows and `col_span` columns.
    pub const fn with_span(self, row_span: NonZeroU16, col_span: NonZeroU16) -> Self {
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
    pub const fn row_span(self) -> NonZeroU16 {
        self.row_span
    }

    /// Columns spanned.
    pub const fn col_span(self) -> NonZeroU16 {
        self.col_span
    }

    /// This cell held inside `rows` × `cols` tracks: the row and column at most the last track,
    /// each span at most the tracks left. Coerced, not checked, as the track lists change at run
    /// time.
    pub(crate) fn clamped_to(self, rows: NonZeroU16, cols: NonZeroU16) -> Self {
        // An index at most the last track leaves at least that track.
        let left = |tracks: NonZeroU16, at: u16| {
            NonZeroU16::new(tracks.get() - at).expect("the last track is left")
        };
        let row = self.row.min(rows.get() - 1);
        let col = self.col.min(cols.get() - 1);
        Self {
            row,
            col,
            row_span: self.row_span.min(left(rows, row)),
            col_span: self.col_span.min(left(cols, col)),
        }
    }

    /// Track-index span on `axis`: `(col, col_span)` for X, `(row, row_span)` for Y.
    #[inline]
    pub(crate) fn track_span(self, axis: Axis) -> Span {
        match axis {
            Axis::X => Span::new(u32::from(self.col), NonZeroU32::from(self.col_span).get()),
            Axis::Y => Span::new(u32::from(self.row), NonZeroU32::from(self.row_span).get()),
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
