//! Which of a toggle theme's two look packs applies.

/// Whether a toggle is on, which picks [`ToggleTheme::checked`](crate::ToggleTheme::checked)
/// or [`ToggleTheme::unchecked`](crate::ToggleTheme::unchecked).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckState {
    /// Off: the checkbox empty, the radio unselected, the switch off.
    Unchecked,
    /// On: the checkbox ticked, the radio selected, the switch on.
    Checked,
}

impl From<bool> for CheckState {
    /// `true` is [`Self::Checked`].
    fn from(checked: bool) -> Self {
        if checked {
            Self::Checked
        } else {
            Self::Unchecked
        }
    }
}
