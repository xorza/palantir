//! Which rows the per-frame sweep drops.

use crate::animation::AnimationMap;
use crate::animation::animation_map_typed::AnimationClock;
use crate::animation::animation_request::AnimationRequest;
use crate::animation::animation_slot::AnimationSlot;
use crate::animation::animation_spec::AnimationSpec;
use crate::animation::tests::support;
use crate::primitives::math::animatable::Animatable;
use crate::primitives::paint::color::RgbaF32;
use glam::Vec2;

/// One tick of `slot` under `id` on render frame `frame`; the value it reads.
fn tick<T: Animatable>(
    map: &mut AnimationMap,
    frame: u64,
    id: &'static str,
    slot: &'static str,
    target: T,
) -> T {
    let request = AnimationRequest::new(support::widget_id(id), AnimationSlot::new(slot), target);
    let clock = AnimationClock {
        dt: 0.016,
        render_frame_id: frame,
    };
    map.animate(request, AnimationSpec::FAST, clock).value
}

/// A sweep keeps exactly the rows its frame ticked, in every typed map: a slot its caller stops
/// asking for drops with its widget still recorded, a map that drains is dropped, and a row
/// ticked again after its eviction starts fresh at its target.
#[test]
fn a_sweep_keeps_the_rows_its_frame_ticked() {
    let mut map = AnimationMap::default();
    let rows = |m: &mut AnimationMap| {
        [
            m.row_count::<f32>(),
            m.row_count::<Vec2>(),
            m.row_count::<RgbaF32>(),
        ]
    };

    tick(&mut map, 1, "a", "x", 1.0_f32);
    tick(&mut map, 1, "a", "y", 2.0_f32);
    tick(&mut map, 1, "a", "x", Vec2::ONE);
    let red = RgbaF32::srgb(1.0, 0.0, 0.0);
    assert_eq!(tick(&mut map, 1, "a", "x", red), red, "a first tick snaps");
    tick(&mut map, 1, "b", "x", 9.0_f32);
    map.sweep(1);
    assert_eq!(rows(&mut map), [3, 1, 1], "frame 1 ticked every row");

    // A second pass of one frame ticks a row again under the same frame id.
    tick(&mut map, 2, "a", "x", 1.0_f32);
    tick(&mut map, 2, "a", "x", 1.0_f32);
    tick(&mut map, 2, "b", "x", 9.0_f32);
    map.sweep(2);
    assert_eq!(
        rows(&mut map),
        [2, 0, 0],
        "`a`'s slot `y` and the two other maps drop"
    );

    map.sweep(3);
    assert!(
        map.is_empty(),
        "frame 3 ticked nothing, so no typed map is kept"
    );

    // Kept, `y` would move from 2 toward 99. Evicted, its first tick snaps to 99.
    assert_eq!(tick(&mut map, 4, "a", "y", 99.0_f32), 99.0);
}
