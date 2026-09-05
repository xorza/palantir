//! The one-widget harness an animation test drives, and the reads it
//! asserts on.

use crate::animation::anim_row::{AnimRow, MotionRow};
use crate::animation::anim_slot::AnimSlot;
use crate::animation::anim_spec::AnimSpec;
use crate::animation::animatable::Animatable;
use crate::animation::easing::Easing;
use crate::primitives::widget_id::WidgetId;
use crate::ui::harness::UiHarness;
use crate::widgets::configure::Configure;
use crate::widgets::frame::Frame;
use glam::UVec2;

const SURFACE: UVec2 = UVec2::new(100, 100);

pub(super) const SLOT: AnimSlot = AnimSlot::new("test");

/// Process-global counter handed to `AnimMapTyped::tick` for tests
/// that don't care about pass A/B semantics — every call gets a
/// fresh id, so the multi-pass guard never short-circuits unless a
/// test deliberately reuses an id. Tests that *do* exercise the
/// multi-pass guard pass an explicit `render_frame_id` literal instead.
pub(super) fn next_frame() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    COUNTER.fetch_add(1, Ordering::Relaxed) + 1
}

pub(super) fn wid(s: &'static str) -> WidgetId {
    WidgetId::from_hash(s)
}

#[derive(Debug)]
pub(super) struct DurationMotionState<'a, T> {
    pub(super) segment_start: &'a T,
    pub(super) elapsed: f32,
}

pub(super) fn duration_motion<T: Animatable>(row: &AnimRow<T>) -> DurationMotionState<'_, T> {
    let MotionRow::Duration {
        segment_start,
        elapsed,
    } = &row.motion
    else {
        panic!("expected duration motion state");
    };
    DurationMotionState {
        segment_start,
        elapsed: *elapsed,
    }
}

pub(super) fn spring_velocity<T: Animatable>(row: &AnimRow<T>) -> &T {
    let MotionRow::Spring { velocity } = &row.motion else {
        panic!("expected spring motion state");
    };
    velocity
}

/// Common prelude for tests that drive an animated widget through
/// [`Ui::frame`]: spin up a `Ui`, pre-record the widget once so
/// its state row exists, return the `Ui` and the widget's id. Per-frame
/// bodies still need to re-record the
/// widget (`Frame::new().id(WidgetId::from_hash(salt)).show(ui)`) so the
/// persistent
/// state survives end-of-frame sweeps.
#[derive(Debug)]
pub(super) struct AnimUi {
    pub(super) h: UiHarness,
    pub(super) id: WidgetId,
}

pub(super) fn setup_anim_ui(salt: &'static str) -> AnimUi {
    let mut h = UiHarness::new(SURFACE);
    let id = wid(salt);
    h.frame(|ui| {
        Frame::new().id(WidgetId::from_hash(salt)).show(ui);
    });
    AnimUi { h, id }
}

pub(super) fn linear_100ms() -> AnimSpec {
    AnimSpec::duration(0.1, Easing::Linear)
}
