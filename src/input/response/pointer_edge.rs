/// What happened, as an *edge*: something that became true this frame.
///
/// **Edges only, deliberately.** A drag's travel is a level — true for as long
/// as the gesture lasts, and wanted as a number rather than as news — and a
/// level is what polling is good at. So this says *that* a drag started and on
/// what, and the caller reads the delta off that one widget's response for as
/// long as it cares. Reporting the delta here as well would be a second answer
/// to a question `Response` already answers, free to disagree with it about the
/// widget's transform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerEdge {
    /// The button went down on this widget. `count` is its place in the
    /// multi-press run — 1 for a single press, 2 for the second of a double.
    Pressed { count: u8 },
    /// Released back on it with no drag latched. `count` as above, so a
    /// double-click arrives as a `Clicked { count: 2 }`.
    Clicked { count: u8 },
    /// Travel passed the drag threshold, latching a drag on this widget.
    DragStarted,
    /// A latched drag ended — the commit edge for drag gestures.
    DragStopped,
}
