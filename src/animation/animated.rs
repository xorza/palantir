//! [`Animated`]: what one [`Ui::animate`](crate::Ui::animate) call returns.

use crate::primitives::math::animatable::Animatable;

/// The value an animation shows this frame, and whether it is at rest on its target.
///
/// `settled` tells a value that crossed its target from one that stays there: an underdamped
/// spring passes its target and comes back, so a value at the target is not yet at rest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Animated<T: Animatable> {
    /// The value to show this frame.
    pub value: T,
    /// The motion is at rest on its target, so the value stays until the target moves. A snap is
    /// settled at once.
    pub settled: bool,
}
