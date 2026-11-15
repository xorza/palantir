//! The interned definition a node carries a whole grid by.

use crate::common::span::Span;
use crate::primitives::layout::axis::Axis;
use crate::primitives::layout::track::Track;
use crate::primitives::math::float_hash::FloatHash;
use std::hash;

/// One Grid's rows and columns, as spans into a `Tree`'s retained flat track arena, and whether
/// a cell of it spans tracks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GridDef {
    pub(crate) rows: Span,
    pub(crate) cols: Span,
    /// Whether a child asks for two rows or more, set as the grid records its cells, so a grid of
    /// none skips the walk for them. A span its tracks clamp to one still counts.
    pub(crate) spans_rows: bool,
    /// Whether a child asks for two columns or more, as [`Self::spans_rows`].
    pub(crate) spans_cols: bool,
}

impl GridDef {
    /// Whether a child asks for two tracks or more on `axis`: columns across, rows down.
    pub(crate) const fn has_spans(self, axis: Axis) -> bool {
        match axis {
            Axis::X => self.spans_cols,
            Axis::Y => self.spans_rows,
        }
    }

    pub(crate) fn hash_visual<H: hash::Hasher>(&self, tracks: &[Track], h: &mut H) {
        h.write_u32(self.rows.len);
        for t in &tracks[self.rows.range()] {
            t.hash_visual(h);
        }
        h.write_u32(self.cols.len);
        for t in &tracks[self.cols.range()] {
            t.hash_visual(h);
        }
    }
}

#[cfg(test)]
mod tests;
