use std::time::{Duration, Instant};

use glam::Vec2;

use crate::host::winit::input::PointerTrace;
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
