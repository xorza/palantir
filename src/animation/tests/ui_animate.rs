//! `Ui::animate` end to end: the repaint it requests and the rows it drops.

use crate::animation::animation_spec::AnimationSpec;
use crate::animation::tests::support::{AnimUi, SLOT, setup_anim_ui};
use crate::internals::harness::UiHarness;
use crate::primitives::paint::color::RgbaF32;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
use std::cell;
use std::time::Duration;

/// First-touch settled: no repaint; in-flight retarget: repaint; repeated frames settle and stop requesting.
#[test]
fn animate_drives_repaint_until_settle() {
    let AnimUi { mut h, id } = setup_anim_ui("anim-test");

    let repaint = h
        .frame(|ui| {
            let _ = ui.animate(id, SLOT, 0.0_f32, Some(AnimationSpec::FAST));
            Block::new().id(id).show(ui);
        })
        .repaint_requested;
    assert!(
        !repaint,
        "first-touch settled animation must not request repaint",
    );

    let repaint = h
        .at(Duration::from_millis(16))
        .frame(|ui| {
            let _ = ui.animate(id, SLOT, 1.0_f32, Some(AnimationSpec::FAST));
            Block::new().id(id).show(ui);
        })
        .repaint_requested;
    assert!(repaint, "in-flight animation must request repaint");

    // FAST is 120 ms; frame `n` after the retarget has spent n × 16 ms: 112 at 7, 128 at 8.
    let frames = h.frames_until_idle(100, Duration::from_millis(16), |ui| {
        let _ = ui.animate(id, SLOT, 1.0_f32, Some(AnimationSpec::FAST));
        Block::new().id(id).show(ui);
    });
    assert_eq!(frames, Some(8), "the 8th 16 ms frame passes 120 ms");
}

/// `Ui::animate(..., None)` returns `target`, allocates no row and requests
/// no repaint. [`AnimationSpec::SNAP`] must behave the same.
#[test]
fn animate_with_none_spec_snaps_and_skips_repaint() {
    for (label, spec) in [("none", None), ("snap", Some(AnimationSpec::SNAP))] {
        let AnimUi { mut h, id } = setup_anim_ui("anim-none");
        let passes = h.at(Duration::from_millis(16)).frame_passes(|ui| {
            let v1 = ui.animate(id, SLOT, 7.0_f32, spec);
            let v2 = ui.animate(id, SLOT, 9.0_f32, spec);
            Block::new().id(id).show(ui);
            [v1, v2]
        });
        assert_eq!(*passes.a(), [7.0, 9.0], "{label}");
        assert!(
            !passes.report().repaint_requested,
            "{label} spec must never request a repaint"
        );
        assert_eq!(
            h.anim_row_count::<f32>(),
            0,
            "{label} spec must not allocate a row",
        );
    }
}

/// Switching from `Some(spec)` to `None` mid-flight drops the stale row, so a later retarget starts fresh.
#[test]
fn animate_some_then_none_drops_stale_row() {
    let AnimUi { mut h, id } = setup_anim_ui("anim-toggle");
    let _ = h.at(Duration::from_millis(0)).frame(|ui| {
        let _ = ui.animate(id, SLOT, 0.0_f32, Some(AnimationSpec::FAST));
        Block::new().id(id).show(ui);
    });
    let _ = h.at(Duration::from_millis(50)).frame(|ui| {
        let _ = ui.animate(id, SLOT, 1.0_f32, Some(AnimationSpec::FAST));
        Block::new().id(id).show(ui);
    });
    assert_eq!(
        h.anim_row_count::<f32>(),
        1,
        "Some(FAST) must allocate a row mid-flight",
    );

    let _ = h.at(Duration::from_millis(60)).frame(|ui| {
        let _ = ui.animate(id, SLOT, 1.0_f32, None);
        Block::new().id(id).show(ui);
    });
    assert_eq!(
        h.anim_row_count::<f32>(),
        0,
        "None spec must drop the stale row inserted by a prior Some()",
    );
}

