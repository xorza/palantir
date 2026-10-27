//! When a tab chip shows its close button.

/// Which chips show their close button, as
/// [`TabsTheme::close_shown`](crate::TabsTheme::close_shown) picks. Every closable chip keeps the
/// button's space either way, so a chip never changes width as its button comes and goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CloseShown {
    /// On every closable chip.
    #[default]
    Always,
    /// On the selected chip and the chip under the pointer, as VS Code's and Zed's strips show
    /// it: one cross on screen per strip at rest, not one per tab.
    SelectedOrHovered,
}
