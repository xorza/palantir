use crate::primitives::Track;
use std::rc::Rc;

/// Per-child placement inside a `Grid` parent. Inert when the parent is not a
/// `LayoutMode::Grid`. `(row, col)` is the top-left cell; `(row_span,
/// col_span)` extends the slot toward the bottom-right (defaults to 1×1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridCell {
    pub row: u16,
    pub col: u16,
    pub row_span: u16,
    pub col_span: u16,
}

impl Default for GridCell {
    fn default() -> Self {
        Self {
            row: 0,
            col: 0,
            row_span: 1,
            col_span: 1,
        }
    }
}

/// `(start, len)` range into `Tree::hug_pool` (per-track hug sizes computed
/// in measure and read in arrange).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) struct HugSlice {
    pub start: u32,
    pub len: u32,
}

impl HugSlice {
    pub(crate) fn range(self) -> std::ops::Range<usize> {
        self.start as usize..(self.start as usize + self.len as usize)
    }
}

/// Track definitions + axis gaps for a `Grid` panel. Stored on `Tree::grid_defs`
/// and addressed from `LayoutMode::Grid(u16)`. Track defs live behind
/// `Rc<[Track]>` so callers can cache and share them across frames without
/// the framework copying — the builder stores the `Rc`, the layout pass
/// reads through it directly. Per-track hug sizes (computed in measure, read
/// in arrange) live in `Tree::hug_pool`. All cleared with `Tree::clear`.
#[derive(Clone, Debug)]
pub(crate) struct GridDef {
    pub rows: Rc<[Track]>,
    pub cols: Rc<[Track]>,
    pub row_gap: f32,
    pub col_gap: f32,
    /// Per-row max desired height of span-1 children. Written by
    /// `grid_measure`, read by `arrange_grid`.
    pub row_hugs: HugSlice,
    /// Per-col max desired width of span-1 children. Same semantics as
    /// `row_hugs` on the X axis.
    pub col_hugs: HugSlice,
}
