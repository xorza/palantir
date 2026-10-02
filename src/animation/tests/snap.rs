//! Fields marked snap carry their target immediately, and only their own
//! velocity clears.

use crate::animation::anim_map_typed::AnimMapTyped;
use crate::animation::anim_spec::AnimSpec;
use crate::animation::tests::support::{
    AnimUi, SLOT, next_frame, setup_anim_ui, spring_velocity, wid,
};
use crate::primitives::color::RgbaF32;
use crate::primitives::widget_id::WidgetId;
use crate::ui::harness::UiHarness;
use crate::widgets::block::Block;
use crate::widgets::configure::Configure;
use std::time::Duration;

/// Pin: `#[animate(snap)]` fields update on retarget mid-spring, not
/// on settle. `Background.radius` is snap; without the
/// `lerp(_, target, 0.0)` carry in spring `step`, the new radius
/// would only land when the spring snaps to target.
#[test]
fn spring_snap_fields_carry_target_immediately() {
    use crate::primitives::background::Background;
    use crate::primitives::corners::Corners;
    use crate::primitives::shadow::Shadow;
    use crate::primitives::stroke::Stroke;

    let mut map = AnimMapTyped::<Background>::default();
    let id = wid("snap-carry");
    let start = Background {
        fill: RgbaF32::srgb(0.0, 0.0, 0.0).into(),
        border: Stroke::ZERO,
        corners: Corners::all(2.0),
        shadow: Shadow::NONE,
    };
    // First touch: snaps current = start, returns settled. No motion
    // started yet.
    let _ = map.tick(
        id,
        SLOT,
        start.clone(),
        AnimSpec::SPRING,
        0.016,
        next_frame(),
    );

    // Retarget to a new fill (animated) and a new radius (snap). From
    // rest, the change's own frame steps nothing and shows the start,
    // snap field included.
    let target = Background {
        fill: RgbaF32::srgb(1.0, 0.0, 0.0).into(),
        border: Stroke::ZERO,
        corners: Corners::all(12.0),
        shadow: Shadow::NONE,
    };
    let r = map.tick(
        id,
        SLOT,
        target.clone(),
        AnimSpec::SPRING,
        0.016,
        next_frame(),
    );
    assert_eq!(r.current, start, "the change's frame shows the start");
    let r = map.tick(
        id,
        SLOT,
        target.clone(),
        AnimSpec::SPRING,
        0.016,
        next_frame(),
    );
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
    use crate::primitives::background::Background;
    use crate::primitives::brush::Brush;
    use crate::primitives::brush::gradient::linear_geometry::LinearGradient;
    use crate::primitives::corners::Corners;
    use crate::primitives::shadow::Shadow;
    use crate::primitives::stroke::Stroke;

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
    let _ = map.tick(id, SLOT, start, AnimSpec::SPRING, 0.0, next_frame());
    for _ in 0..3 {
        let _ = map.tick(
            id,
            SLOT,
            moving.clone(),
            AnimSpec::SPRING,
            0.016,
            next_frame(),
        );
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
    let result = map.tick(id, SLOT, target, AnimSpec::SPRING, 0.0, next_frame());
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
    use crate::primitives::background::Background;
    use crate::primitives::brush::Brush;
    use crate::primitives::brush::gradient::radial_geometry::RadialGradient;
    use crate::widgets::theme::text_style::TextStyle;
    use crate::widgets::theme::widget_look::animated_look::AnimatedLook;

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

    // Pass A's look, with the frame's report: the repaint request is
    // what says whether the spring is still moving.
    let frame = |h: &mut UiHarness, look: &AnimatedLook| {
        h.frame_passes(|ui| {
            let current = ui.animate(id, SLOT, look.clone(), Some(AnimSpec::SPRING));
            Block::new()
                .id(WidgetId::from_hash("gradient-look-settle"))
                .show(ui);
            current
        })
    };

    let first = frame(&mut h, &start);
    assert_eq!(*first.a(), start);
    assert!(!first.report().repaint_requested);

    let mut now = Duration::from_millis(16);
    let retarget = frame(h.at(now), &target);
    assert_eq!(retarget.a().background.fill, gradient);
    assert_ne!(retarget.a().text.color, target.text.color);
    assert!(retarget.report().repaint_requested);

    let mut settled_at = None;
    for frame_index in 0..600 {
        now += Duration::from_millis(16);
        let output = frame(h.at(now), &target);
        assert_eq!(output.a().background.fill, gradient);
        if !output.report().repaint_requested {
            assert_eq!(*output.a(), target);
            settled_at = Some(frame_index);
            break;
        }
    }
    assert!(settled_at.is_some(), "the look's color spring must settle");

    now += Duration::from_millis(16);
    let after_settle = frame(h.at(now), &target);
    assert_eq!(*after_settle.a(), target);
    assert!(
        !after_settle.report().repaint_requested,
        "a settled look must not request a surplus repaint",
    );
}
