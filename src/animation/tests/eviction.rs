//! Which rows the per-frame sweep drops.

use crate::animation::anim_slot::AnimSlot;
use crate::animation::anim_spec::AnimSpec;
use crate::animation::tests::support::{next_frame, wid};
use crate::animation::*;
use crate::primitives::color::RgbaF32;
use glam::Vec2;

#[test]
fn removed_widget_evicts_all_slots_across_typed_maps() {
    let mut map = AnimMap::default();
    let id = wid("a");
    let other = wid("b");
    let _ = map.typed_mut::<f32>().tick(
        id,
        AnimSlot::new("a"),
        1.0,
        AnimSpec::FAST,
        0.016,
        next_frame(),
    );
    let _ = map.typed_mut::<f32>().tick(
        id,
        AnimSlot::new("b"),
        2.0,
        AnimSpec::FAST,
        0.016,
        next_frame(),
    );
    let _ = map.typed_mut::<Vec2>().tick(
        id,
        AnimSlot::new("a"),
        Vec2::ONE,
        AnimSpec::FAST,
        0.016,
        next_frame(),
    );
    let _ = map.typed_mut::<RgbaF32>().tick(
        id,
        AnimSlot::new("a"),
        RgbaF32::srgb(1.0, 0.0, 0.0),
        AnimSpec::FAST,
        0.016,
        next_frame(),
    );
    let _ = map.typed_mut::<f32>().tick(
        other,
        AnimSlot::new("a"),
        9.0,
        AnimSpec::FAST,
        0.016,
        next_frame(),
    );
    // No `Ui` here — reach into typed maps via `try_typed_mut`
    // (immutable peek goes through the same `as_any_mut` downcast
    // path; we just read `.rows.len()`).
    let f = |m: &mut AnimMap| m.try_typed_mut::<f32>().map_or(0, |t| t.rows.len());
    let v = |m: &mut AnimMap| m.try_typed_mut::<Vec2>().map_or(0, |t| t.rows.len());
    let c = |m: &mut AnimMap| m.try_typed_mut::<RgbaF32>().map_or(0, |t| t.rows.len());
    assert_eq!(f(&mut map), 3);
    assert_eq!(v(&mut map), 1);
    assert_eq!(c(&mut map), 1);

    map.sweep_removed(&WidgetIdSet::from_iter([id]));
    assert_eq!(
        f(&mut map),
        1,
        "scalar slots for `id` must drop, `other` survives",
    );
    assert_eq!(v(&mut map), 0, "vec2 slots for `id` must drop");
    assert_eq!(c(&mut map), 0, "color slots for `id` must drop");
}

/// `post_record` also evicts slots that were *not* poked this frame
/// even when the widget id itself stuck around — without this a
/// `(WidgetId, AnimSlot)` whose owner stopped calling
/// `Ui::animate` would linger forever, since the only other drop
/// trigger is full widget removal.
#[test]
fn post_record_evicts_untouched_slots() {
    let mut map = AnimMap::default();
    let id = wid("a");
    let empty = WidgetIdSet::default();

    // Touch two slots, then run `post_record` to commit a "frame":
    // both rows survive, both `touched` flags clear.
    let _ = map.typed_mut::<f32>().tick(
        id,
        AnimSlot::new("a"),
        1.0,
        AnimSpec::FAST,
        0.016,
        next_frame(),
    );
    let _ = map.typed_mut::<f32>().tick(
        id,
        AnimSlot::new("b"),
        2.0,
        AnimSpec::FAST,
        0.016,
        next_frame(),
    );
    map.sweep_removed(&empty);
    let count = |m: &mut AnimMap| m.try_typed_mut::<f32>().map_or(0, |t| t.rows.len());
    assert_eq!(
        count(&mut map),
        2,
        "both slots must survive the first sweep"
    );

    // Next frame: only poke slot 0. Slot 1 was never re-touched
    // after `post_record` cleared its flag, so it should drop.
    let _ = map.typed_mut::<f32>().tick(
        id,
        AnimSlot::new("a"),
        1.0,
        AnimSpec::FAST,
        0.016,
        next_frame(),
    );
    map.sweep_removed(&empty);
    assert_eq!(
        count(&mut map),
        1,
        "abandoned slot must drop while the still-poked slot survives",
    );

    // Re-poke slot 1 — first-touch path snaps to target. Confirms
    // dropped rows behave like any other never-seen `(id, slot)`.
    let r = map.typed_mut::<f32>().tick(
        id,
        AnimSlot::new("b"),
        99.0,
        AnimSpec::FAST,
        0.016,
        next_frame(),
    );
    assert_eq!(r.current, 99.0);
    assert!(r.settled, "re-touch after eviction is a fresh first-touch");
}
