//! Pin tests for the animation primitive (generic over `Animatable`).
//! Covers first-touch, duration interpolation, retarget mid-flight,
//! spring convergence, settle clears repaint, removed-widget eviction,
//! plus typed-slot dispatch via `Vec2` and `Color`.

use super::*;
use crate::layout::types::display::Display;
use crate::primitives::color::Color;
use crate::support::testing::ui_at;
use crate::tree::element::Configure;
use crate::tree::widget_id::WidgetId;
use crate::widgets::frame::Frame;
use glam::{UVec2, Vec2};
use std::time::Duration;

const SURFACE: UVec2 = UVec2::new(100, 100);
const SLOT: AnimSlot = AnimSlot(0);

fn wid(s: &'static str) -> WidgetId {
    WidgetId::from_hash(s)
}

fn linear_100ms() -> AnimSpec {
    AnimSpec::Duration {
        secs: 0.1,
        ease: Easing::Linear,
    }
}

/// A degenerate `Duration { secs ≈ 0 }` must be a true noop: snaps
/// to target, drops any existing row, and reports settled (so the
/// caller doesn't request a repaint). Switching from a real spec to
/// instant-degenerate-Duration must reset cleanly, not carry the
/// in-flight `current` forward. (The `None` path on `Ui::animate`
/// uses the same `is_instant` predicate — see
/// `animate_drives_repaint_until_settle` for the higher-level
/// guarantee.)
#[test]
fn instant_duration_is_noop_and_drops_row() {
    let instant = AnimSpec::Duration {
        secs: 0.0,
        ease: Easing::Linear,
    };
    assert!(instant.is_instant());
    assert!(!AnimSpec::FAST.is_instant());
    assert!(!AnimSpec::SPRING.is_instant());

    let mut map = AnimMapTyped::<f32>::default();
    let id = wid("a");

    // Instant on a fresh slot: snaps, settled, no row inserted.
    let r = map.tick(id, SLOT, 1.0, instant, 0.016);
    assert_eq!(r.current, 1.0);
    assert!(r.settled);
    assert!(
        map.rows.is_empty(),
        "instant must not allocate a row on a fresh slot",
    );

    // Mid-flight on FAST: row exists.
    let _ = map.tick(id, SLOT, 0.0, AnimSpec::FAST, 0.0);
    let _ = map.tick(id, SLOT, 1.0, AnimSpec::FAST, 0.05);
    assert_eq!(map.rows.len(), 1);

    // Switching to instant mid-flight: snap and drop the row.
    let r = map.tick(id, SLOT, 1.0, instant, 0.016);
    assert_eq!(r.current, 1.0);
    assert!(r.settled);
    assert!(
        map.rows.is_empty(),
        "instant must drop the stale row so a future non-instant \
         call starts fresh from `target`, not from in-flight current",
    );

    // Switching back to FAST: first-touch snaps to new target with no
    // residual `current` from before.
    let r = map.tick(id, SLOT, 5.0, AnimSpec::FAST, 0.016);
    assert_eq!(r.current, 5.0, "post-instant first-touch snaps");
    assert!(r.settled);
}

#[test]
fn first_touch_returns_target_and_settled() {
    let mut map = AnimMapTyped::<f32>::default();
    let r = map.tick(wid("a"), SLOT, 1.0, AnimSpec::FAST, 0.016);
    assert_eq!(r.current, 1.0, "first touch must snap to target");
    assert!(r.settled, "first touch must report settled (no anim)");
}

#[test]
fn duration_settles_in_finite_steps() {
    let mut map = AnimMapTyped::<f32>::default();
    let id = wid("a");
    let spec = linear_100ms();
    let _ = map.tick(id, SLOT, 0.0, spec, 0.016);
    let _ = map.tick(id, SLOT, 1.0, spec, 0.0);
    let r = map.tick(id, SLOT, 1.0, spec, 0.05);
    assert!(
        r.current > 0.4 && r.current < 0.6,
        "halfway should be ~0.5; got {}",
        r.current,
    );
    assert!(!r.settled, "halfway is not settled");
    let r = map.tick(id, SLOT, 1.0, spec, 0.05);
    assert_eq!(r.current, 1.0, "must snap to target on settle");
    assert!(r.settled, "100ms total elapsed must settle");
}

#[test]
fn retarget_mid_flight_starts_new_segment_from_current() {
    let mut map = AnimMapTyped::<f32>::default();
    let id = wid("a");
    let spec = linear_100ms();
    let _ = map.tick(id, SLOT, 0.0, spec, 0.016);
    let _ = map.tick(id, SLOT, 1.0, spec, 0.0);
    let mid = map.tick(id, SLOT, 1.0, spec, 0.05).current;
    assert!(mid > 0.4 && mid < 0.6, "halfway to 1.0; got {mid}");

    let r = map.tick(id, SLOT, 2.0, spec, 0.0);
    assert_eq!(r.current, mid, "retarget must preserve current");
    let r = map.tick(id, SLOT, 2.0, spec, 0.05);
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
    let mut map = AnimMapTyped::<f32>::default();
    let id = wid("a");
    let spec = linear_100ms();
    let _ = map.tick(id, SLOT, 0.0, spec, 0.0);
    let _ = map.tick(id, SLOT, 1.0, spec, 0.0);
    let r = map.tick(id, SLOT, 1.0, spec, 0.0);
    assert_eq!(r.current, 0.0, "dt=0 must not advance toward target");
    assert!(!r.settled, "still in flight");
}

