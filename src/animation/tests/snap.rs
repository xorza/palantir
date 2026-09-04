//! Fields marked snap carry their target immediately, and only their own
//! velocity clears.

use crate::animation::anim_map_typed::AnimMapTyped;
use crate::animation::anim_spec::AnimSpec;
use crate::animation::tests::support::{
    AnimUi, SLOT, next_frame, setup_anim_ui, spring_velocity, wid,
};
use crate::primitives::color::RgbaF32;
use crate::primitives::widget_id::WidgetId;
use crate::scene::node::configure::Configure;
use crate::widgets::frame::Frame;
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
        stroke: Stroke::ZERO,
        corners: Corners::all(2.0),
        shadow: Shadow::NONE,
    };
    // First touch: snaps current = start, returns settled. No motion
    // started yet.
    let _ = map.tick(id, SLOT, start, AnimSpec::SPRING, 0.016, next_frame());

    // Retarget to a new fill (animated) and a new radius (snap).
    let target = Background {
        fill: RgbaF32::srgb(1.0, 0.0, 0.0).into(),
        stroke: Stroke::ZERO,
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
        stroke: Stroke::solid(RgbaF32::BLACK, 0.0),
        corners: Corners::ZERO,
        shadow: Shadow::NONE,
    };
    let moving = Background {
        fill: Brush::Solid(RgbaF32::WHITE),
        stroke: Stroke::solid(RgbaF32::BLACK, 10.0),
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
    let stroke_velocity = spring_velocity(&map.rows[&(id, SLOT)]).stroke.width;
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
        stroke: Stroke::solid(RgbaF32::BLACK, 20.0),
        corners: Corners::ZERO,
        shadow: Shadow::NONE,
    };
    let result = map.tick(id, SLOT, target, AnimSpec::SPRING, 0.0, next_frame());
    let row = &map.rows[&(id, SLOT)];
    let velocity = spring_velocity(row);
    assert_eq!(result.current.fill, gradient);
    assert_eq!(velocity.fill, Brush::TRANSPARENT);
    assert_eq!(velocity.stroke.width, stroke_velocity);
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
    let gradient = Brush::Radial(RadialGradient::two_stop_centered(
        RgbaF32::BLACK,
        RgbaF32::WHITE,
    ));
    let target = AnimatedLook {
        background: Background::fill(gradient.clone()),
        text: TextStyle::default().with_color(RgbaF32::WHITE),
    };

    let first = h.frame(|ui| {
        let current = ui.animate(id, SLOT, start.clone(), Some(AnimSpec::SPRING));
        assert_eq!(current, start);
        Frame::new()
            .id(WidgetId::from_hash("gradient-look-settle"))
            .show(ui);
    });
    assert!(!first.repaint_requested);

    let mut now = Duration::from_millis(16);
    let retarget = h.at(now).frame(|ui| {
        let current = ui.animate(id, SLOT, target.clone(), Some(AnimSpec::SPRING));
        assert_eq!(current.background.fill, gradient);
        assert_ne!(current.text.color, target.text.color);
        Frame::new()
            .id(WidgetId::from_hash("gradient-look-settle"))
            .show(ui);
    });
    assert!(retarget.repaint_requested);

    let mut settled_at = None;
    for frame in 0..600 {
        now += Duration::from_millis(16);
        let mut current = target.clone();
        let output = h.at(now).frame(|ui| {
            current = ui.animate(id, SLOT, target.clone(), Some(AnimSpec::SPRING));
            assert_eq!(current.background.fill, gradient);
            Frame::new()
                .id(WidgetId::from_hash("gradient-look-settle"))
                .show(ui);
        });
        if !output.repaint_requested {
            assert_eq!(current, target);
            settled_at = Some(frame);
            break;
        }
    }
    assert!(settled_at.is_some(), "the look's color spring must settle");

    now += Duration::from_millis(16);
    let after_settle = h.at(now).frame(|ui| {
        let current = ui.animate(id, SLOT, target.clone(), Some(AnimSpec::SPRING));
        assert_eq!(current, target);
        Frame::new()
            .id(WidgetId::from_hash("gradient-look-settle"))
            .show(ui);
    });
    assert!(
        !after_settle.repaint_requested,
        "a settled look must not request a surplus repaint",
    );
}
