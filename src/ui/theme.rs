use crate::widgets::ButtonStyle;

/// Per-widget default styles. Widgets read from here at `show()` time when no
/// per-instance style override is supplied.
#[derive(Clone, Debug, Default)]
pub struct Theme {
    pub button: ButtonStyle,
}
