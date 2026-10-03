use crate::common::span::Span;
use crate::internals::panic_probe;
use crate::primitives::layout::track::{GridDef, Track};
use crate::primitives::math::approx::EPS;
use std::collections::hash_map::DefaultHasher;
use std::hash::Hasher;

#[test]
fn bounds_accept_valid_ranges_in_either_order() {
    const MIN_THEN_MAX: Track = Track::FILL.min(10.0).max(20.0);
    const MAX_THEN_MIN: Track = Track::FILL.max(20.0).min(10.0);
    const PINNED: Track = Track::fixed(5.0).min(5.0).max(5.0);

    assert_eq!(MIN_THEN_MAX, MAX_THEN_MIN);
    assert_eq!(MIN_THEN_MAX.min, 10.0);
    assert_eq!(MIN_THEN_MAX.max, 20.0);
    assert_eq!(PINNED.min, 5.0);
    assert_eq!(PINNED.max, 5.0);
}

#[test]
fn bounds_reject_invalid_values_and_inverted_setter_orders() {
    const MIN: &str = "Track minimum must be finite, non-negative, and not exceed its maximum";
    const MAX: &str = "Track maximum must be non-negative and not be less than its minimum";
    type Case = (&'static str, fn() -> Track);

    let cases: &[Case] = &[
        (MIN, || Track::HUG.min(-1.0)),
        (MIN, || Track::HUG.min(f32::NAN)),
        (MIN, || Track::HUG.min(f32::INFINITY)),
        (MAX, || Track::HUG.max(-1.0)),
        (MAX, || Track::HUG.max(f32::NEG_INFINITY)),
        (MAX, || Track::HUG.max(f32::NAN)),
        // Minimum above an existing maximum.
        (MIN, || Track::HUG.max(10.0).min(11.0)),
        // Maximum below an existing minimum.
        (MAX, || Track::HUG.min(11.0).max(10.0)),
    ];

    for &(expected, build) in cases {
        panic_probe::assert_panics_with(expected, build);
    }

    assert_eq!(Track::HUG.max(f32::INFINITY).max, f32::INFINITY);
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
            Track::HUG.min(noise),
            Track::FILL,
            Track::HUG.min(noise),
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
