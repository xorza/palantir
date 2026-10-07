//! The nesting stack that gives each active grid its own scratch slot.

use crate::layout::drivers::grid::grid_scratch::GridScratch;

/// Nesting stack of per-depth grid scratch: one `GridScratch` slot per active `LayoutMode::Grid` ancestor; `depth` is the next free slot.
#[derive(Debug, Default)]
pub(crate) struct GridDepthStack {
    scratch: Vec<GridScratch>,
    pub(crate) depth: usize,
}

impl GridDepthStack {
    pub(super) fn enter(&mut self) -> usize {
        let d = self.depth;
        if self.scratch.len() == d {
            self.scratch.push(GridScratch::default());
        }
        self.depth = d + 1;
        d
    }

    /// An unpaired exit wraps `depth` to `usize::MAX` and the next `enter` to zero, so two nested grids share a slot. Debug-only: pairing is the engine's own.
    pub(super) const fn exit(&mut self) {
        debug_assert!(self.depth > 0, "GridDepthStack::exit underflow");
        self.depth -= 1;
    }

    pub(super) fn at(&mut self, depth: usize) -> &mut GridScratch {
        &mut self.scratch[depth]
    }
}
