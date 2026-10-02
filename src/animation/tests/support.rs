//! The one-widget harness an animation test drives, and the reads it
//! asserts on.

use crate::animation::anim_row::{AnimRow, MotionRow};
use crate::animation::anim_slot::AnimSlot;
use crate::animation::anim_spec::AnimSpec;
use crate::animation::animatable::Animatable;
use crate::animation::easing::Easing;
use crate::primitives::widget_id::WidgetId;
use crate::ui::harness::UiHarness;
use crate::widgets::block::Block;
use crate::widgets::configure::Configure;
use glam::UVec2;

const SURFACE: UVec2 = UVec2::new(100, 100);

pub(super) const SLOT: AnimSlot = AnimSlot::new("test");

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

/// Common prelude for tests that drive an animated widget through
/// [`Ui::frame`]: spin up a `Ui`, pre-record the widget once so its state
/// row exists, return the `Ui` and the widget's id. Per-frame bodies
/// still re-record the widget (`Block::new().id(id).show(ui)`) so the
/// persistent state survives end-of-frame sweeps.
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

/// The first step at which a spring released from rest `distance` from
/// its target is inside both settle floors — from the closed form in
/// `f64`, independent of the integrator. Step `n` lasts `dt_of(n)`.
///
/// Released from rest, `x(t) = e^(-h·t)(C + h·S)` and
/// `v(t) = -k·e^(-h·t)·S` per unit distance, with `h = c/2` and `(C, S)`
/// the pair `SpringTransition` names: `cos`/`sin` over `ω` below critical
/// damping, `1`/`t` at it, `cosh`/`sinh` over `ψ` above it. The floors are
/// `POS_EPS = 1e-4` and `VEL_EPS = 0.1`.
pub(super) fn closed_form_settle_step(
    stiffness: f64,
    damping: f64,
    distance: f64,
    dt_of: impl Fn(u32) -> f32,
) -> u32 {
    let h = damping / 2.0;
    let psi_sq = h * h - stiffness;
    let mut t = 0.0;
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
            x.abs() < 1e-4 && v.abs() < 0.1
        })
        .unwrap()
}

pub(super) fn linear_100ms() -> AnimSpec {
    AnimSpec::duration(0.1, Easing::Linear)
}
