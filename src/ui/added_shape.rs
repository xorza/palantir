//! [`AddedShape`]: a shape [`Ui::add_shape`] just recorded, which an animation can still reach.

use crate::scene::forest::AddedSlot;
use crate::shape::paint_animation::PaintAnimation;
use crate::ui::Ui;

/// A shape [`Ui::add_shape`] just recorded, which paints. [`Self::animate`] animates it at paint
/// time; dropped, it leaves the shape still.
#[derive(Debug)]
pub struct AddedShape<'u> {
    ui: &'u mut Ui,
    /// Where the shape went.
    slot: AddedSlot,
    /// Whether the paint reads an animation's time: an effect's does.
    reads_time: bool,
}

impl<'u> AddedShape<'u> {
    pub(crate) const fn new(ui: &'u mut Ui, slot: AddedSlot, reads_time: bool) -> Self {
        Self {
            ui,
            slot,
            reads_time,
        }
    }

    /// Animate the shape at paint time. The recorded shape is identical every frame, so the widget
    /// never records it again. With motion reduced ([`Ui::is_motion_reduced`]), a decorative
    /// animation lands at once, as
    /// [`MotionRole::Decorative`](crate::widget::MotionRole::Decorative) says. An [`EffectPaint`](crate::widget::EffectPaint) reads the animation's time. A shape
    /// that paints nothing takes no animation.
    ///
    /// # Panics
    ///
    /// When `animation` drives no channel and the paint reads no time, so nothing would move.
    #[track_caller]
    pub fn animate(self, animation: PaintAnimation) {
        let Self {
            ui,
            slot,
            reads_time,
        } = self;
        assert!(
            reads_time || animation.drives_channel(),
            "a paint animation that drives no channel moves only an effect's time, and this \
             shape's paint reads none"
        );
        let animation = if ui.is_motion_reduced() {
            animation.reduced()
        } else {
            Some(animation)
        };
        if let Some(animation) = animation {
            ui.forest.animate(slot, animation);
        }
    }
}
