use crate::primitives::{Rect, Size};
use crate::tree::{NodeId, Tree};

/// `(start, len)` range into `GridHugStore::pool` (per-track hug sizes
/// computed in measure and read in arrange). Tighter than `Range<u32>`
/// because both bounds are inline `Copy` and the slot itself is `Copy`.
#[derive(Clone, Copy, Default)]
struct HugSlice {
    start: u32,
    len: u32,
}

impl HugSlice {
    fn range(self) -> std::ops::Range<usize> {
        self.start as usize..(self.start as usize + self.len as usize)
    }
}

/// Per-frame layout output: `desired` + `rect` indexed by `NodeId.0`, plus
/// the per-grid hug pool measure produces and arrange consumes. Lives inside
/// `LayoutEngine` and is reused across frames (`resize` keeps allocator
/// capacity). The split lets `Tree` stay read-only after recording.
#[derive(Default)]
pub struct LayoutResult {
    desired: Vec<Size>,
    rect: Vec<Rect>,
    grid_hugs: GridHugStore,
}

impl LayoutResult {
    pub(super) fn resize_for(&mut self, tree: &Tree) {
        let n = tree.node_count();
        self.desired.clear();
        self.desired.resize(n, Size::ZERO);
        self.rect.clear();
        self.rect.resize(n, Rect::ZERO);
        self.grid_hugs.reset_for(tree);
    }

    pub fn desired(&self, id: NodeId) -> Size {
        self.desired[id.index()]
    }

    pub fn rect(&self, id: NodeId) -> Rect {
        self.rect[id.index()]
    }

    pub(super) fn set_desired(&mut self, id: NodeId, v: Size) {
        self.desired[id.index()] = v;
    }

    pub(super) fn set_rect(&mut self, id: NodeId, v: Rect) {
        self.rect[id.index()] = v;
    }

    pub(super) fn grid_row_hugs(&self, idx: u16) -> &[f32] {
        self.grid_hugs.rows(idx)
    }

    pub(super) fn grid_col_hugs(&self, idx: u16) -> &[f32] {
        self.grid_hugs.cols(idx)
    }

    pub(super) fn grid_row_hugs_mut(&mut self, idx: u16) -> &mut [f32] {
        self.grid_hugs.rows_mut(idx)
    }

    pub(super) fn grid_col_hugs_mut(&mut self, idx: u16) -> &mut [f32] {
        self.grid_hugs.cols_mut(idx)
    }
}

/// Flat per-track hug-size pool with one `(rows, cols)` slot per recorded
/// `GridDef`. Reset at the start of each layout pass; capacity is retained
/// across frames so steady-state layout is alloc-free.
#[derive(Default)]
struct GridHugStore {
    pool: Vec<f32>,
    slots: Vec<GridHugSlot>,
}

#[derive(Clone, Copy)]
struct GridHugSlot {
    rows: HugSlice,
    cols: HugSlice,
}

impl GridHugStore {
    fn reset_for(&mut self, tree: &Tree) {
        self.pool.clear();
        self.slots.clear();
        for def in tree.grid_defs() {
            let rows = self.alloc(def.rows.len());
            let cols = self.alloc(def.cols.len());
            self.slots.push(GridHugSlot { rows, cols });
        }
    }

    fn alloc(&mut self, n: usize) -> HugSlice {
        let start = self.pool.len() as u32;
        self.pool.resize(start as usize + n, 0.0);
        HugSlice {
            start,
            len: n as u32,
        }
    }

    fn rows(&self, idx: u16) -> &[f32] {
        &self.pool[self.slots[idx as usize].rows.range()]
    }

    fn cols(&self, idx: u16) -> &[f32] {
        &self.pool[self.slots[idx as usize].cols.range()]
    }

    fn rows_mut(&mut self, idx: u16) -> &mut [f32] {
        &mut self.pool[self.slots[idx as usize].rows.range()]
    }

    fn cols_mut(&mut self, idx: u16) -> &mut [f32] {
        &mut self.pool[self.slots[idx as usize].cols.range()]
    }
}
