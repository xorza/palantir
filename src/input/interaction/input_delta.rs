//! What feeding one event changed, so a host can decide whether the frame
//! it would run is worth running.

/// Repaint hint returned by `Ui::on_input`: `true` when the event
/// changed something the next frame must reflect.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InputDelta {
    /// `true` when the event moved state the next frame has to show —
    /// hover crossing a widget boundary, a press latching, focus moving.
    /// A host that idles between frames wakes on this.
    pub requests_repaint: bool,
}
