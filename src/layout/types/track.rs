use crate::layout::types::sizing::Sizing;
use std::rc::Rc;

/// One row or column definition for a `Grid`. Wraps a `Sizing` (Pixel / Auto /
/// Star) with optional `[min, max]` clamps. Defaults: `min = 0.0`,
/// `max = INFINITY` (no clamp).
///
/// `From<Sizing>` lets bare sizing values land in `.cols([Sizing::FILL, …])`
/// without the wrapper.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Track {
    pub size: Sizing,
    pub min: f32,
    pub max: f32,
}

impl Track {
    pub const fn new(size: Sizing) -> Self {
        size.assert_non_negative();
        Self {
            size,
            min: 0.0,
            max: f32::INFINITY,
        }
    }

    pub const fn fixed(v: f32) -> Self {
        Self::new(Sizing::Fixed(v))
    }
    pub const fn hug() -> Self {
        Self::new(Sizing::Hug)
    }
    pub const fn fill() -> Self {
        Self::new(Sizing::FILL)
    }
    pub const fn fill_weight(w: f32) -> Self {
        Self::new(Sizing::Fill(w))
    }

    pub const fn min(mut self, m: f32) -> Self {
        self.min = m;
        self
    }
    pub const fn max(mut self, m: f32) -> Self {
        self.max = m;
        self
    }
}

impl From<Sizing> for Track {
    fn from(s: Sizing) -> Self {
        Self::new(s)
    }
}

impl std::hash::Hash for Track {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.size.hash(h);
        h.write_u32(self.min.to_bits());
        h.write_u32(self.max.to_bits());
    }
}

/// Track definitions + axis gaps for a `Grid` panel. Stored on
/// `GridArena` (a `Tree`-owned `Vec<GridDef>`) and addressed from
/// `LayoutMode::Grid(u16)`. Track defs live behind `Rc<[Track]>` so
/// callers can cache and share them across frames without the
/// framework copying — the builder stores the `Rc`, the layout pass
/// reads through it directly. Per-track hug sizes (computed in
/// measure, read in arrange) live on `Layout` keyed by grid def
/// index — the tree is read-only after recording.
///
/// Lives here (vocabulary, beside [`Track`]) rather than in the grid
/// driver so `forest::tree` can store it without importing the driver
/// — which itself imports `forest::tree`.
#[derive(Clone, Debug)]
pub(crate) struct GridDef {
    pub rows: Rc<[Track]>,
    pub cols: Rc<[Track]>,
    pub row_gap: f32,
    pub col_gap: f32,
}

impl std::hash::Hash for GridDef {
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        h.write_u32(self.rows.len() as u32);
        for t in self.rows.iter() {
            t.hash(h);
        }
        h.write_u32(self.cols.len() as u32);
        for t in self.cols.iter() {
            t.hash(h);
        }
        h.write_u32(self.row_gap.to_bits());
        h.write_u32(self.col_gap.to_bits());
    }
}
