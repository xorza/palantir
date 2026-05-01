/// Hard cap on track count per axis on a single `Grid`. Shared by the
/// `Grid` builder (inline buffers), the `Tree` track pool, and the layout
/// pass (stack scratch).
pub const MAX_TRACKS: usize = 64;

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

/// Range into `Tree::tracks` for one axis of a `GridDef`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) struct TrackSlice {
    pub start: u32,
    pub len: u32,
}

impl TrackSlice {
    pub(crate) fn range(self) -> std::ops::Range<usize> {
        self.start as usize..(self.start as usize + self.len as usize)
    }
}

/// Track definitions + axis gaps for a `Grid` panel. Stored on `Tree::grid_defs`
/// and addressed from `LayoutMode::Grid(u32)`. Tracks themselves live in a
/// shared `Tree::tracks` pool; the `TrackSlice`s here are `(start, len)` ranges
/// into it. Cleared with `Tree::clear`.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub(crate) struct GridDef {
    pub rows: TrackSlice,
    pub cols: TrackSlice,
    pub row_gap: f32,
    pub col_gap: f32,
}
