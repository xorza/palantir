//! The keys that activate a focused widget.

/// Which keys activate a focused widget as one left click, by its WAI-ARIA role. Read through
/// [`Widget::activation_response`](crate::widget::Widget::activation_response).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activation {
    /// Space and Enter, as a button, a menu item or a disclosure.
    Button,
    /// Space alone, as a checkbox, a radio button or a switch: Enter stays with the form.
    Toggle,
}
