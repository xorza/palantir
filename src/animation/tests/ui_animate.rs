//! `Ui::animate` end to end: the repaint it requests and the rows it drops.

use crate::animation::anim_spec::AnimSpec;
use crate::animation::tests::support::{AnimUi, SLOT, setup_anim_ui};
use crate::primitives::color::RgbaF32;
use crate::primitives::widget_id::WidgetId;
use crate::widgets::configure::Configure;
use crate::widgets::frame::Frame;
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
            Frame::new().id(WidgetId::from_hash("anim-test")).show(ui);
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
            Frame::new().id(WidgetId::from_hash("anim-test")).show(ui);
        })
        .repaint_requested;
    assert!(repaint, "in-flight animation must request repaint");

    let mut now = Duration::from_millis(16);
    let mut settled_at = None;
    for i in 0..100 {
        now += Duration::from_millis(16);
        let repaint = h
            .at(now)
            .frame(|ui| {
                let _ = ui.animate(id, SLOT, 1.0_f32, Some(AnimSpec::FAST));
                Frame::new().id(WidgetId::from_hash("anim-test")).show(ui);
            })
            .repaint_requested;
        if !repaint {
            settled_at = Some(i);
            break;
        }
    }
    assert!(
        settled_at.is_some(),
        "animation must settle and stop requesting repaints",
    );
}

/// `Ui::animate(..., None)` must: return `target` unchanged, never
/// allocate a row, never request a repaint. `None` is the API-level
/// signal "this caller didn't ask for motion."
#[test]
fn animate_with_none_spec_snaps_and_skips_repaint() {
    let AnimUi { mut h, id } = setup_anim_ui("anim-none");
    let repaint = h
        .at(Duration::from_millis(16))
        .frame(|ui| {
            let v1 = ui.animate(id, SLOT, 7.0_f32, None);
            let v2 = ui.animate(id, SLOT, 9.0_f32, None);
            assert_eq!(v1, 7.0);
            assert_eq!(v2, 9.0);
            Frame::new().id(WidgetId::from_hash("anim-none")).show(ui);
        })
        .repaint_requested;
    assert!(!repaint, "None spec must never request a repaint");
    assert!(
        h.anim_row_count::<f32>() == 0,
        "None spec must not allocate a row",
    );
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
        Frame::new().id(WidgetId::from_hash("anim-toggle")).show(ui);
    });
    let _ = h.at(Duration::from_millis(50)).frame(|ui| {
        let _ = ui.animate(id, SLOT, 1.0_f32, Some(AnimSpec::FAST));
        Frame::new().id(WidgetId::from_hash("anim-toggle")).show(ui);
    });
    assert!(
        h.anim_row_count::<f32>() > 0,
        "Some(FAST) must allocate a row mid-flight",
    );

    // Frame B: switch to None — the stale row should drop.
    let _ = h.at(Duration::from_millis(60)).frame(|ui| {
        let _ = ui.animate(id, SLOT, 1.0_f32, None);
        Frame::new().id(WidgetId::from_hash("anim-toggle")).show(ui);
    });
    assert!(
        h.anim_row_count::<f32>() == 0,
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
        stroke: Stroke::solid(RgbaF32::hex(0xffffff), 2.0),
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
        Frame::new().id(WidgetId::from_hash("look-test")).show(ui);
    });
    let snap = captured.take().expect("animate ran");
    assert_eq!(snap.background.fill, bg.fill, "None: fill snaps to target");
    assert_eq!(
        snap.background.stroke.width, 2.0,
        "None: stroke width snaps"
    );
    assert_eq!(snap.background.stroke.color, bg.stroke.color);
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
        Frame::new().id(WidgetId::from_hash("look-test")).show(ui);
    });
    assert!(
        h.anim_row_count::<AnimatedLook>() > 0,
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
        Frame::new().id(WidgetId::from_hash("look-test")).show(ui);
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
