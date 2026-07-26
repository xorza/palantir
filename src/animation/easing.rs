//! Closed-form easing curves for duration-based animation. Input `t`
//! is normalized 0..1 progress; output is the eased value (also 0..1
//! for "out" curves; may overshoot for `OutBack`).

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
/// The easing curve a duration-based tween follows.
pub enum Easing {
    /// No easing — constant rate.
    Linear,
    /// Fast start, decelerating to a stop. The default feel for UI
    /// transitions.
    OutCubic,
    /// Accelerate out of rest, decelerate into it. Symmetric; reads well
    /// for a value moving between two resting states.
    InOutCubic,
    /// Like [`Self::OutCubic`] but with a sharper initial burst and a
    /// longer settle.
    OutQuart,
    /// Overshoots past the target and settles back. **Leaves 0..1**, so
    /// only use it on values that tolerate exceeding their endpoints.
    OutBack,
}

impl Easing {
    /// Ease normalized progress `t`. Input is clamped to 0..1; output is
    /// also 0..1 except for [`Self::OutBack`], which overshoots.
    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Easing::Linear => t,
            Easing::OutCubic => {
                let inv = 1.0 - t;
                1.0 - inv * inv * inv
            }
            Easing::InOutCubic => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    let f = 2.0 * t - 2.0;
                    1.0 + f * f * f * 0.5
                }
            }
            Easing::OutQuart => {
                let inv = 1.0 - t;
                1.0 - inv * inv * inv * inv
            }
            Easing::OutBack => {
                const C1: f32 = 1.70158;
                const C3: f32 = C1 + 1.0;
                let inv = t - 1.0;
                1.0 + C3 * inv * inv * inv + C1 * inv * inv
            }
        }
    }
}
