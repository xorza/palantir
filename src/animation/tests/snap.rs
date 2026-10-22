//! Snap-marked fields carry their target immediately; only their own velocity clears.

use crate::animation::animation_map_typed::AnimationMapTyped;
use crate::animation::animation_request::AnimationRequest;
use crate::animation::animation_spec::AnimationSpec;
use crate::animation::spring::Spring;
use crate::animation::tests::support::{self, AnimationUi, Release, SLOT};
use crate::internals::ui_harness::UiHarness;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::paint::color::RgbaF32;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
use std::time::Duration;

/// `#[animate(snap)]` fields update on retarget mid-spring, not on settle (`Chrome.radius` is snap).
#[test]
fn spring_snap_fields_carry_target_immediately() {
    use crate::primitives::geometry::corners::Corners;
    use crate::primitives::paint::chrome::Chrome;
    use crate::primitives::paint::shadow::Shadow;
    use crate::primitives::paint::stroke::Stroke;

    let mut map = AnimationMapTyped::<Chrome>::default();
    let id = support::widget_id("snap-carry");
    let start = Chrome {
        fill: RgbaF32::srgb(0.0, 0.0, 0.0).into(),
        border: Stroke::NONE,
        corners: Corners::all(2.0),
        shadow: Shadow::NONE,
    };
    let _ = map.step(
        AnimationRequest::new(id, SLOT, start.clone()),
        AnimationSpec::SPRING,
        0.016,
    );

    // Retarget fill (animated) and radius (snap): from rest the change's frame steps nothing and shows the start.
    let target = Chrome {
        fill: RgbaF32::srgb(1.0, 0.0, 0.0).into(),
        border: Stroke::NONE,
        corners: Corners::all(12.0),
        shadow: Shadow::NONE,
    };
    let r = map.step(
        AnimationRequest::new(id, SLOT, target.clone()),
        AnimationSpec::SPRING,
        0.016,
    );
    assert_eq!(r.cur, start, "the change's frame shows the start");
    let r = map.step(
        AnimationRequest::new(id, SLOT, target.clone()),
        AnimationSpec::SPRING,
        0.016,
    );
    assert!(
        !r.settled,
        "spring with a real fill diff must remain in flight after one step",
    );
    assert_eq!(
        r.cur.corners, target.corners,
        "snap field must carry target value on the first stepped frame, not lag until settle",
    );
    assert!(
        r.cur.fill.as_solid().unwrap().r < target.fill.as_solid().unwrap().r - 0.05,
        "animated fill should still be mid-flight; got {:?}",
        r.cur.fill,
    );
}

#[test]
fn gradient_snap_clears_only_its_chrome_velocity() {
    use crate::primitives::geometry::corners::Corners;
    use crate::primitives::paint::brush::Brush;
    use crate::primitives::paint::brush::gradient::linear_geometry::LinearGradient;
    use crate::primitives::paint::chrome::Chrome;
    use crate::primitives::paint::shadow::Shadow;
    use crate::primitives::paint::stroke::Stroke;

    let mut map = AnimationMapTyped::<Chrome>::default();
    let id = support::widget_id("gradient-background-velocity");
    let start = Chrome {
        fill: Brush::Solid(RgbaF32::BLACK),
        border: Stroke::new(RgbaF32::BLACK, 0.0),
        corners: Corners::ZERO,
        shadow: Shadow::NONE,
    };
    let moving = Chrome {
        fill: Brush::Solid(RgbaF32::WHITE),
        border: Stroke::new(RgbaF32::BLACK, 10.0),
        corners: Corners::ZERO,
        shadow: Shadow::NONE,
    };
    let _ = map.step(
        AnimationRequest::new(id, SLOT, start),
        AnimationSpec::SPRING,
        0.0,
    );
    for _ in 0..3 {
        let _ = map.step(
            AnimationRequest::new(id, SLOT, moving.clone()),
            AnimationSpec::SPRING,
            0.016,
        );
    }
    let stroke_velocity = support::spring_velocity(&map.rows[&(id, SLOT)])
        .border
        .width;
    assert!(
        stroke_velocity > 0.0,
        "test setup must carry positive stroke velocity",
    );

    let gradient = Brush::Linear(LinearGradient::two_stop(
        0.0,
        RgbaF32::BLACK,
        RgbaF32::WHITE,
    ));
    let target = Chrome {
        fill: gradient.clone(),
        border: Stroke::new(RgbaF32::BLACK, 20.0),
        corners: Corners::ZERO,
        shadow: Shadow::NONE,
    };
    let result = map.step(
        AnimationRequest::new(id, SLOT, target),
        AnimationSpec::SPRING,
        0.0,
    );
    let row = &map.rows[&(id, SLOT)];
    let velocity = support::spring_velocity(row);
    assert_eq!(result.cur.fill, gradient);
    assert_eq!(velocity.fill, Brush::TRANSPARENT);
    assert_eq!(velocity.border.width, stroke_velocity);
    assert!(
        !result.settled,
        "the independently animated stroke still has real displacement",
    );
}

#[test]
fn gradient_snap_inside_look_repaints_only_until_numeric_fields_settle() {
    use crate::primitives::paint::brush::Brush;
    use crate::primitives::paint::brush::gradient::radial_geometry::RadialGradient;
    use crate::primitives::paint::chrome::Chrome;
    use crate::theme::look::animated_look::AnimatedLook;
    use crate::theme::text_style::TextStyle;

    let AnimationUi { mut h, id } = support::setup_animation_ui("gradient-look-settle");
    let start = AnimatedLook {
        chrome: Chrome::fill(RgbaF32::BLACK),
        text: TextStyle::default().with_color(RgbaF32::BLACK),
    };
    let gradient = Brush::Radial(RadialGradient::two_stop(RgbaF32::BLACK, RgbaF32::WHITE));
    let target = AnimatedLook {
        chrome: Chrome::fill(gradient.clone()),
        text: TextStyle::default().with_color(RgbaF32::WHITE),
    };

    let frame = |h: &mut UiHarness, look: &AnimatedLook| {
        h.frame_passes(|ui| {
            let cur = ui.animate(
                AnimationRequest::new(id, SLOT, look.clone()),
                Some(AnimationSpec::SPRING),
            );
            Block::new()
                .id(WidgetId::from_hash("gradient-look-settle"))
                .show(ui);
            cur
        })
    };

    let first = frame(&mut h, &start);
    assert_eq!(*first.a(), start);
    assert!(!first.report().repaint_requested);

    let tick = Duration::from_millis(16);
    let retarget = frame(h.advance(tick), &target);
    assert_eq!(retarget.a().chrome.fill, gradient);
    assert_ne!(retarget.a().text.color, target.text.color);
    assert!(retarget.report().repaint_requested);

    let mut last = None;
    let frames = h.frames_until_idle(600, tick, |ui| {
        let cur = ui.animate(
            AnimationRequest::new(id, SLOT, target.clone()),
            Some(AnimationSpec::SPRING),
        );
        assert_eq!(cur.chrome.fill, gradient);
        Block::new()
            .id(WidgetId::from_hash("gradient-look-settle"))
            .show(ui);
        last = Some(cur);
    });
    // Text color moves black → white, √3 in linear RGB; frame `n` after the retarget is step `n`.
    let release = Release {
        spring: Spring::DEFAULT,
        distance: 3.0f64.sqrt(),
        eps: 1.0 / 4096.0,
    };
    let step = release.settle_step(|_| 0.016);
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
