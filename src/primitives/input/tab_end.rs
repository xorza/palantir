//! [`TabEnd`]: which end of a subtree's Tab stops focus enters at.

/// Which end of a subtree's Tab stops focus enters at, as a list opened by Home or End lands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TabEnd {
    /// The first stop in Tab order.
    #[default]
    First,
    /// The last stop in Tab order.
    Last,
}
