use crate::common::span::Span;
use crate::internals::panic_probe;
use crate::primitives::layout::track::{GridDef, Track};
use crate::primitives::math::domain::{self, EPS};
use std::collections::hash_map::DefaultHasher;
use std::hash::Hasher;

#[test]
fn bounds_accept_valid_ranges_in_either_order() {
    const MIN_THEN_MAX: Track = Track::FILL.with_min(10.0).with_max(20.0);
    const MAX_THEN_MIN: Track = Track::FILL.with_max(20.0).with_min(10.0);
    const PINNED: Track = Track::fixed(5.0).with_min(5.0).with_max(5.0);

    assert_eq!(MIN_THEN_MAX, MAX_THEN_MIN);
    assert_eq!(MIN_THEN_MAX.min, 10.0);
    assert_eq!(MIN_THEN_MAX.max, 20.0);
    assert_eq!(PINNED.min, 5.0);
    assert_eq!(PINNED.max, 5.0);
}

/// A minimum is a length and a maximum an extent, so each panics with its kind's rule. Order is coerced: the minimum wins, raising a lower maximum in either setter order.
#[test]
fn bounds_validate_their_kinds_and_coerce_their_order() {
    type Case = (&'static str, fn() -> Track);

    let cases: &[Case] = &[
        (domain::LENGTH_RULE, || Track::HUG.with_min(-1.0)),
        (domain::LENGTH_RULE, || Track::HUG.with_min(f32::NAN)),
        (domain::LENGTH_RULE, || Track::HUG.with_min(f32::INFINITY)),
        (domain::EXTENT_RULE, || Track::HUG.with_max(-1.0)),
        (domain::EXTENT_RULE, || {
            Track::HUG.with_max(f32::NEG_INFINITY)
        }),
        (domain::EXTENT_RULE, || Track::HUG.with_max(f32::NAN)),
    ];
    for &(expected, build) in cases {
        panic_probe::assert_panics_with(expected, build);
    }

    assert_eq!(Track::HUG.with_max(f32::INFINITY).max, f32::INFINITY);
    let raised = Track::HUG.with_max(10.0).with_min(11.0);
    assert_eq!((raised.min, raised.max), (11.0, 11.0), "max then min");
    let raised = Track::HUG.with_min(11.0).with_max(10.0);
    assert_eq!((raised.min, raised.max), (11.0, 11.0), "min then max");
}

fn grid_content_hash(def: GridDef, tracks: &[Track]) -> u64 {
    let mut hasher = DefaultHasher::new();
    def.hash_visual(tracks, &mut hasher);
    hasher.finish()
}

#[test]
fn grid_content_hash_uses_tracks_not_arena_offsets_and_collapses_visual_noise() {
    let hash_at = |start: u32, noise: f32| {
        let tracks = [
            Track::fixed(99.0),
            Track::HUG.with_min(noise),
            Track::FILL,
            Track::HUG.with_min(noise),
            Track::FILL,
        ];
        let def = GridDef {
            rows: Span::new(start, 1),
            cols: Span::new(start + 1, 1),
        };
        grid_content_hash(def, &tracks)
    };

    assert_eq!(hash_at(1, 0.0), hash_at(3, EPS * 0.5));
    assert_ne!(hash_at(1, 0.0), hash_at(3, EPS * 2.0));
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
        };
        grid_content_hash(def, &tracks)
    }

    let empty = hash_definition(&[], &[]);
    assert_eq!(empty, hash_definition(&[], &[]));
    assert_ne!(empty, hash_definition(&[], &[Track::FILL]));

    let small_rows = [Track::fixed(10.0)];
    let small_cols = [Track::HUG, Track::FILL];
    let small = hash_definition(&small_rows, &small_cols);
    assert_eq!(small, hash_definition(&small_rows, &small_cols));
    assert_ne!(small, hash_definition(&small_cols, &small_rows));

    let large = [Track::FILL; 64];
    let mut changed_large = large;
    changed_large[63] = Track::fixed(1.0);
    assert_eq!(hash_definition(&large, &[]), hash_definition(&large, &[]));
    assert_ne!(
        hash_definition(&large, &[]),
        hash_definition(&changed_large, &[]),
    );
}
