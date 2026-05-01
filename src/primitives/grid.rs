/// Hard cap on track count per axis at *builder* construction time. The
/// `Grid` builder uses `ArrayVec<Track, MAX_TRACKS>` for inline track
/// storage — bounded so the builder is fully stack-resident. Lift this if
/// you need wider grids; the layout pass and `Tree` track pool have no
/// such cap.
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

/// Generic `(start, len)` range into one of `Tree`'s per-frame pools.
/// Used for tracks (`Tree::tracks`) and per-track hug sizes (`Tree::hug_pool`).
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
/// and addressed from `LayoutMode::Grid(u32)`. Track defs live in `Tree::tracks`;
/// per-track hug sizes (computed in measure, read in arrange) live in
/// `Tree::hug_pool`. All cleared with `Tree::clear`.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub(crate) struct GridDef {
    pub rows: TrackSlice,
    pub cols: TrackSlice,
    pub row_gap: f32,
    pub col_gap: f32,
    /// Per-row max desired height of span-1 children. Written by
    /// `grid_measure`, read by `arrange_grid`.
    pub row_hugs: TrackSlice,
    /// Per-col max desired width of span-1 children. Same semantics as
    /// `row_hugs` on the X axis.
    pub col_hugs: TrackSlice,
}
