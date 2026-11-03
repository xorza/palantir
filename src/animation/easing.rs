//! Closed-form easing curves: input `t` is 0..1 progress, output the eased value (overshoots for `OutBack`).

use crate::primitives::math::domain;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
/// The easing curve a duration-based tween follows.
pub enum Easing {
    /// Constant rate.
    Linear,
    /// Fast start, decelerating to a stop; the default UI feel.
    OutCubic,
    /// Accelerate out of rest, decelerate into it.
    InOutCubic,
    /// Like [`Self::OutCubic`] with a sharper burst and longer settle.
    OutQuart,
    /// Overshoots the target and settles back; leaves 0..1.
    OutBack,
}

impl Easing {
    /// Ease progress `t`, a *fraction*: clamped to `0..=1`, `0` if not finite. The output is in `0..=1` except for [`Self::OutBack`].
    pub const fn apply(self, t: f32) -> f32 {
        let t = domain::fraction(t);
        match self {
            Self::Linear => t,
            Self::OutCubic => {
                let inv = 1.0 - t;
                1.0 - inv * inv * inv
            }
            Self::InOutCubic => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    let f = 2.0 * t - 2.0;
                    1.0 + f * f * f * 0.5
                }
            }
            Self::OutQuart => {
                let inv = 1.0 - t;
                1.0 - inv * inv * inv * inv
            }
            Self::OutBack => {
                const C1: f32 = 1.70158;
                const C3: f32 = C1 + 1.0;
                let inv = t - 1.0;
                1.0 + C3 * inv * inv * inv + C1 * inv * inv
            }
        }
    }

    /// This easing as a [`PaintCurve`](crate::widget::PaintCurve), for a paint-time animation
    /// that eases as a [`Ui::animate`](crate::Ui::animate) tween does. Each pass of the paint
    /// animation runs the curve once over its phase.
    pub const fn curve(self) -> fn(f32) -> f32 {
        match self {
            Self::Linear => |t| Self::Linear.apply(t),
            Self::OutCubic => |t| Self::OutCubic.apply(t),
            Self::InOutCubic => |t| Self::InOutCubic.apply(t),
            Self::OutQuart => |t| Self::OutQuart.apply(t),
            Self::OutBack => |t| Self::OutBack.apply(t),
        }
    }
}
