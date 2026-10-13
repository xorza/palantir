//! Which of a tabs theme's two chip look packs applies.

/// Whether a chip is the strip's selected one, which picks
/// [`TabsTheme::active`](crate::TabsTheme::active) or
/// [`TabsTheme::inactive`](crate::TabsTheme::inactive).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabSelection {
    /// Any chip but the selected one.
    Unselected,
    /// The chip wearing the selection cap.
    Selected,
}

impl From<bool> for TabSelection {
    /// `true` is [`Self::Selected`].
    fn from(selected: bool) -> Self {
        if selected {
            Self::Selected
        } else {
            Self::Unselected
        }
    }
}
