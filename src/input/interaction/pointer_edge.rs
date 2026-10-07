//! The edges a pointer produces: what became true this frame, as opposed to levels.

/// What happened, as an *edge*: something that became true this frame.
///
/// **A second walk, not a projection of [`ButtonPhase`].** A widget's phase is one value per frame, so press-release-press collapses to the live press there; here the release still reports its click. Both read one `ReleaseKind::click`.
///
/// [`ButtonPhase`]: crate::ButtonPhase
///
/// **Edges only.** A drag's travel is a level, read off that widget's `Response`; reporting a delta here would duplicate it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerEdge {
    /// The button went down on this widget; `count` is the place in the multi-press run.
    Pressed {
        /// Place in the multi-press run — 1 for a single press.
        count: u8,
    },
    /// Released on it with no drag latched; a double-click is `Clicked { count: 2 }`.
    Clicked {
        /// Place in the multi-press run — 2 for a double-click.
        count: u8,
    },
    /// Travel passed the drag threshold, latching a drag on this widget.
    DragStarted,
    /// A latched drag ended — the commit edge for drag gestures.
    DragStopped,
}
