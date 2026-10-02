//! `Ui::animate` end to end: the repaint it requests and the rows it drops.

use crate::animation::anim_spec::AnimSpec;
use crate::animation::tests::support::{AnimUi, SLOT, setup_anim_ui};
use crate::primitives::color::RgbaF32;
use crate::primitives::widget_id::WidgetId;
use crate::widgets::block::Block;
use crate::widgets::configure::Configure;
use std::time::Duration;

/// End-to-end through `Ui::animate` + `FrameOutput::repaint_requested`:
/// first-touch settled → no repaint; retarget in-flight → repaint;
/// repeated frames eventually settle and stop requesting repaint.
#[test]
fn animate_drives_repaint_until_settle() {
    let AnimUi { mut h, id } = setup_anim_ui("anim-test");

    let repaint = h
        .frame(|ui| {
            let _ = ui.animate(id, SLOT, 0.0_f32, Some(AnimSpec::FAST));
            Block::new().id(WidgetId::from_hash("anim-test")).show(ui);
        })
        .repaint_requested;
    assert!(
        !repaint,
        "first-touch settled animation must not request repaint",
    );

    let repaint = h
        .at(Duration::from_millis(16))
        .frame(|ui| {
            let _ = ui.animate(id, SLOT, 1.0_f32, Some(AnimSpec::FAST));
            Block::new().id(WidgetId::from_hash("anim-test")).show(ui);
        })
        .repaint_requested;
    assert!(repaint, "in-flight animation must request repaint");

    // FAST is 120 ms. The retarget frame spent nothing, so frame `n`
    // after it has spent n × 16 ms: 112 ms at 7, 128 ms at 8.
    let frames = h.frames_until_idle(100, Duration::from_millis(16), |ui| {
        let _ = ui.animate(id, SLOT, 1.0_f32, Some(AnimSpec::FAST));
        Block::new().id(WidgetId::from_hash("anim-test")).show(ui);
    });
    assert_eq!(frames, Some(8), "the 8th 16 ms frame passes 120 ms");
}

