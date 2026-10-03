//! Which half of a splitter a body records.

/// Which pane [`Splitter::show`](crate::Splitter::show)'s body is currently recording.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitHalf {
    /// The leading pane — left of a vertical divider, above a
    /// horizontal one.
    First,
    /// The trailing pane.
    Second,
}
