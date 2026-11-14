//! Which of a theme's two look packs, unselected or selected, applies.

use crate::theme::look::stateful_look::StatefulLook;

/// Whether a widget shows its selected state: a strip's current tab chip
/// ([`TabsTheme::selected`](crate::TabsTheme::selected)), or a button that shows an on state
/// ([`ButtonTheme::selected`](crate::ButtonTheme::selected)).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Selection {
    /// The resting pack.
    #[default]
    Unselected,
    /// The pack that marks the widget as the selected or on one.
    Selected,
}

impl Selection {
    /// The pack of `unselected` and `selected` this selection picks, for a theme slot that holds
    /// the two.
    pub(crate) const fn pack<'a>(
        self,
        unselected: &'a StatefulLook,
        selected: &'a StatefulLook,
    ) -> &'a StatefulLook {
        match self {
            Self::Unselected => unselected,
            Self::Selected => selected,
        }
    }
}

impl From<bool> for Selection {
    /// `true` is [`Self::Selected`].
    fn from(selected: bool) -> Self {
        if selected {
            Self::Selected
        } else {
            Self::Unselected
        }
    }
}
