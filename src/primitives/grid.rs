use crate::primitives::Track;

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

/// Track definitions + axis gaps for a `Grid` panel. Stored on the `Tree`'s
/// `grid_defs` side-arena and addressed from `LayoutMode::Grid(u32)`. Owns its
/// `Vec<Track>`s so `UiElement` stays `Copy`.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct GridDef {
    pub rows: Vec<Track>,
    pub cols: Vec<Track>,
    pub row_gap: f32,
    pub col_gap: f32,
}
