//! Where focus lands in a menu the keyboard opens.

use crate::cascade::rows::tab_stop_row::TabEnd;
use crate::primitives::identity::widget_id::WidgetId;
use crate::ui::Ui;
use crate::widget_core::overlay_response::OverlayResponse;

/// The row a menu the keyboard opens focuses, as WAI-ARIA's menu button and select-only
/// combobox move visual focus on open
/// (<https://www.w3.org/WAI/ARIA/apg/patterns/combobox/examples/combobox-select-only/>).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MenuLanding {
    /// The first row, as Home or Up opens a list.
    #[default]
    First,
    /// The last row, as End opens a list.
    Last,
    /// The row of this id, as Down opens a list on its current pick.
    Row(WidgetId),
}

impl MenuLanding {
    /// Move focus onto this row of the menu `resp` answers for, on the frame it opens while focus
    /// shows the ring: the keyboard opened it. Focus returns to where it was when the menu closes.
    /// A closed menu, `None`, lands nothing.
    pub(crate) const fn land<R>(self, ui: &mut Ui, resp: Option<&OverlayResponse<R>>) {
        let Some(resp) = resp else {
            return;
        };
        if !resp.opened || !ui.is_focus_visible() {
            return;
        }
        match self {
            Self::First => ui.focus_end_within(resp.id, TabEnd::First),
            Self::Last => ui.focus_end_within(resp.id, TabEnd::Last),
            Self::Row(row) => ui.set_focus(row),
        }
    }
}
