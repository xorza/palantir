use std::time::{Duration, Instant};

use glam::Vec2;

use crate::host::winit::input::translator::PointerTrace;
use crate::host::winit::window::{FramePresent, PointerAnchor};

const AT: Vec2 = Vec2::new(300.0, 120.0);

fn anchor(scale: f32) -> Option<PointerAnchor> {
    PointerAnchor::after(None, PointerTrace::At(AT), scale)
}

#[test]
fn a_trace_records_a_position_and_a_departure_clears_one() {
    let held = anchor(2.0);
    assert_eq!(held.map(|a| a.physical), Some(AT));

    assert_eq!(
        PointerAnchor::after(held, PointerTrace::Unchanged, 4.0),
        held,
        "an event carrying no position leaves the anchor alone, scale included",
    );
    assert_eq!(PointerAnchor::after(held, PointerTrace::Gone, 2.0), None);
    assert_eq!(
        PointerAnchor::after(None, PointerTrace::Unchanged, 2.0),
        None,
    );
}

/// The pointer sits at physical (300, 120). At scale 2 the recorder
/// holds (150, 60); after a move to 2.5 it must hold (120, 48).
#[test]
fn a_scale_move_restates_the_position_once() {
    let mut held = anchor(2.0).unwrap();
    assert_eq!(held.restate_at(2.5), Some(Vec2::new(120.0, 48.0)));
    assert_eq!(
        held.restate_at(2.5),
        None,
        "the anchor adopted the scale, so the recorder is up to date",
    );
    assert_eq!(
        held.restate_at(2.0),
        Some(Vec2::new(150.0, 60.0)),
        "and a move back restates the position it started at",
    );
}

#[test]
fn an_unmoved_scale_restates_nothing() {
    assert_eq!(anchor(1.5).unwrap().restate_at(1.5), None);
}

#[test]
fn due_deadlines_resolve_to_immediate_and_future_ones_stand() {
    let now = Instant::now();
    let past = now - Duration::from_millis(1);
    let future = now + Duration::from_millis(16);

    assert_eq!(FramePresent::At(past).resolve(now), FramePresent::Immediate);
    // `<=` — a deadline landing exactly on `now` is due, not pending.
    assert_eq!(FramePresent::At(now).resolve(now), FramePresent::Immediate);
    assert_eq!(
        FramePresent::At(future).resolve(now),
        FramePresent::At(future)
    );
    assert_eq!(
        FramePresent::Immediate.resolve(now),
        FramePresent::Immediate
    );
    assert_eq!(FramePresent::Idle.resolve(now), FramePresent::Idle);
}

/// The IME caret goes to the platform in physical px: every component times the scale (10, 20, 2, 16 at 1.5 → 15, 30, 3, 24).
#[test]
fn the_ime_caret_scales_to_physical_px() {
    use crate::host::winit::window::physical_rect;
    use crate::primitives::geometry::rect::Rect;

    assert_eq!(
        physical_rect(Rect::new(10.0, 20.0, 2.0, 16.0), 1.5),
        Rect::new(15.0, 30.0, 3.0, 24.0),
    );
}

/// Hidden by occlusion at 0 s and shown at 3 s, the clock skips 3 s. A minimize during an
/// occlusion keeps the window hidden past the un-occlude, so the span ends only when both clear
/// (at 10 s, after 4 s). A frame run while hidden at 12 s restarts the span, so showing at 14 s
/// skips 2 s, not 4 s; a second occlusion report does not move the start.
#[test]
fn a_hidden_span_ends_when_every_reason_clears() {
    use crate::host::winit::window::visibility::Visibility;

    let t0 = Instant::now();
    let at = |s: u64| t0 + Duration::from_secs(s);
    let mut v = Visibility::default();
    assert!(!v.is_hidden());
    assert_eq!(v.set_occluded(true, at(0)), None);
    assert_eq!(
        v.set_occluded(true, at(1)),
        None,
        "a repeat keeps the start"
    );
    assert!(v.is_hidden());
    assert_eq!(v.set_occluded(false, at(3)), Some(Duration::from_secs(3)));
    assert!(!v.is_hidden());

    assert_eq!(v.set_occluded(true, at(6)), None);
    assert_eq!(v.set_minimized(true, at(7)), None);
    assert_eq!(v.set_occluded(false, at(8)), None, "still minimized");
    assert!(v.is_hidden() && v.is_minimized());
    assert_eq!(v.set_minimized(false, at(10)), Some(Duration::from_secs(4)));

    assert_eq!(v.set_occluded(true, at(11)), None);
    v.restart(at(12));
    assert_eq!(v.set_occluded(false, at(14)), Some(Duration::from_secs(2)));
    v.restart(at(15));
    assert!(!v.is_hidden(), "a restart while shown hides nothing");
}

