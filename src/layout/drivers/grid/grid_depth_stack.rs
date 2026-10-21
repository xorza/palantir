//! The nesting stack that gives each active grid its own scratch slot.

use crate::layout::drivers::grid::grid_scratch::GridScratch;

/// Nesting stack of per-depth grid scratch: one `GridScratch` slot per active `LayoutMode::Grid` ancestor; `depth` is the next free slot.
#[derive(Debug, Default)]
pub(crate) struct GridDepthStack {
    scratch: Vec<GridScratch>,
    pub(crate) depth: usize,
}

impl GridDepthStack {
    /// Take the next slot for a grid whose measure or arrange begins; [`Self::current`] reads it until the paired [`Self::exit`].
    pub(super) fn enter(&mut self) {
        if self.scratch.len() == self.depth {
            self.scratch.push(GridScratch::default());
        }
        self.depth += 1;
    }

    /// An unpaired exit wraps `depth` to `usize::MAX` and the next `enter` to zero, so two nested grids share a slot. Debug-only: pairing is the engine's own.
    pub(super) const fn exit(&mut self) {
        debug_assert!(self.depth > 0, "GridDepthStack::exit underflow");
        self.depth -= 1;
    }

    /// The innermost entered grid's slot. Nested grids enter and exit inside it, so it holds across a child's measure or arrange.
    pub(super) fn current(&mut self) -> &mut GridScratch {
        debug_assert!(self.depth > 0, "GridDepthStack::current outside any grid");
        &mut self.scratch[self.depth - 1]
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::internals::scratch_report;
    use crate::layout::drivers::grid::grid_depth_stack::GridDepthStack;
    use crate::layout::drivers::grid::grid_scratch::GridScratch;

    impl GridDepthStack {
        pub(crate) fn scratch_bytes(&self) -> usize {
            let Self { scratch, depth: _ } = self;
            scratch_report::vec_bytes(scratch)
                + scratch
                    .iter()
                    .map(GridScratch::scratch_bytes)
                    .sum::<usize>()
        }
    }
}
