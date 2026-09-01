use crate::primitives::brush::gradient::FillAxis;
use crate::primitives::brush::gradient::stops::{GradientStops, Stop};
use crate::primitives::brush::gradient::{Interp, Spread};
use crate::primitives::color::ColorU8;
use crate::primitives::fill_kind::FillKind;
use crate::scene::record_store::RecordStore;
use crate::scene::record_store::recorded_gradient::RecordedGradient;
use crate::scene::record_store::recorded_gradients::RecordedGradients;
use glam::Vec2;
use std::panic::AssertUnwindSafe;

#[test]
fn stores_are_isolated() {
    let mut first = RecordStore::default();
    let second = RecordStore::default();
    first.stage_polyline(&[Vec2::new(3.0, 5.0)], &[]);

    assert_eq!(first.polyline_points.as_slice(), &[Vec2::new(3.0, 5.0)]);
    assert!(second.polyline_points.is_empty());
}

/// Two properties, in priority order. **A hit is confirmed by
/// equality**, so two distinct gradients that land on one hash never
/// share an id — that one is correctness, and a shape painted with
/// the wrong gradient is what it buys off. **Dedup is by hash**, so
/// the repeat of a gradient whose key is uncontested returns the id
/// it already has, while a colliding pair each mint a fresh record:
/// wasted rows, never a wrong one.
#[test]
fn gradient_interner_confirms_equality_across_hash_collisions_and_clears() {
    let stops = GradientStops::new([
        Stop::new(0.0, ColorU8::BLACK),
        Stop::new(1.0, ColorU8::WHITE),
    ]);
    let first = RecordedGradient {
        axis: FillAxis::from_lanes(1.0, 0.0, 0.0, 1.0),
        kind: FillKind::linear(Spread::Pad),
        stops,
        interp: Interp::Oklab,
    };
    let colliding = RecordedGradient {
        axis: FillAxis::from_lanes(0.0, 1.0, 0.0, 1.0),
        ..first.clone()
    };
    let mut gradients = RecordedGradients::default();
    let first_id = gradients.intern(7, first.clone());
    // The uncontested repeat: same hash, same content, same id, no
    // second record.
    assert_eq!(gradients.intern(7, first.clone()), first_id);
    assert_eq!(gradients.records.len(), 1);

    // The collision: same hash, different content. Equality
    // confirmation refuses to hand back `first_id`, which is the
    // property that matters, and mints a record of its own.
    let colliding_id = gradients.intern(7, colliding.clone());
    assert_ne!(first_id, colliding_id);
    assert_eq!(gradients.records.len(), 2);
    assert_eq!(gradients.records[colliding_id.0 as usize], colliding);

    // Dedup — and only dedup — is what the collision costs: each of
    // the pair now displaces the other's candidate, so both keep
    // minting records rather than one being wrongly reused.
    assert_ne!(gradients.intern(7, first), first_id);
    assert_ne!(gradients.intern(7, colliding), colliding_id);
    assert_eq!(gradients.records.len(), 4);

    // The reset does not write the index: a slot last frame left under
    // hash 7 has to read as absent on its serial alone. Reused, it would
    // hand back id 3 and the equality confirm would index a `records`
    // that no longer holds it.
    gradients.clear();
    let after_clear = RecordedGradient {
        axis: FillAxis::ZERO,
        kind: FillKind::linear(Spread::Reflect),
        stops,
        interp: Interp::Linear,
    };
    let after_clear_id = gradients.intern(7, after_clear.clone());
    assert_eq!(after_clear_id.0, 0);
    assert_eq!(gradients.records.len(), 1);

    // ...including on the frame the serial wraps, which is the one
    // frame a stale slot could carry the live serial.
    gradients.wind_index_to_last_frame();
    gradients.clear();
    assert_eq!(gradients.intern(7, after_clear).0, 0);
    assert_eq!(gradients.records.len(), 1);
}

/// Dedup holds at every width the index takes, including the ones it
/// passes through while widening mid-frame. Keys are distinct small
/// integers, so each lands in a slot of its own and nothing here
/// exercises collision behaviour by accident.
#[test]
fn gradient_interner_dedups_at_every_table_width() {
    const COUNT: u64 = 200;

    fn gradient(i: u64) -> RecordedGradient {
        RecordedGradient {
            axis: FillAxis::from_lanes(i as f32, 0.0, 0.0, 1.0),
            kind: FillKind::linear(Spread::Pad),
            stops: GradientStops::new([
                Stop::new(0.0, ColorU8::BLACK),
                Stop::new(1.0, ColorU8::WHITE),
            ]),
            interp: Interp::Oklab,
        }
    }

    let mut gradients = RecordedGradients::default();
    // Past 64 slots twice over, so the table widens mid-frame more than
    // once and the repeats below span every width it took.
    for i in 0..COUNT {
        assert_eq!(gradients.intern(i, gradient(i)).0, i as u32);
    }
    assert_eq!(gradients.records.len() as u64, COUNT);

    // The next frame indexes at the width the last one reached, so every
    // repeat dedups and the record count does not move.
    gradients.clear();
    for i in 0..COUNT {
        assert_eq!(gradients.intern(i, gradient(i)).0, i as u32);
    }
    for i in 0..COUNT {
        assert_eq!(gradients.intern(i, gradient(i)).0, i as u32);
    }
    assert_eq!(gradients.records.len() as u64, COUNT);
}

/// A handle from an earlier pass is rejected by both paths that take
/// one, in every build.
///
/// The arena is cleared per pass and the bytes a stale span addresses
/// are gone, so resolving one records whatever text now sits at those
/// offsets — another widget's label under this widget's identity. That
/// is a wrong frame rather than a crash, which is why the panic
/// `InternedStr` and `Ui::fmt` both document is a release one.
///
/// `reuse` is the path with teeth: it copies nothing, so the epoch is
/// the only thing standing between a stale handle and a recorded span.
#[test]
fn a_stale_handle_is_rejected_by_both_paths_in_every_build() {
    let mut store = RecordStore::default();
    let stale = store.intern_str("last frame");
    assert_eq!(store.record_text(stale).span, stale.span);
    assert_eq!(store.reuse(stale).span, stale.span);

    // A new pass retires it.
    store.clear();
    let fresh = store.intern_str("this frame");
    let recorded = std::panic::catch_unwind(AssertUnwindSafe(|| store.record_text(stale)));
    assert!(
        recorded.is_err(),
        "record_text must reject a retired handle"
    );
    let reused = std::panic::catch_unwind(AssertUnwindSafe(|| store.reuse(stale)));
    assert!(reused.is_err(), "reuse must reject a retired handle");
    // The pass's own handle still resolves, so the epoch — not the
    // clear — is what the rejection turns on.
    assert_eq!(store.record_text(fresh).span, fresh.span);
}
