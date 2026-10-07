//! Snap-marked fields carry their target immediately; only their own velocity clears.

use crate::animation::anim_map_typed::AnimMapTyped;
use crate::animation::animation_spec::AnimationSpec;
use crate::animation::tests::support::{
    AnimUi, SLOT, closed_form_settle_step, setup_anim_ui, spring_velocity, wid,
};
use crate::internals::harness::UiHarness;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::paint::color::RgbaF32;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
use std::time::Duration;

/// `#[animate(snap)]` fields update on retarget mid-spring, not on settle (`Background.radius` is snap).
#[test]
fn spring_snap_fields_carry_target_immediately() {
    use crate::primitives::geometry::corners::Corners;
    use crate::primitives::paint::background::Background;
    use crate::primitives::paint::shadow::Shadow;
    use crate::primitives::paint::stroke::Stroke;

    let mut map = AnimMapTyped::<Background>::default();
    let id = wid("snap-carry");
    let start = Background {
        fill: RgbaF32::srgb(0.0, 0.0, 0.0).into(),
        border: Stroke::NONE,
        corners: Corners::all(2.0),
        shadow: Shadow::NONE,
    };
    let _ = map.step(id, SLOT, start.clone(), AnimationSpec::SPRING, 0.016);

    // Retarget fill (animated) and radius (snap): from rest the change's frame steps nothing and shows the start.
    let target = Background {
        fill: RgbaF32::srgb(1.0, 0.0, 0.0).into(),
        border: Stroke::NONE,
        corners: Corners::all(12.0),
        shadow: Shadow::NONE,
    };
    let r = map.step(id, SLOT, target.clone(), AnimationSpec::SPRING, 0.016);
    assert_eq!(r.current, start, "the change's frame shows the start");
    let r = map.step(id, SLOT, target.clone(), AnimationSpec::SPRING, 0.016);
    assert!(
        !r.settled,
        "spring with a real fill diff must remain in flight after one step",
    );
    assert_eq!(
        r.current.corners, target.corners,
        "snap field must carry target value on the first stepped frame, not lag until settle",
    );
    assert!(
        r.current.fill.as_solid().unwrap().r < target.fill.as_solid().unwrap().r - 0.05,
        "animated fill should still be mid-flight; got {:?}",
        r.current.fill,
    );
}

#[test]
fn gradient_snap_clears_only_its_background_velocity() {
    use crate::primitives::geometry::corners::Corners;
    use crate::primitives::paint::background::Background;
    use crate::primitives::paint::brush::Brush;
    use crate::primitives::paint::brush::gradient::linear_geometry::LinearGradient;
    use crate::primitives::paint::shadow::Shadow;
    use crate::primitives::paint::stroke::Stroke;

    let mut map = AnimMapTyped::<Background>::default();
    let id = wid("gradient-background-velocity");
    let start = Background {
        fill: Brush::Solid(RgbaF32::BLACK),
        border: Stroke::new(RgbaF32::BLACK, 0.0),
        corners: Corners::ZERO,
        shadow: Shadow::NONE,
    };
    let moving = Background {
        fill: Brush::Solid(RgbaF32::WHITE),
        border: Stroke::new(RgbaF32::BLACK, 10.0),
        corners: Corners::ZERO,
        shadow: Shadow::NONE,
    };
    let _ = map.step(id, SLOT, start, AnimationSpec::SPRING, 0.0);
    for _ in 0..3 {
        let _ = map.step(id, SLOT, moving.clone(), AnimationSpec::SPRING, 0.016);
    }
    let stroke_velocity = spring_velocity(&map.rows[&(id, SLOT)]).border.width;
    assert!(
        stroke_velocity > 0.0,
        "test setup must carry positive stroke velocity",
    );

    let gradient = Brush::Linear(LinearGradient::two_stop(
        0.0,
        RgbaF32::BLACK,
        RgbaF32::WHITE,
    ));
    let target = Background {
        fill: gradient.clone(),
        border: Stroke::new(RgbaF32::BLACK, 20.0),
        corners: Corners::ZERO,
        shadow: Shadow::NONE,
    };
    let result = map.step(id, SLOT, target, AnimationSpec::SPRING, 0.0);
    let row = &map.rows[&(id, SLOT)];
    let velocity = spring_velocity(row);
    assert_eq!(result.current.fill, gradient);
    assert_eq!(velocity.fill, Brush::TRANSPARENT);
    assert_eq!(velocity.border.width, stroke_velocity);
    assert!(
        !result.settled,
        "the independently animated stroke still has real displacement",
    );
}

#[test]
fn gradient_snap_inside_look_repaints_only_until_numeric_fields_settle() {
    use crate::primitives::paint::background::Background;
    use crate::primitives::paint::brush::Brush;
    use crate::primitives::paint::brush::gradient::radial_geometry::RadialGradient;
    use crate::widget_core::widget_look::animated_look::AnimatedLook;
    use crate::widgets::theme::text_style::TextStyle;

    let AnimUi { mut h, id } = setup_anim_ui("gradient-look-settle");
    let start = AnimatedLook {
        background: Background::fill(RgbaF32::BLACK),
        text: TextStyle::default().with_color(RgbaF32::BLACK),
    };
    let gradient = Brush::Radial(RadialGradient::two_stop(RgbaF32::BLACK, RgbaF32::WHITE));
    let target = AnimatedLook {
        background: Background::fill(gradient.clone()),
        text: TextStyle::default().with_color(RgbaF32::WHITE),
    };

    let frame = |h: &mut UiHarness, look: &AnimatedLook| {
        h.frame_passes(|ui| {
            let current = ui.animate(id, SLOT, look.clone(), Some(AnimationSpec::SPRING));
            Block::new()
                .id(WidgetId::from_hash("gradient-look-settle"))
                .show(ui);
            current
        })
    };

    let first = frame(&mut h, &start);
    assert_eq!(*first.a(), start);
    assert!(!first.report().repaint_requested);

    let tick = Duration::from_millis(16);
    let retarget = frame(h.advance(tick), &target);
    assert_eq!(retarget.a().background.fill, gradient);
    assert_ne!(retarget.a().text.color, target.text.color);
    assert!(retarget.report().repaint_requested);

    let mut last = None;
    let frames = h.frames_until_idle(600, tick, |ui| {
        let current = ui.animate(id, SLOT, target.clone(), Some(AnimationSpec::SPRING));
        assert_eq!(current.background.fill, gradient);
        Block::new()
            .id(WidgetId::from_hash("gradient-look-settle"))
            .show(ui);
        last = Some(current);
    });
    // Text colour moves black → white, √3 in linear RGB; frame `n` after the retarget is step `n`.
    let step = closed_form_settle_step(170.0, 26.0, 3.0f64.sqrt(), 1.0 / 4096.0, |_| 0.016);
    assert_eq!(step, 56);
    assert_eq!(frames, Some(step), "the look's color spring settles");
    assert_eq!(last, Some(target.clone()));

    let after_settle = frame(h.advance(tick), &target);
    assert_eq!(*after_settle.a(), target);
    assert!(
        !after_settle.report().repaint_requested,
        "a settled look must not request a surplus repaint",
    );
}
