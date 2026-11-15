//! [`FocusPolicy`]: what an unfocusable press does to focus.

/// What happens to the focused widget when the pointer presses somewhere that
/// isn't a focusable widget. Set via [`crate::Ui::set_focus_policy`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FocusPolicy {
    /// A press on a non-focusable widget or empty surface keeps focus, so e.g.
    /// clicking a toolbar button set [`focusable(false)`](crate::Configure::focusable) while
    /// editing a field keeps the cursor there; a focusable button takes the focus under either
    /// policy.
    PreserveOnMiss,
    /// A press anywhere not focusable clears focus (click-outside-to-blur). Default.
    #[default]
    ClearOnMiss,
}
