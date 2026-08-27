/// One pointer button's press lifecycle on a widget. The phases are
/// mutually exclusive per frame, walked in order:
///
/// `Idle` → `Down` (press-edge frame) → `Held` (every following held
/// frame) → `Up` (release-edge frame) → `Idle`.
///
/// `Down`/`Held` are capture-based and rect-independent: they keep
/// reporting while the pointer drags outside the widget's rect or off
/// the surface entirely (no travel threshold — live from the first
/// press frame). Drag-tracking widgets (text selection) ride that to
/// keep following the pointer past their own bounds.
///
/// Multi-press runs ride the phases: presses chain when they land on
/// the same widget within the configured double-click time window and
/// pointer radius; any break resets the run. `Down.press` is the press's
/// position in its run (1 = single,
/// 2 = double-press, 3+ = triple…), and a completing click carries the
/// same number in `Up.click` — so `Up { click: Some(2) }` *is* the
/// double-click, and the second click of a double still reads as a
/// click (`clicked()` and `double_clicked()` both fire on it).
///
/// Collapsed edge cases (one event batch, no frame between): a
/// press+release collapses to `Up` (the completed click outranks the
/// lost press edge); a release+re-press collapses to `Down` (the live
/// capture outranks the stale release).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum ButtonPhase {
    /// The button is not down on this widget, and no edge fired this
    /// frame. The resting phase.
    #[default]
    Idle,
    /// One-frame edge: the press landed this frame. `press` = its
    /// position in the multi-press run. Rises on the press — clicks
    /// fire on the release — so press-driven gestures (caret
    /// placement, press-select-drag) react while the button is still
    /// down.
    Down {
        /// Position of this press in its multi-press run: 1 for a single
        /// press, 2 for the second press of a double, 3+ for triple and up.
        press: u8,
    },
    /// The press is latched on the widget (level, frames after the
    /// press edge).
    Held,
    /// One-frame edge: released this frame. `click` is `Some(n)` when
    /// the release completed a click (press + release on the widget,
    /// no drag latched), with `n` the click's position in its
    /// multi-press run; `None` when a drag suppressed the click or
    /// the release landed off the widget.
    Up {
        /// `Some(n)` when this release completed a click, `n` being the
        /// click's position in its multi-press run — `Some(2)` *is* the
        /// double-click. `None` when a drag ate the click or the release
        /// landed outside the widget.
        click: Option<u8>,
    },
}
