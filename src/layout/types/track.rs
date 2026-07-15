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
    pub(crate) size: Sizing,
    pub(crate) min: f32,
    pub(crate) max: f32,
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

    /// Set the lower size clamp.
    ///
    /// # Panics
    ///
    /// Panics if `min` is negative, NaN, or greater than the current maximum.
    pub const fn min(mut self, min: f32) -> Self {
        assert!(
            min >= 0.0 && min <= self.max,
            "Track minimum must be non-negative and not exceed its maximum",
        );
        self.min = min;
        self
    }

    /// Set the upper size clamp.
    ///
    /// # Panics
    ///
    /// Panics if `max` is negative, NaN, or less than the current minimum.
    pub const fn max(mut self, max: f32) -> Self {
        assert!(
            max >= 0.0 && max >= self.min,
            "Track maximum must be non-negative and not be less than its minimum",
        );
        self.max = max;
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
        h.write(bytemuck::bytes_of(&[self.min, self.max]));
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
        h.write(bytemuck::bytes_of(&[self.row_gap, self.col_gap]));
    }
}

#[cfg(test)]
mod tests {
    use super::Track;

    #[test]
    fn bounds_accept_valid_ranges_in_either_order() {
        const MIN_THEN_MAX: Track = Track::fill().min(10.0).max(20.0);
        const MAX_THEN_MIN: Track = Track::fill().max(20.0).min(10.0);
        const PINNED: Track = Track::fixed(5.0).min(5.0).max(5.0);

        assert_eq!(MIN_THEN_MAX, MAX_THEN_MIN);
        assert_eq!(MIN_THEN_MAX.min, 10.0);
        assert_eq!(MIN_THEN_MAX.max, 20.0);
        assert_eq!(PINNED.min, 5.0);
        assert_eq!(PINNED.max, 5.0);
    }

    #[test]
    fn bounds_reject_invalid_values_and_inverted_setter_orders() {
        type Case = (&'static str, fn() -> Track);

        let cases: &[Case] = &[
            ("negative minimum", || Track::hug().min(-1.0)),
            ("NaN minimum", || Track::hug().min(f32::NAN)),
            ("negative maximum", || Track::hug().max(-1.0)),
            ("NaN maximum", || Track::hug().max(f32::NAN)),
            ("minimum above existing maximum", || {
                Track::hug().max(10.0).min(11.0)
            }),
            ("maximum below existing minimum", || {
                Track::hug().min(11.0).max(10.0)
            }),
        ];

        for &(label, build) in cases {
            assert!(
                std::panic::catch_unwind(build).is_err(),
                "case `{label}` must panic",
            );
        }
    }
}
