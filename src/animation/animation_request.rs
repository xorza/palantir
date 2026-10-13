//! What one [`Ui::animate`](crate::Ui::animate) call asks for: the row it advances and the value it heads for.

use crate::animation::animatable::Animatable;
use crate::animation::animation_slot::AnimationSlot;
use crate::primitives::identity::widget_id::WidgetId;

/// The animation row keyed by `(id, slot)`, and the `target` it heads for this frame.
#[derive(Clone, Debug)]
pub struct AnimationRequest<T: Animatable> {
    /// The widget the row belongs to.
    pub id: WidgetId,
    /// Which of the widget's animations this is.
    pub slot: AnimationSlot,
    /// The value the row heads for.
    pub target: T,
}

impl<T: Animatable> AnimationRequest<T> {
    /// Head `slot` of `id` for `target`.
    pub fn new(id: WidgetId, slot: impl Into<AnimationSlot>, target: T) -> Self {
        Self {
            id,
            slot: slot.into(),
            target,
        }
    }
}