#[test]
fn spring_with_initial_displacement_converges_within_settle_eps() {
    let mut map = AnimMapTyped::<f32>::default();
    let id = wid("a");
    let _ = map.tick(id, SLOT, 0.0, AnimSpec::SPRING, 0.016);
    let mut last = f32::NAN;
    let mut settled_at = None;
    for i in 0..600 {
        let r = map.tick(id, SLOT, 1.0, AnimSpec::SPRING, 0.016);
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

#[test]
fn vec2_duration_lerps_componentwise() {
    let mut map = AnimMapTyped::<Vec2>::default();
    let id = wid("a");
    let spec = linear_100ms();
    let _ = map.tick(id, SLOT, Vec2::ZERO, spec, 0.0);
    let _ = map.tick(id, SLOT, Vec2::new(10.0, 20.0), spec, 0.0);
    let r = map.tick(id, SLOT, Vec2::new(10.0, 20.0), spec, 0.05);
    assert!(
        (r.current.x - 5.0).abs() < 0.01 && (r.current.y - 10.0).abs() < 0.01,
        "halfway should be (5, 10); got {:?}",
        r.current,
    );
}

#[test]
fn color_spring_converges_to_target() {
    let mut map = AnimMapTyped::<Color>::default();
    let id = wid("a");
    let start = Color::rgb(0.0, 0.0, 0.0);
    let target = Color::rgb(1.0, 0.5, 0.25);
    let _ = map.tick(id, SLOT, start, AnimSpec::SPRING, 0.016);
    let mut last = start;
    let mut settled_at = None;
    for i in 0..600 {
        let r = map.tick(id, SLOT, target, AnimSpec::SPRING, 0.016);
        last = r.current;
        if r.settled {
            settled_at = Some(i);
            break;
        }
    }
    assert!(
        settled_at.is_some(),
        "color spring must settle; last = {last:?}",
    );
    assert!(
        (last.r - target.r).abs() < 0.01
            && (last.g - target.g).abs() < 0.01
            && (last.b - target.b).abs() < 0.01,
        "settled color must match target; got {last:?} expected {target:?}",
    );
}

/// End-to-end through `Ui::animate` + `FrameOutput::repaint_requested`:
/// first-touch settled → no repaint; retarget in-flight → repaint;
/// repeated frames eventually settle and stop requesting repaint.
#[test]
fn animate_drives_repaint_until_settle() {
    let mut ui = ui_at(SURFACE);
    let id = wid("anim-test");
    Frame::new().id_salt("anim-test").show(&mut ui);
    ui.end_frame();

    let display = Display::from_physical(SURFACE, 1.0);

    let frame = ui.run_frame(display, Duration::ZERO, |ui| {
        let _ = ui.animate(id, SLOT, 0.0_f32, AnimSpec::FAST);
        Frame::new().id_salt("anim-test").show(ui);
    });
    assert!(
        !frame.repaint_requested(),
        "first-touch settled animation must not request repaint",
    );

    let frame = ui.run_frame(display, Duration::from_millis(16), |ui| {
        let _ = ui.animate(id, SLOT, 1.0_f32, AnimSpec::FAST);
        Frame::new().id_salt("anim-test").show(ui);
    });
    assert!(
        frame.repaint_requested(),
        "in-flight animation must request repaint",
    );

    let mut now = Duration::from_millis(16);
    let mut settled_at = None;
    for i in 0..100 {
        now += Duration::from_millis(16);
        let frame = ui.run_frame(display, now, |ui| {
            let _ = ui.animate(id, SLOT, 1.0_f32, AnimSpec::FAST);
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
fn removed_widget_evicts_all_slots_across_typed_maps() {
    let mut map = AnimMap::default();
    let id = wid("a");
    let other = wid("b");
    let _ = map
        .scalars
        .tick(id, AnimSlot(0), 1.0, AnimSpec::FAST, 0.016);
    let _ = map
        .scalars
        .tick(id, AnimSlot(1), 2.0, AnimSpec::FAST, 0.016);
    let _ = map
        .vec2s
        .tick(id, AnimSlot(0), Vec2::ONE, AnimSpec::FAST, 0.016);
    let _ = map.colors.tick(
        id,
        AnimSlot(0),
        Color::rgb(1.0, 0.0, 0.0),
        AnimSpec::FAST,
        0.016,
    );
    let _ = map
        .scalars
        .tick(other, AnimSlot(0), 9.0, AnimSpec::FAST, 0.016);
    assert_eq!(map.scalars.rows.len(), 3);
    assert_eq!(map.vec2s.rows.len(), 1);
    assert_eq!(map.colors.rows.len(), 1);

    map.sweep_removed(&[id]);
    assert_eq!(
        map.scalars.rows.len(),
        1,
        "scalar slots for `id` must drop, `other` survives",
    );
    assert!(map.vec2s.rows.is_empty(), "vec2 slots for `id` must drop",);
    assert!(map.colors.rows.is_empty(), "color slots for `id` must drop",);
}
