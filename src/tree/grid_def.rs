use crate::primitives::track::Track;
use std::rc::Rc;

/// Track definitions + axis gaps for a `Grid` panel. Stored on `GridArena`
/// (a `Tree`-owned `Vec<GridDef>`) and addressed from
/// `LayoutMode::Grid(u16)`. Track defs live behind `Rc<[Track]>` so callers
/// can cache and share them across frames without the framework copying —
/// the builder stores the `Rc`, the layout pass reads through it directly.
/// Per-track hug sizes (computed in measure, read in arrange) live on
/// `LayoutResult` keyed by grid def index — the tree is read-only after
/// recording.
#[derive(Clone, Debug)]
pub(crate) struct GridDef {
    pub rows: Rc<[Track]>,
    pub cols: Rc<[Track]>,
    pub row_gap: f32,
    pub col_gap: f32,
}
