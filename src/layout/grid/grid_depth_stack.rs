//! The nesting stack that gives each active grid its own scratch slot.

use crate::layout::grid::grid_scratch::GridScratch;

/// Nesting stack of per-depth grid scratch. One `GridScratch` slot per
/// active `LayoutMode::Grid` ancestor. `depth` is the next free slot.
#[derive(Debug, Default)]
pub(crate) struct GridDepthStack {
    scratch: Vec<GridScratch>,
    pub(crate) depth: usize,
}

impl GridDepthStack {
    /// Reserve a scratch slot for the next nesting depth. Grows on first
    /// descent; reuses thereafter.
    pub(super) fn enter(&mut self) -> usize {
        let d = self.depth;
        if self.scratch.len() == d {
            self.scratch.push(GridScratch::default());
        }
        self.depth = d + 1;
        d
    }

    /// An unpaired exit wraps `depth` to `usize::MAX`, and the next
    /// `enter` wraps it back to zero — two nested grids then share one
    /// scratch slot. Debug-only: `enter`/`exit` are the layout engine's
    /// own pairing, run per grid node per frame, so this is the crate
    /// checking itself rather than screening anything a caller passed.
    pub(super) fn exit(&mut self) {
        debug_assert!(self.depth > 0, "GridDepthStack::exit underflow");
        self.depth -= 1;
    }

    pub(super) fn at(&mut self, depth: usize) -> &mut GridScratch {
        &mut self.scratch[depth]
    }
}
