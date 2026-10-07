//! Owned theme values for a themed widget, so the theme borrow ends before its `&mut Ui` body.

use crate::ui::Ui;
use crate::widget_core::configure::Configure;
use crate::widget_core::widget::Widget;
use crate::widget_core::widget_look::WidgetLook;
use crate::widget_core::widget_look::animated_look::AnimatedLook;
use crate::widget_core::widget_look::theme_slot::SlotDefaults;

/// Everything a themed widget takes from its theme slot, owned so the [`Ui::theme`] borrow ends
/// before [`Self::apply`] animates (a `&mut Ui` use). Built by [`ThemeSlot::plan`], never by hand.
///
/// [`ThemeSlot::plan`]: crate::widget::ThemeSlot::plan
#[derive(Debug)]
pub struct LookPlan {
    pub(crate) target: AnimatedLook,
    pub(crate) defaults: SlotDefaults,
}

impl LookPlan {
    /// Fills the padding/margin `widget`'s builder left unset, then animates toward the planned look.
    /// The look is not stashed on the widget: a toggle paints it on its inner box (see
    /// `ToggleChrome::record_row`); widgets that wear it pass `Some(&look.background)` to [`Widget::record`].
    // Forced inline across the theme/widget codegen-unit boundary: left outlined, the resolver cost
    // 3.9% self-time in the frame bench.
    #[inline(always)]
    pub fn apply(self, ui: &mut Ui, widget: &mut Widget) -> AnimatedLook {
        let Self {
            target,
            defaults:
                SlotDefaults {
                    padding,
                    margin,
                    animation,
                },
        } = self;
        widget
            .configure()
            .default_padding(padding)
            .default_margin(margin);
        let id = widget.resolve(ui);
        ui.animate(id, WidgetLook::SLOT_LOOK, target, animation)
    }
}
