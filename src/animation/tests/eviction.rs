//! Which rows the per-frame sweep drops.

use crate::animation::tests::support::wid;
use crate::animation::*;
use crate::primitives::paint::color::RgbaF32;
use glam::Vec2;

#[test]
fn removed_widget_evicts_all_slots_across_typed_maps() {
    let mut map = AnimMap::default();
    let id = wid("a");
    let other = wid("b");
    let _ =
        map.typed_mut::<f32>()
            .step(id, AnimationSlot::new("a"), 1.0, AnimationSpec::FAST, 0.016);
    let _ =
        map.typed_mut::<f32>()
            .step(id, AnimationSlot::new("b"), 2.0, AnimationSpec::FAST, 0.016);
    let _ = map.typed_mut::<Vec2>().step(
        id,
        AnimationSlot::new("a"),
        Vec2::ONE,
        AnimationSpec::FAST,
        0.016,
    );
    let _ = map.typed_mut::<RgbaF32>().step(
        id,
        AnimationSlot::new("a"),
        RgbaF32::srgb(1.0, 0.0, 0.0),
        AnimationSpec::FAST,
        0.016,
    );
    let _ = map.typed_mut::<f32>().step(
        other,
        AnimationSlot::new("a"),
        9.0,
        AnimationSpec::FAST,
        0.016,
    );
    let f = |m: &mut AnimMap| m.row_count::<f32>();
    let v = |m: &mut AnimMap| m.row_count::<Vec2>();
    let c = |m: &mut AnimMap| m.row_count::<RgbaF32>();
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

    // A typed map its sweep drained goes with its last row (`Drained::Drop`),
    // which is what lets `Ui::animate`'s empty fast path come back.
    map.sweep_removed(&WidgetIdSet::from_iter([other]));
    assert!(map.is_empty(), "every typed map drained, so none is kept");
}

/// `post_record` also evicts slots that were *not* poked this frame
/// even when the widget id itself stuck around — without this a
/// `(WidgetId, AnimationSlot)` whose owner stopped calling
/// `Ui::animate` would linger forever, since the only other drop
/// trigger is full widget removal.
#[test]
fn post_record_evicts_untouched_slots() {
    let mut map = AnimMap::default();
    let id = wid("a");
    let empty = WidgetIdSet::default();

    // Touch two slots, then run `post_record` to commit a "frame":
    // both rows survive, both `touched` flags clear.
    let _ =
        map.typed_mut::<f32>()
            .step(id, AnimationSlot::new("a"), 1.0, AnimationSpec::FAST, 0.016);
    let _ =
        map.typed_mut::<f32>()
            .step(id, AnimationSlot::new("b"), 2.0, AnimationSpec::FAST, 0.016);
    map.sweep_removed(&empty);
    let count = |m: &mut AnimMap| m.row_count::<f32>();
    assert_eq!(
        count(&mut map),
        2,
        "both slots must survive the first sweep"
    );

    // Next frame: only poke slot 0. Slot 1 was never re-touched
    // after `post_record` cleared its flag, so it should drop.
    let _ =
        map.typed_mut::<f32>()
            .step(id, AnimationSlot::new("a"), 1.0, AnimationSpec::FAST, 0.016);
    map.sweep_removed(&empty);
    assert_eq!(
        count(&mut map),
        1,
        "abandoned slot must drop while the still-poked slot survives",
    );

    // Re-poke slot 1 — first-touch path snaps to target. Confirms
    // dropped rows behave like any other never-seen `(id, slot)`.
    let r = map.typed_mut::<f32>().step(
        id,
        AnimationSlot::new("b"),
        99.0,
        AnimationSpec::FAST,
        0.016,
    );
    assert_eq!(r.current, 99.0);
    assert!(r.settled, "re-touch after eviction is a fresh first-touch");
}
