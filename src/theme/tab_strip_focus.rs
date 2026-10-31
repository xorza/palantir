//! Which selection-cap color a tab strip paints.

/// Whether a strip's view holds focus, which picks [`TabsTheme::cap`](crate::TabsTheme::cap)'s
/// color.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabStripFocus {
    /// The view holds focus: [`TabsTheme::cap_focused`](crate::TabsTheme::cap_focused).
    Focused,
    /// Focus is elsewhere: [`TabsTheme::cap_unfocused`](crate::TabsTheme::cap_unfocused).
    Unfocused,
}

impl From<bool> for TabStripFocus {
    /// `true` is [`Self::Focused`].
    fn from(focused: bool) -> Self {
        if focused {
            Self::Focused
        } else {
            Self::Unfocused
        }
    }
}
