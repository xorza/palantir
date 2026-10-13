//! Which response state engages a four-state look pack.

/// The response state that wears a [`StatefulLook`](crate::StatefulLook)'s
/// `active` look, read by [`StatefulLook::pick`](crate::StatefulLook::pick).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Engagement {
    /// Held by the pointer: a button, a toggle, a menu item, a tab chip.
    Pressed,
    /// Holding keyboard focus: a text field.
    Focused,
}
