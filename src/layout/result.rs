use crate::primitives::{HugSlice, Rect, Size};
use crate::tree::{NodeId, Tree};

/// Per-frame layout output: `desired` + `rect` indexed by `NodeId.0`, plus
/// the per-grid hug pool measure produces and arrange consumes. Lives inside
/// `LayoutEngine` and is reused across frames (`resize` keeps allocator
/// capacity). The split lets `Tree` stay read-only after recording.
#[derive(Default)]
pub struct LayoutResult {
    desired: Vec<Size>,
    rect: Vec<Rect>,
    /// Flat per-track hug-size pool. One `(row_hugs, col_hugs)` slice per
    /// `GridDef`, addressed by grid def index.
    grid_hug_pool: Vec<f32>,
    grid_hug_slices: Vec<(HugSlice, HugSlice)>,
}

impl LayoutResult {
    pub(super) fn resize_for(&mut self, tree: &Tree) {
        let n = tree.node_count();
        self.desired.clear();
        self.desired.resize(n, Size::ZERO);
        self.rect.clear();
        self.rect.resize(n, Rect::ZERO);

        self.grid_hug_pool.clear();
        self.grid_hug_slices.clear();
        for def in tree.grid_defs() {
            let row_hugs = self.reserve_hugs(def.rows.len());
            let col_hugs = self.reserve_hugs(def.cols.len());
            self.grid_hug_slices.push((row_hugs, col_hugs));
        }
    }

    fn reserve_hugs(&mut self, n: usize) -> HugSlice {
        let start = self.grid_hug_pool.len() as u32;
        self.grid_hug_pool.resize(start as usize + n, 0.0);
        HugSlice {
            start,
            len: n as u32,
        }
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
        let (row, _) = self.grid_hug_slices[idx as usize];
        &self.grid_hug_pool[row.range()]
    }

    pub(super) fn grid_col_hugs(&self, idx: u16) -> &[f32] {
        let (_, col) = self.grid_hug_slices[idx as usize];
        &self.grid_hug_pool[col.range()]
    }

    pub(super) fn grid_row_hugs_mut(&mut self, idx: u16) -> &mut [f32] {
        let (row, _) = self.grid_hug_slices[idx as usize];
        &mut self.grid_hug_pool[row.range()]
    }

    pub(super) fn grid_col_hugs_mut(&mut self, idx: u16) -> &mut [f32] {
        let (_, col) = self.grid_hug_slices[idx as usize];
        &mut self.grid_hug_pool[col.range()]
    }
}
