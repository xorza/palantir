//! What one [`Ui::animate`](crate::Ui::animate) call asks for: the row it advances and the value it heads for.

use crate::animation::animation_key::AnimationKey;
use crate::animation::animation_slot::AnimationSlot;
use crate::animation::motion_role::MotionRole;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::math::animatable::Animatable;

/// The animation row keyed by `(id, slot)`, and the `target` it heads for this frame.
#[derive(Clone, Debug)]
pub struct AnimationRequest<T: Animatable> {
    /// The widget the row belongs to.
    pub id: WidgetId,
    /// Which of the widget's animations this is.
    pub slot: AnimationSlot,
    /// The value the row heads for.
    pub target: T,
    /// The target's cause, so a target that moves under it is followed, not animated toward;
    /// `None` animates toward every new target.
    pub key: Option<AnimationKey>,
    /// What the motion tells the person, which decides whether it snaps while motion or power is
    /// reduced; [`MotionRole::Decorative`] by default.
    pub motion: MotionRole,
}

impl<T: Animatable> AnimationRequest<T> {
    /// Head `slot` of `id` for `target`, with no key.
    pub fn new(id: WidgetId, slot: impl Into<AnimationSlot>, target: T) -> Self {
        Self {
            id,
            slot: slot.into(),
            target,
            key: None,
            motion: MotionRole::Decorative,
        }
    }

    /// The target's cause; see [`AnimationKey`].
    #[must_use]
    pub const fn with_key(mut self, key: AnimationKey) -> Self {
        self.key = Some(key);
        self
    }

    /// Say what the motion tells the person; [`MotionRole::Decorative`] by default.
    #[must_use]
    pub const fn with_motion(mut self, motion: MotionRole) -> Self {
        self.motion = motion;
        self
    }
}
