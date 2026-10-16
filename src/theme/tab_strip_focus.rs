//! Which selection-cap color a tab strip paints.

/// Whether a strip's view holds focus, which picks [`TabsTheme::cap`](crate::TabsTheme::cap)'s
/// color.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabStripFocus {
    /// The view holds focus: [`TabsTheme::accent`](crate::TabsTheme::accent).
    Focused,
    /// Focus is elsewhere: [`TabsTheme::accent_idle`](crate::TabsTheme::accent_idle).
    Unfocused,
}