/// `Ui::animate(..., None)` must: return `target` unchanged, never
/// allocate a row, never request a repaint. `None` is the API-level
/// signal "this caller didn't ask for motion."
///
/// [`AnimSpec::SNAP`] is the named spelling of the same answer, so it is
/// swept here rather than pinned apart — a caller reaching for the name
/// must not get different behaviour from the one passing `None`.
#[test]
fn animate_with_none_spec_snaps_and_skips_repaint() {
    for (label, spec) in [("none", None), ("snap", Some(AnimSpec::SNAP))] {
        let AnimUi { mut h, id } = setup_anim_ui("anim-none");
        let passes = h.at(Duration::from_millis(16)).frame_passes(|ui| {
            let v1 = ui.animate(id, SLOT, 7.0_f32, spec);
            let v2 = ui.animate(id, SLOT, 9.0_f32, spec);
            Block::new().id(WidgetId::from_hash("anim-none")).show(ui);
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

/// Switching from `Some(spec)` to `None` mid-flight must drop the
/// stale row so a future `Some(spec)` retarget starts fresh from the
/// new target rather than carrying in-flight `current` forward.
#[test]
fn animate_some_then_none_drops_stale_row() {
    let AnimUi { mut h, id } = setup_anim_ui("anim-toggle");
    // Frame A: animate to 1.0 with FAST (in flight).
    let _ = h.at(Duration::from_millis(0)).frame(|ui| {
        let _ = ui.animate(id, SLOT, 0.0_f32, Some(AnimSpec::FAST));
        Block::new().id(WidgetId::from_hash("anim-toggle")).show(ui);
    });
    let _ = h.at(Duration::from_millis(50)).frame(|ui| {
        let _ = ui.animate(id, SLOT, 1.0_f32, Some(AnimSpec::FAST));
        Block::new().id(WidgetId::from_hash("anim-toggle")).show(ui);
    });
    assert_eq!(
        h.anim_row_count::<f32>(),
        1,
        "Some(FAST) must allocate a row mid-flight",
    );

    // Frame B: switch to None — the stale row should drop.
    let _ = h.at(Duration::from_millis(60)).frame(|ui| {
        let _ = ui.animate(id, SLOT, 1.0_f32, None);
        Block::new().id(WidgetId::from_hash("anim-toggle")).show(ui);
    });
    assert_eq!(
        h.anim_row_count::<f32>(),
        0,
        "None spec must drop the stale row inserted by a prior Some()",
    );
}

/// `WidgetLook::animate` resolves the look's optional components to
/// flat values and returns an `AnimatedLook` with the right defaults.
/// Walks both branches: with `spec = None` (snap, no rows) and with a
/// real spec (rows allocated for non-trivial components).
#[test]
fn widget_look_animate_resolves_components_and_falls_back() {
    use crate::primitives::background::Background;
    use crate::primitives::corners::Corners;
    use crate::primitives::shadow::Shadow;
    use crate::primitives::stroke::Stroke;
    use crate::widgets::theme::text_style::TextStyle;
    use crate::widgets::theme::widget_look::WidgetLook;
    use crate::widgets::theme::widget_look::animated_look::AnimatedLook;
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
        text: None, // → falls back to TextStyle default
    };
    let fallback = TextStyle::default();

    // None spec: snaps to target, no rows allocated. Use Cell to
    // capture out of the FnMut closure.
    let captured: Cell<Option<AnimatedLook>> = Cell::new(None);
    let _ = h.at(Duration::from_millis(16)).frame(|ui| {
        let target = look.to_animated(fallback);
        captured.set(Some(ui.animate(id, WidgetLook::SLOT_LOOK, target, None)));
        Block::new().id(WidgetId::from_hash("look-test")).show(ui);
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
        snap.text.color, fallback.color,
        "None: text falls back to fallback_text",
    );
    assert_eq!(snap.text.font_size_px, fallback.font_size_px);
    assert_eq!(snap.text.line_height_mult, fallback.line_height_mult);
    assert_eq!(
        h.anim_row_count::<AnimatedLook>(),
        0,
        "None spec: WidgetLook::animate must allocate no AnimatedLook row",
    );

    // Some(FAST) spec, retargeting to a different fill: a row gets
    // allocated for the in-flight Background animation. Text didn't
    // change, so the snap-if-close fast path leaves TextStyle row
    // unallocated.
    let look2 = WidgetLook {
        background: Background {
            fill: RgbaF32::hex(0xff0000).into(),
            ..bg.clone()
        },
        text: None,
    };
    let _ = h.at(Duration::from_millis(32)).frame(|ui| {
        let target = look2.to_animated(fallback);
        let _ = ui.animate(id, WidgetLook::SLOT_LOOK, target, Some(AnimSpec::FAST));
        Block::new().id(WidgetId::from_hash("look-test")).show(ui);
    });
    assert_eq!(
        h.anim_row_count::<AnimatedLook>(),
        1,
        "Some(FAST) on changed fill must allocate an AnimatedLook row",
    );

    // The other half of the `fallback_text` contract: a look that
    // overrides `text` must not read the fallback at all. The fallback is
    // made wrong in every field so any read shows up.
    let own_text = TextStyle {
        font_size_px: fallback.font_size_px + 7.0,
        color: RgbaF32::hex(0x00ff00),
        line_height_mult: fallback.line_height_mult + 0.5,
        ..fallback
    };
    let unread = TextStyle {
        font_size_px: fallback.font_size_px + 99.0,
        color: RgbaF32::hex(0xff00ff),
        line_height_mult: fallback.line_height_mult + 9.0,
        ..fallback
    };
    let look3 = WidgetLook {
        background: bg.clone(),
        text: Some(own_text),
    };
    let captured: Cell<Option<AnimatedLook>> = Cell::new(None);
    let _ = h.at(Duration::from_millis(48)).frame(|ui| {
        let target = look3.to_animated(unread);
        captured.set(Some(ui.animate(
            id.with("own"),
            WidgetLook::SLOT_LOOK,
            target,
            None,
        )));
        Block::new().id(WidgetId::from_hash("look-test")).show(ui);
    });
    let snap = captured.take().expect("animate ran");
    assert_eq!(
        snap.text.font_size_px,
        fallback.font_size_px + 7.0,
        "an overriding look keeps its own size, not the fallback's",
    );
    assert_eq!(snap.text.color, own_text.color);
    assert_eq!(snap.text.line_height_mult, own_text.line_height_mult);
}

/// `AnimSpec::FAST` from rest after a second of idle: the frame of the
/// change shows the start value, though the clamp would have handed it
/// 0.1 s — 83 % of a 120 ms curve, 99.5 % eased. The next frame, 16 ms
/// later, shows `OutCubic(16 / 120)`.
#[test]
fn a_motion_from_rest_starts_on_the_frame_of_the_change() {
    use crate::animation::easing::Easing;
    let AnimUi { mut h, id } = setup_anim_ui("from-rest");
    let record = |h: &mut crate::ui::harness::UiHarness, at: Duration, target: f32| {
        let value = std::cell::Cell::new(f32::NAN);
        h.at(at).frame(|ui| {
            value.set(ui.animate(id, SLOT, target, Some(AnimSpec::FAST)));
            Block::new().id(WidgetId::from_hash("from-rest")).show(ui);
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
