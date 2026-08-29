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
    const MIN_THEN_MAX: Track = Track::FILL.min(10.0).max(20.0);
    const MAX_THEN_MIN: Track = Track::FILL.max(20.0).min(10.0);
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
        ("negative minimum", || Track::HUG.min(-1.0)),
        ("NaN minimum", || Track::HUG.min(f32::NAN)),
        ("infinite minimum", || Track::HUG.min(f32::INFINITY)),
        ("negative maximum", || Track::HUG.max(-1.0)),
        ("negative infinite maximum", || {
            Track::HUG.max(f32::NEG_INFINITY)
        }),
        ("NaN maximum", || Track::HUG.max(f32::NAN)),
        ("minimum above existing maximum", || {
            Track::HUG.max(10.0).min(11.0)
        }),
        ("maximum below existing minimum", || {
            Track::HUG.min(11.0).max(10.0)
        }),
    ];

    for &(label, build) in cases {
        assert!(
            std::panic::catch_unwind(build).is_err(),
            "case `{label}` must panic",
        );
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
    let tracks = [
        Track::fixed(99.0),
        Track::HUG,
        Track::FILL,
        Track::HUG,
        Track::FILL,
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
