//! Where one pass stands in a main pass broken at its panes' barriers.

/// One pass of a main pass the panes that capture break at their barriers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MainSegment {
    /// The only pass: no pane captures.
    Whole,
    First,
    Inner,
    Last,
}

impl MainSegment {
    /// Whether the main pass starts here: it clears, and its timing opens.
    pub(crate) const fn opens(self) -> bool {
        matches!(self, Self::Whole | Self::First)
    }

    /// Whether the main pass ends here: its timing closes.
    pub(crate) const fn closes(self) -> bool {
        matches!(self, Self::Whole | Self::Last)
    }
}
