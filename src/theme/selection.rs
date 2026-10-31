//! Which of a theme's two look packs, unselected or selected, applies.

/// Whether a widget shows its selected state: a strip's current tab chip
/// ([`TabsTheme::selected`](crate::TabsTheme::selected)), or a button that shows an on state
/// ([`ButtonTheme::selected`](crate::ButtonTheme::selected)).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Selection {
    /// The resting pack.
    #[default]
    Unselected,
    /// The pack that marks the widget as the selected or on one.
    Selected,
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
