//! Which of the three toggles a [`ToggleTheme`](crate::ToggleTheme) recipe dresses.

/// The toggle a [`ToggleTheme`](crate::ToggleTheme) recipe is for, as each wears its own geometry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ToggleKind {
    /// [`crate::Checkbox`]: a 16 px box, control-rounded.
    Checkbox,
    /// [`crate::RadioGroup`]'s pips: 16 px with pill radius.
    Radio,
    /// [`crate::Switch`]: a 20 px-tall pill track whose knob wears `on_accent` when on and `text`
    /// when off, with an animated slide and cross-fade; `box_size` is the track height.
    Switch,
}
