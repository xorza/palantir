//! Closed-form easing curves for duration-based animation. Input `t`
//! is normalized 0..1 progress; output is the eased value (also 0..1
//! for "out" curves; may overshoot for `OutBack`).

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Easing {
    Linear,
    OutCubic,
    InOutCubic,
    OutQuart,
    OutBack,
}

impl Easing {
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