/// `WidgetLook::animate` folds text overrides onto the ambient style into an `AnimatedLook`; covers `spec = None` (no rows) and a real spec.
#[test]
fn widget_look_animate_resolves_components_and_falls_back() {
    use crate::primitives::geometry::corners::Corners;
    use crate::primitives::paint::background::Background;
    use crate::primitives::paint::shadow::Shadow;
    use crate::primitives::paint::stroke::Stroke;
    use crate::widget_core::widget_look::WidgetLook;
    use crate::widget_core::widget_look::animated_look::AnimatedLook;
    use crate::widgets::theme::text_style::{TextStyle, TextStyleOverrides};
    use std::cell::Cell;

    let AnimUi { mut h, id } = setup_anim_ui("look-test");

    let bg = Background {
        fill: RgbaF32::hex(0x336699).into(),
        border: Stroke::new(RgbaF32::hex(0xffffff), 2.0),
        corners: Corners::all(4.0),
        shadow: Shadow::NONE,
    };
    let look = WidgetLook {
        background: bg.clone(),
        text: TextStyleOverrides::NONE,
    };
    let fallback = TextStyle::default();

    let captured: Cell<Option<AnimatedLook>> = Cell::new(None);
    let _ = h.at(Duration::from_millis(16)).frame(|ui| {
        let target = look.to_animated(fallback);
        captured.set(Some(ui.animate(id, WidgetLook::SLOT_LOOK, target, None)));
        Block::new().id(id).show(ui);
    });
    let snap = captured.take().expect("animate ran");
    assert_eq!(snap.background.fill, bg.fill, "None: fill snaps to target");
    assert_eq!(
        snap.background.border.width, 2.0,
        "None: stroke width snaps"
    );
    assert_eq!(snap.background.border.color, bg.border.color);
    assert_eq!(snap.background.corners, bg.corners);
    assert_eq!(
        snap.text, fallback,
        "a look that overrides nothing takes the ambient style whole",
    );
    assert_eq!(
        h.anim_row_count::<AnimatedLook>(),
        0,
        "None spec: WidgetLook::animate must allocate no AnimatedLook row",
    );

    // Retargeting the fill allocates a Background row; text is unchanged, so the snap-if-close path leaves no TextStyle row.
    let look2 = WidgetLook {
        background: Background {
            fill: RgbaF32::hex(0xff0000).into(),
            ..bg.clone()
        },
        text: TextStyleOverrides::NONE,
    };
    let _ = h.at(Duration::from_millis(32)).frame(|ui| {
        let target = look2.to_animated(fallback);
        let _ = ui.animate(id, WidgetLook::SLOT_LOOK, target, Some(AnimationSpec::FAST));
        Block::new().id(id).show(ui);
    });
    assert_eq!(
        h.anim_row_count::<AnimatedLook>(),
        1,
        "Some(FAST) on changed fill must allocate an AnimatedLook row",
    );

    // Axes the look names win; the rest come from the ambient style. Every axis differs, so a wrong source shows.
    let own_size = fallback.font_size + 7.0;
    let own_color = RgbaF32::hex(0x00ff00);
    let ambient = TextStyle {
        font_size: fallback.font_size + 99.0,
        color: RgbaF32::hex(0xff00ff),
        line_height_factor: fallback.line_height_factor + 9.0,
        ..fallback
    };
    let look3 = WidgetLook {
        background: bg.clone(),
        text: TextStyleOverrides::NONE
            .with_font_size(own_size)
            .with_color(own_color),
    };
    let captured: Cell<Option<AnimatedLook>> = Cell::new(None);
    let _ = h.at(Duration::from_millis(48)).frame(|ui| {
        let target = look3.to_animated(ambient);
        captured.set(Some(ui.animate(
            id.with("own"),
            WidgetLook::SLOT_LOOK,
            target,
            None,
        )));
        Block::new().id(id).show(ui);
    });
    let snap = captured.take().expect("animate ran");
    assert_eq!(
        snap.text,
        TextStyle {
            font_size: own_size,
            color: own_color,
            ..ambient
        },
        "named axes come from the look, the leading from the ambient style",
    );
}

/// `FAST` from rest after a second idle: the change frame shows the start value (not the clamped 0.1 s); the next frame, 16 ms later, shows `OutCubic(16 / 120)`.
#[test]
fn a_motion_from_rest_starts_on_the_frame_of_the_change() {
    use crate::animation::easing::Easing;
    let AnimUi { mut h, id } = setup_anim_ui("from-rest");
    let record = |h: &mut UiHarness, at: Duration, target: f32| {
        let value = cell::Cell::new(f32::NAN);
        h.at(at).frame(|ui| {
            value.set(ui.animate(id, SLOT, target, Some(AnimationSpec::FAST)));
            Block::new().id(id).show(ui);
        });
        value.get()
    };
    assert_eq!(record(&mut h, Duration::ZERO, 0.0), 0.0);
    let idle = Duration::from_secs(1);
    assert_eq!(
        record(&mut h, idle, 1.0),
        0.0,
        "the change's frame shows the start"
    );
    let next = record(&mut h, idle + Duration::from_millis(16), 1.0);
    assert_eq!(next, Easing::OutCubic.apply(0.016 / 0.12));
}
