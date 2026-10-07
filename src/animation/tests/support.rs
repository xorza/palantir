//! The one-widget harness an animation test drives, and the reads it asserts on.

use crate::animation::anim_row::{AnimRow, MotionRow};
use crate::animation::animatable::Animatable;
use crate::animation::animation_slot::AnimationSlot;
use crate::animation::animation_spec::AnimationSpec;
use crate::animation::easing::Easing;
use crate::internals::harness::UiHarness;
use crate::primitives::identity::widget_id::WidgetId;
use crate::widget_core::configure::Configure;
use crate::widgets::block::Block;
use glam::UVec2;
use std::time::Duration;

const SURFACE: UVec2 = UVec2::new(100, 100);

pub(super) const SLOT: AnimationSlot = AnimationSlot::new("test");

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
        ..
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
    let MotionRow::Spring { velocity, .. } = &row.motion else {
        panic!("expected spring motion state");
    };
    velocity
}

/// Prelude: pre-records the widget so its state row exists; per-frame bodies re-record it so state survives sweeps.
#[derive(Debug)]
pub(super) struct AnimUi {
    pub(super) h: UiHarness,
    pub(super) id: WidgetId,
}

pub(super) fn setup_anim_ui(salt: &'static str) -> AnimUi {
    let mut h = UiHarness::new(SURFACE);
    let id = wid(salt);
    h.frame(|ui| {
        Block::new().id(id).show(ui);
    });
    AnimUi { h, id }
}

/// The first step at which a spring released from rest `distance` from its target is inside its settle bound, from the `f64` closed form `x(t) = e^(-h·t)(C + h·S)`, `v(t) = -k·e^(-h·t)·S` (`h = c/2`, `(C, S)` per `SpringTransition`); the bound is `x² + v²/k < eps²`. Step `n` lasts `dt_of(n)`.
pub(super) fn closed_form_settle_step(
    stiffness: f64,
    damping: f64,
    distance: f64,
    eps: f64,
    dt_of: impl Fn(u32) -> f32,
) -> u32 {
    let h = damping / 2.0;
    let psi_sq = h * h - stiffness;
    let mut t = 0.0;
    #[expect(
        clippy::maybe_infinite_iter,
        reason = "every caller passes a damped spring, and a damped spring settles"
    )]
    (1..)
        .find(|&n| {
            t += f64::from(dt_of(n));
            let (c, s) = if psi_sq < 0.0 {
                let omega = (-psi_sq).sqrt();
                ((omega * t).cos(), (omega * t).sin() / omega)
            } else if psi_sq == 0.0 {
                (1.0, t)
            } else {
                let psi = psi_sq.sqrt();
                ((psi * t).cosh(), (psi * t).sinh() / psi)
            };
            let decay = (-h * t).exp();
            let x = distance * decay * (c + h * s);
            let v = distance * stiffness * decay * s;
            x * x + v * v / stiffness < eps * eps
        })
        .unwrap()
}

pub(super) fn linear_100ms() -> AnimationSpec {
    AnimationSpec::duration(Duration::from_millis(100), Easing::Linear)
}
