//! Where one pointer button sits in its press lifecycle on one widget.

/// One pointer button's press lifecycle on a widget, one phase per frame: `Idle` → `Down` (press edge) → `Held` → `Up` (release edge) → `Idle`.
///
/// `Down`/`Held` follow the capture, not the rect: they keep reporting while the pointer drags off the widget, with no travel threshold.
///
/// Presses on the same widget within the double-click window and radius form a run; `Down { count }` is the position in it and a completing click carries it in `Up { click }`, so `Up { click: Some(2) }` is the double-click.
///
/// Collapsed batches: press+release gives `Up` (the click outranks the lost press edge); release+re-press gives `Down` (the live capture outranks the release).
///
/// [`PointerEdge`](crate::PointerEdge) reports the same edges frame-wide as a second walk, not a projection of this one: a batch that presses, releases and presses again yields two edges there but one phase here.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum ButtonPhase {
    /// Not down on this widget; no edge this frame.
    #[default]
    Idle,
    /// One-frame edge: the press landed this frame. Rises on the press (clicks fire on release), so press-driven gestures react while the button is down.
    Down {
        /// Position of this press in its multi-press run (1 single, 2 double, ...); the same number [`Self::Up`]'s click and [`PointerEdge`](crate::PointerEdge) report.
        count: u8,
    },
    /// Latched on the widget: the frames after the press edge.
    Held,
    /// One-frame edge: released this frame.
    Up {
        /// `Some(n)` when this release completed a click (press + release on the widget, no drag), `n` its position in the run; `None` when a drag ate it or the release landed off the widget.
        click: Option<u8>,
    },
}
