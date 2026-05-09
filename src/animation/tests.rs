//! Pin tests for the f32 animation primitive. Covers first-touch
//! semantics, duration-based interpolation, retarget mid-flight,
//! spring convergence, settle clears repaint, and removed-widget
//! eviction.

use super::*;
use crate::layout::types::display::Display;
use crate::support::testing::ui_at;
use crate::tree::element::Configure;
use crate::tree::widget_id::WidgetId;
use crate::widgets::frame::Frame;
use glam::UVec2;
use std::time::Duration;

const SURFACE: UVec2 = UVec2::new(100, 100);
const SLOT: AnimSlot = AnimSlot(0);

fn wid(s: &'static str) -> WidgetId {
    WidgetId::from_hash(s)
}

#[test]
fn first_touch_returns_target_and_settled() {
    let mut map = AnimMap::default();
    let r = map.tick_f32(wid("a"), SLOT, 1.0, AnimSpec::FAST, 0.016);
    assert_eq!(r.current, 1.0, "first touch must snap to target");
    assert!(r.settled, "first touch must report settled (no anim)");
}

#[test]
fn duration_settles_in_finite_steps() {
    let mut map = AnimMap::default();
    let id = wid("a");
    let spec = AnimSpec::Duration {
        secs: 0.1,
        ease: Easing::Linear,
    };
    // Establish row at 0, then retarget to 1.
    let _ = map.tick_f32(id, SLOT, 0.0, spec, 0.016);
    let _ = map.tick_f32(id, SLOT, 1.0, spec, 0.0);
    let r = map.tick_f32(id, SLOT, 1.0, spec, 0.05);
    assert!(
        r.current > 0.4 && r.current < 0.6,
        "halfway should be ~0.5; got {}",
        r.current,
    );
    assert!(!r.settled, "halfway is not settled");
    let r = map.tick_f32(id, SLOT, 1.0, spec, 0.05);
    assert_eq!(r.current, 1.0, "must snap to target on settle");
    assert!(r.settled, "100ms total elapsed must settle");
}

#[test]
fn retarget_mid_flight_starts_new_segment_from_current() {
    let mut map = AnimMap::default();
    let id = wid("a");
    let spec = AnimSpec::Duration {
        secs: 0.1,
        ease: Easing::Linear,
    };
    let _ = map.tick_f32(id, SLOT, 0.0, spec, 0.016);
    let _ = map.tick_f32(id, SLOT, 1.0, spec, 0.0);
    let mid = map.tick_f32(id, SLOT, 1.0, spec, 0.05).current;
    assert!(mid > 0.4 && mid < 0.6, "halfway to 1.0; got {mid}");

    // Retarget to 2.0 mid-flight. New segment should start from `mid`.
    let r = map.tick_f32(id, SLOT, 2.0, spec, 0.0);
    assert_eq!(r.current, mid, "retarget must preserve current");
    let r = map.tick_f32(id, SLOT, 2.0, spec, 0.05);
    let expected = (mid + 2.0) * 0.5;
    assert!(
        (r.current - expected).abs() < 0.01,
        "new segment should ease from mid to 2.0; got {} expected {}",
        r.current,
        expected,
    );
}

#[test]
fn dt_zero_does_not_advance_duration() {
    let mut map = AnimMap::default();
    let id = wid("a");
    let spec = AnimSpec::Duration {
        secs: 0.1,
        ease: Easing::Linear,
    };
    let _ = map.tick_f32(id, SLOT, 0.0, spec, 0.0);
    let _ = map.tick_f32(id, SLOT, 1.0, spec, 0.0);
    let r = map.tick_f32(id, SLOT, 1.0, spec, 0.0);
    assert_eq!(r.current, 0.0, "dt=0 must not advance toward target");
    assert!(!r.settled, "still in flight");
}

#[test]
fn spring_with_initial_displacement_converges_within_settle_eps() {
    let mut map = AnimMap::default();
    let id = wid("a");
    let _ = map.tick_f32(id, SLOT, 0.0, AnimSpec::SPRING, 0.016);
    let mut last = f32::NAN;
    let mut settled_at = None;
    for i in 0..600 {
        let r = map.tick_f32(id, SLOT, 1.0, AnimSpec::SPRING, 0.016);
        last = r.current;
        if r.settled {
            settled_at = Some(i);
            break;
        }
    }
    assert!(
        settled_at.is_some(),
        "spring must settle within 600 frames (10s @ 60Hz); last = {last}",
    );
    assert!(
        (last - 1.0).abs() < 0.01,
        "settled value must equal target within eps; got {last}",
    );
}

/// End-to-end through `Ui::animate_f32` + `FrameOutput::repaint_requested`:
/// first-touch settled → no repaint; retarget in-flight → repaint;
/// repeated frames eventually settle and stop requesting repaint.
#[test]
fn animate_f32_drives_repaint_until_settle() {
    let mut ui = ui_at(SURFACE);
    let id = wid("anim-test");
    Frame::new().id_salt("anim-test").show(&mut ui);
    ui.end_frame();

    let display = Display::from_physical(SURFACE, 1.0);

    // Frame 1: first-touch at target=0 → settled.
    let frame = ui.run_frame(display, Duration::ZERO, |ui| {
        let _ = ui.animate_f32(id, SLOT, 0.0, AnimSpec::FAST);
        Frame::new().id_salt("anim-test").show(ui);
    });
    assert!(
        !frame.repaint_requested(),
        "first-touch settled animation must not request repaint",
    );

    // Frame 2: retarget to 1.0 → in-flight, must request repaint.
    let frame = ui.run_frame(display, Duration::from_millis(16), |ui| {
        let _ = ui.animate_f32(id, SLOT, 1.0, AnimSpec::FAST);
        Frame::new().id_salt("anim-test").show(ui);
    });
    assert!(
        frame.repaint_requested(),
        "in-flight animation must request repaint",
    );

    // Tick at 16ms until the animation settles. AnimSpec::FAST is
    // 120ms, so ~8 frames; allow generous headroom.
    let mut now = Duration::from_millis(16);
    let mut settled_at = None;
    for i in 0..100 {
        now += Duration::from_millis(16);
        let frame = ui.run_frame(display, now, |ui| {
            let _ = ui.animate_f32(id, SLOT, 1.0, AnimSpec::FAST);
            Frame::new().id_salt("anim-test").show(ui);
        });
        if !frame.repaint_requested() {
            settled_at = Some(i);
            break;
        }
    }
    assert!(
        settled_at.is_some(),
        "animation must settle and stop requesting repaints",
    );
}

#[test]
fn removed_widget_evicts_all_slots() {
    let mut map = AnimMap::default();
    let id = wid("a");
    let other = wid("b");
    let _ = map.tick_f32(id, AnimSlot(0), 1.0, AnimSpec::FAST, 0.016);
    let _ = map.tick_f32(id, AnimSlot(1), 2.0, AnimSpec::FAST, 0.016);
    let _ = map.tick_f32(other, AnimSlot(0), 3.0, AnimSpec::FAST, 0.016);
    assert_eq!(map.rows.len(), 3);

    map.sweep_removed(&[id]);
    assert_eq!(map.rows.len(), 1, "all slots for `id` must drop");
    assert!(
        map.rows.contains_key(&(other, AnimSlot(0))),
        "unrelated widget's row must survive",
    );
}
