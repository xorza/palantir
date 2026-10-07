use crate::internals::panic_probe;
use crate::primitives::packed::fill_axis::FillAxis;
use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::paint::brush::gradient::color_ramp::ColorRamp;
use crate::primitives::paint::brush::gradient::{Interpolation, Spread};
use crate::primitives::paint::color::RgbaF32;
use crate::scene::record_store::RecordStore;
use crate::scene::record_store::recorded_gradient::RecordedGradient;
use crate::scene::record_store::recorded_gradients::RecordedGradients;
use glam::Vec2;

#[test]
fn stores_are_isolated() {
    let mut first = RecordStore::default();
    let second = RecordStore::default();
    first.stage_polyline(&[Vec2::new(3.0, 5.0)], &[], RgbaF32::WHITE);

    assert_eq!(first.polyline_points.as_slice(), &[Vec2::new(3.0, 5.0)]);
    assert!(second.polyline_points.is_empty());
}

/// A hit is confirmed by equality, so distinct gradients on one hash never share an id; dedup is by hash, so a colliding pair each mint a fresh record (wasted rows, never a wrong one).
#[test]
fn gradient_interner_confirms_equality_across_hash_collisions_and_clears() {
    let ramp = ColorRamp::two_stop(RgbaF32::BLACK, RgbaF32::WHITE);
    let first = RecordedGradient {
        axis: FillAxis::from_lanes(1.0, 0.0, 0.0, 1.0),
        kind: FillKind::linear(Spread::Pad),
        ramp,
    };
    let colliding = RecordedGradient {
        axis: FillAxis::from_lanes(0.0, 1.0, 0.0, 1.0),
        ..first.clone()
    };
    let mut gradients = RecordedGradients::default();
    let first_id = gradients.intern(7, first.clone());
    assert_eq!(gradients.intern(7, first.clone()), first_id);
    assert_eq!(gradients.records.len(), 1);

    // Collision: same hash, different content; equality refuses `first_id` and mints a record.
    let colliding_id = gradients.intern(7, colliding.clone());
    assert_ne!(first_id, colliding_id);
    assert_eq!(gradients.records.len(), 2);
    assert_eq!(gradients.records[colliding_id.0 as usize], colliding);

    assert_ne!(gradients.intern(7, first), first_id);
    assert_ne!(gradients.intern(7, colliding), colliding_id);
    assert_eq!(gradients.records.len(), 4);

    // The reset does not write the index: a slot left under hash 7 must read as absent on its serial alone.
    gradients.clear();
    let after_clear = RecordedGradient {
        axis: FillAxis::ZERO,
        kind: FillKind::linear(Spread::Reflect),
        ramp: ramp.with_interpolation(Interpolation::Linear),
    };
    let after_clear_id = gradients.intern(7, after_clear.clone());
    assert_eq!(after_clear_id.0, 0);
    assert_eq!(gradients.records.len(), 1);

    // ...including the frame the serial wraps.
    gradients.wind_index_to_last_frame();
    gradients.clear();
    assert_eq!(gradients.intern(7, after_clear).0, 0);
    assert_eq!(gradients.records.len(), 1);
}

/// Dedup holds at every width the index takes, including mid-frame widening.
#[test]
fn gradient_interner_dedups_at_every_table_width() {
    const COUNT: u64 = 200;

    fn gradient(i: u64) -> RecordedGradient {
        RecordedGradient {
            axis: FillAxis::from_lanes(i as f32, 0.0, 0.0, 1.0),
            kind: FillKind::linear(Spread::Pad),
            ramp: ColorRamp::two_stop(RgbaF32::BLACK, RgbaF32::WHITE),
        }
    }

    let mut gradients = RecordedGradients::default();
    for i in 0..COUNT {
        assert_eq!(gradients.intern(i, gradient(i)).0, i as u32);
    }
    assert_eq!(gradients.records.len() as u64, COUNT);

    gradients.clear();
    for i in 0..COUNT {
        assert_eq!(gradients.intern(i, gradient(i)).0, i as u32);
    }
    for i in 0..COUNT {
        assert_eq!(gradients.intern(i, gradient(i)).0, i as u32);
    }
    assert_eq!(gradients.records.len() as u64, COUNT);
}

/// A handle from an earlier pass is rejected by both paths that take one, in every build: the arena is cleared per pass, so a stale span would record another widget's text (hence the release panic `InternedStr` and `Ui::fmt` document). `reuse` copies nothing, so the epoch alone guards it.
#[test]
fn a_stale_handle_is_rejected_by_both_paths_in_every_build() {
    let mut store = RecordStore::default();
    let stale = store.intern_str("last frame");
    assert_eq!(store.record_text(stale).span, stale.span);
    assert_eq!(store.reuse(stale).span, stale.span);

    store.clear();
    let fresh = store.intern_str("this frame");
    panic_probe::assert_panics_with(
        "InternedStr outlived the record pass that minted it",
        || store.record_text(stale),
    );
    panic_probe::assert_panics_with(
        "InternedStr outlived the record pass that minted it",
        || store.reuse(stale),
    );
    // The pass's own handle still resolves, so the epoch drives the rejection.
    assert_eq!(store.record_text(fresh).span, fresh.span);
}
