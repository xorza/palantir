//! Owned theme values for a themed widget, so the theme borrow ends before its `&mut Ui` body.

use crate::theme::look::animated_look::AnimatedLook;
use crate::theme::look::theme_slot::SlotDefaults;

/// Everything a themed widget takes from its theme slot, owned so the
/// [`Ui::theme`](crate::Ui::theme) borrow ends before
/// [`Widget::wear`](crate::widget::Widget::wear) animates (a `&mut Ui` use). Built by
/// [`ThemeSlot::plan`], never by hand.
///
/// [`ThemeSlot::plan`]: crate::widget::ThemeSlot::plan
#[derive(Debug)]
pub struct LookPlan {
    pub(crate) target: AnimatedLook,
    pub(crate) defaults: SlotDefaults,
}
