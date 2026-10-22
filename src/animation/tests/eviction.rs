//! Which rows the per-frame sweep drops.

use crate::animation::AnimationMap;
use crate::animation::animation_request::AnimationRequest;
use crate::animation::animation_slot::AnimationSlot;
use crate::animation::animation_spec::AnimationSpec;
use crate::animation::tests::support;
use crate::primitives::identity::widget_id::WidgetIdSet;
use crate::primitives::paint::color::RgbaF32;
use glam::Vec2;

#[test]
fn removed_widget_evicts_all_slots_across_typed_maps() {
    let mut map = AnimationMap::default();
    let id = support::widget_id("a");
    let other = support::widget_id("b");
    let _ = map.typed_mut::<f32>().step(
        AnimationRequest::new(id, AnimationSlot::new("a"), 1.0),
        AnimationSpec::FAST,
        0.016,
    );
    let _ = map.typed_mut::<f32>().step(
        AnimationRequest::new(id, AnimationSlot::new("b"), 2.0),
        AnimationSpec::FAST,
        0.016,
    );
    let _ = map.typed_mut::<Vec2>().step(
        AnimationRequest::new(id, AnimationSlot::new("a"), Vec2::ONE),
        AnimationSpec::FAST,
        0.016,
    );
    let _ = map.typed_mut::<RgbaF32>().step(
        AnimationRequest::new(id, AnimationSlot::new("a"), RgbaF32::srgb(1.0, 0.0, 0.0)),
        AnimationSpec::FAST,
        0.016,
    );
    let _ = map.typed_mut::<f32>().step(
        AnimationRequest::new(other, AnimationSlot::new("a"), 9.0),
        AnimationSpec::FAST,
        0.016,
    );
    let f = |m: &mut AnimationMap| m.row_count::<f32>();
    let v = |m: &mut AnimationMap| m.row_count::<Vec2>();
    let c = |m: &mut AnimationMap| m.row_count::<RgbaF32>();
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

    map.sweep_removed(&WidgetIdSet::from_iter([other]));
    assert!(map.is_empty(), "every typed map drained, so none is kept");
}

/// `post_record` also evicts slots not poked this frame even when the widget id stays.
#[test]
fn post_record_evicts_untouched_slots() {
    let mut map = AnimationMap::default();
    let id = support::widget_id("a");
    let empty = WidgetIdSet::default();

    let _ = map.typed_mut::<f32>().step(
        AnimationRequest::new(id, AnimationSlot::new("a"), 1.0),
        AnimationSpec::FAST,
        0.016,
    );
    let _ = map.typed_mut::<f32>().step(
        AnimationRequest::new(id, AnimationSlot::new("b"), 2.0),
        AnimationSpec::FAST,
        0.016,
    );
    map.sweep_removed(&empty);
    let count = |m: &mut AnimationMap| m.row_count::<f32>();
    assert_eq!(
        count(&mut map),
        2,
        "both slots must survive the first sweep"
    );

    let _ = map.typed_mut::<f32>().step(
        AnimationRequest::new(id, AnimationSlot::new("a"), 1.0),
        AnimationSpec::FAST,
        0.016,
    );
    map.sweep_removed(&empty);
    assert_eq!(
        count(&mut map),
        1,
        "abandoned slot must drop while the still-poked slot survives",
    );

    let r = map.typed_mut::<f32>().step(
        AnimationRequest::new(id, AnimationSlot::new("b"), 99.0),
        AnimationSpec::FAST,
        0.016,
    );
    assert_eq!(r.cur, 99.0);
    assert!(r.settled, "re-touch after eviction is a fresh first-touch");
}