/// Each level reaches the platform only on a change: the IME turns on with its first area,
/// moves with a new one, stays for a repeat, and turns off once.
#[test]
fn applied_levels_report_only_changes() {
    use crate::host::winit::window::applied_levels::{AppliedLevels, ImeChange};
    use crate::primitives::geometry::rect::Rect;
    use crate::window::cursor_icon::CursorIcon;

    let mut levels = AppliedLevels::default();
    for (cursor, changed) in [
        (CursorIcon::default(), false),
        (CursorIcon::Text, true),
        (CursorIcon::Text, false),
        (CursorIcon::Pointer, true),
    ] {
        assert_eq!(levels.set_cursor(cursor), changed, "{cursor:?}");
    }

    let a = Rect::new(10.0, 20.0, 2.0, 16.0);
    let b = Rect::new(30.0, 20.0, 2.0, 16.0);
    for (area, change) in [
        (None, None),
        (Some(a), Some(ImeChange::Enable(a))),
        (Some(a), None),
        (Some(b), Some(ImeChange::Move(b))),
        (None, Some(ImeChange::Disable)),
        (None, None),
        (Some(b), Some(ImeChange::Enable(b))),
    ] {
        assert_eq!(levels.set_ime_area(area), change, "{area:?}");
    }
}

/// How each acquire outcome schedules the next frame. A validation failure backs off from 16 ms,
/// doubling to the 500 ms ceiling (16, 32, 64, 128, 256, 500, 500), ahead of any repaint the
/// frame asked for; a lost surface backs off the same way and reconfigures nothing, as the host
/// makes a new one; any other outcome clears the back-off. A suboptimal swapchain rebuilds once
/// per size, an outdated one every time.
#[test]
fn the_schedule_follows_each_acquire_outcome() {
    use crate::host::winit::window::present_schedule::{
        AcquireOutcome as O, PresentDecision, PresentSchedule, Wake,
    };
    use crate::ui::frame_report::{FrameProcessing, FrameReport};
    use glam::UVec2;

    let report = |repaint_requested, repaint_after| FrameReport {
        repaint_requested,
        repaint_after,
        plan: None,
        processing: FrameProcessing::SingleLayout,
    };
    let due = Duration::from_secs(2);
    let ms = Duration::from_millis;
    let mut schedule = PresentSchedule::default();
    let mut step = |outcome, report: &FrameReport| schedule.after(outcome, report);
    let wake = |wake| PresentDecision {
        rebuild: false,
        wake,
    };
    let rebuild = |wake| PresentDecision {
        rebuild: true,
        wake,
    };

    for (label, outcome, report, want) in [
        (
            "a still frame",
            O::Skipped,
            report(false, None),
            wake(Wake::Idle),
        ),
        (
            "a still frame that asks",
            O::Skipped,
            report(true, None),
            wake(Wake::Now),
        ),
        (
            "a timed wake",
            O::Presented,
            report(false, Some(due)),
            wake(Wake::At(due)),
        ),
        (
            "a hidden window",
            O::Occluded,
            report(true, None),
            wake(Wake::Idle),
        ),
        (
            "a timeout",
            O::Timeout,
            report(false, None),
            wake(Wake::Now),
        ),
        (
            "an outdated swapchain",
            O::Outdated,
            report(false, None),
            rebuild(Wake::Now),
        ),
        (
            "and again",
            O::Outdated,
            report(false, None),
            rebuild(Wake::Now),
        ),
        (
            "stale at 64 × 48",
            O::Suboptimal(UVec2::new(64, 48)),
            report(false, None),
            rebuild(Wake::Idle),
        ),
        (
            "still stale there",
            O::Suboptimal(UVec2::new(64, 48)),
            report(false, None),
            wake(Wake::Idle),
        ),
        (
            "stale at 80 × 48",
            O::Suboptimal(UVec2::new(80, 48)),
            report(false, None),
            rebuild(Wake::Idle),
        ),
        (
            "rejected",
            O::Validation,
            report(true, Some(due)),
            wake(Wake::After(ms(16))),
        ),
        (
            "rejected twice",
            O::Validation,
            report(false, None),
            wake(Wake::After(ms(32))),
        ),
        (
            "three times",
            O::Validation,
            report(false, None),
            wake(Wake::After(ms(64))),
        ),
        (
            "four times",
            O::Validation,
            report(false, None),
            wake(Wake::After(ms(128))),
        ),
        (
            "five times",
            O::Validation,
            report(false, None),
            wake(Wake::After(ms(256))),
        ),
        (
            "six times",
            O::Validation,
            report(false, None),
            wake(Wake::After(ms(500))),
        ),
        (
            "at the ceiling",
            O::Validation,
            report(false, None),
            wake(Wake::After(ms(500))),
        ),
        (
            "presented again",
            O::Presented,
            report(false, None),
            wake(Wake::Idle),
        ),
        (
            "rejected anew",
            O::Validation,
            report(false, None),
            wake(Wake::After(ms(16))),
        ),
        (
            "a still frame clears it",
            O::Skipped,
            report(false, None),
            wake(Wake::Idle),
        ),
        (
            "a lost surface",
            O::Lost,
            report(false, None),
            wake(Wake::After(ms(16))),
        ),
        (
            "lost again",
            O::Lost,
            report(true, None),
            wake(Wake::After(ms(32))),
        ),
        (
            "made again and presented",
            O::Presented,
            report(false, None),
            wake(Wake::Idle),
        ),
    ] {
        assert_eq!(step(outcome, &report), want, "{label}");
    }
}
