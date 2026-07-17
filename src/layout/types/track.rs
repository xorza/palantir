use crate::layout::types::sizing::Sizing;
use crate::primitives::approx;
use crate::primitives::span::Span;

/// One row or column definition for a `Grid`. Wraps a `Sizing` (Pixel / Auto /
/// Star) with optional `[min, max]` clamps. Defaults: `min = 0.0`,
/// `max = INFINITY` (no clamp).
///
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Track {
    pub(crate) size: Sizing,
    pub(crate) min: f32,
    pub(crate) max: f32,
}

impl Track {
    pub const fn new(size: Sizing) -> Self {
        Self {
            size,
            min: 0.0,
            max: f32::INFINITY,
        }
    }

    pub const fn fixed(v: f32) -> Self {
        Self::new(Sizing::fixed(v))
    }
    pub const fn hug() -> Self {
        Self::new(Sizing::HUG)
    }
    pub const fn fill() -> Self {
        Self::new(Sizing::FILL)
    }
    pub const fn fill_weight(w: f32) -> Self {
        Self::new(Sizing::fill(w))
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

    #[inline]
    pub(crate) fn hash_visual<H: std::hash::Hasher>(&self, h: &mut H) {
        self.size.hash_visual(h);
        approx::hash_visual_f32(self.min, h);
        approx::hash_visual_f32(self.max, h);
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
        approx::hash_f32(self.min, h);
        approx::hash_f32(self.max, h);
    }
}

/// Spans into a `Tree`'s retained flat track arena plus the gaps for one Grid.
#[derive(Clone, Copy, Debug)]
pub(crate) struct GridDef {
    pub rows: Span,
    pub cols: Span,
    pub row_gap: f32,
    pub col_gap: f32,
}

impl GridDef {
    pub(crate) fn hash_visual<H: std::hash::Hasher>(&self, tracks: &[Track], h: &mut H) {
        h.write_u32(self.rows.len);
        for t in &tracks[self.rows.range()] {
            t.hash_visual(h);
        }
        h.write_u32(self.cols.len);
        for t in &tracks[self.cols.range()] {
            t.hash_visual(h);
        }
        approx::hash_visual_f32(self.row_gap, h);
        approx::hash_visual_f32(self.col_gap, h);
    }
}

#[cfg(test)]
mod tests {
    use crate::layout::types::sizing::Sizing;
    use crate::layout::types::track::{GridDef, Track};
    use crate::primitives::approx::EPS;
    use crate::primitives::span::Span;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    fn hash_value(value: impl Hash) -> u64 {
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        hasher.finish()
    }

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

        let positive_zero = Track::new(Sizing::fixed(0.0)).min(0.0);
        let negative_zero = Track::new(Sizing::fixed(-0.0)).min(-0.0);
        assert_eq!(positive_zero, negative_zero);
        assert_eq!(hash_value(positive_zero), hash_value(negative_zero));
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

    fn grid_content_hash(def: GridDef, tracks: &[Track]) -> u64 {
        let mut hasher = DefaultHasher::new();
        def.hash_visual(tracks, &mut hasher);
        hasher.finish()
    }

    #[test]
    fn grid_content_hash_uses_tracks_not_arena_offsets_and_collapses_visual_noise() {
        let tracks = [
            Track::fixed(99.0),
            Track::hug(),
            Track::fill(),
            Track::hug(),
            Track::fill(),
        ];
        let make = |start, row_gap| GridDef {
            rows: Span::new(start, 1),
            cols: Span::new(start + 1, 1),
            row_gap,
            col_gap: -row_gap,
        };

        assert_eq!(
            grid_content_hash(make(1, 0.0), &tracks),
            grid_content_hash(make(3, EPS * 0.5), &tracks),
        );
        assert_ne!(
            grid_content_hash(make(1, 0.0), &tracks),
            grid_content_hash(make(3, EPS * 2.0), &tracks),
        );
    }

    #[test]
    fn grid_content_hash_covers_empty_small_and_large_definitions() {
        fn hash_definition(rows: &[Track], cols: &[Track]) -> u64 {
            let mut tracks = Vec::with_capacity(rows.len() + cols.len());
            tracks.extend_from_slice(rows);
            tracks.extend_from_slice(cols);
            let def = GridDef {
                rows: Span::new(0, rows.len() as u32),
                cols: Span::new(rows.len() as u32, cols.len() as u32),
                row_gap: 2.0,
                col_gap: 3.0,
            };
            grid_content_hash(def, &tracks)
        }

        let empty = hash_definition(&[], &[]);
        assert_eq!(empty, hash_definition(&[], &[]));
        assert_ne!(empty, hash_definition(&[], &[Track::fill()]));

        let small_rows = [Track::fixed(10.0)];
        let small_cols = [Track::hug(), Track::fill()];
        let small = hash_definition(&small_rows, &small_cols);
        assert_eq!(small, hash_definition(&small_rows, &small_cols));
        assert_ne!(small, hash_definition(&small_cols, &small_rows));

        let large = [Track::fill(); 64];
        let mut changed_large = large;
        changed_large[63] = Track::fixed(1.0);
        assert_eq!(hash_definition(&large, &[]), hash_definition(&large, &[]));
        assert_ne!(
            hash_definition(&large, &[]),
            hash_definition(&changed_large, &[]),
        );
    }
}
