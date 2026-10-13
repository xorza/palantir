//! The interned definition a node carries a whole grid by.

use crate::common::span::Span;
use crate::primitives::layout::track::Track;
use crate::primitives::math::float_hash::FloatHash;
use std::hash;

/// One Grid's rows and columns, as spans into a `Tree`'s retained flat track arena.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GridDef {
    pub(crate) rows: Span,
    pub(crate) cols: Span,
}

impl GridDef {
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
