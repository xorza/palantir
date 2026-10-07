//! What counts as input arriving, and what an unfocusable press does to focus.

/// Which signal the per-frame gate consults to decide whether input requires
/// re-recording.
///
/// `Always`: any input event, even a pointer move over inert surface, forces a
/// full record pass. `OnDelta`: only `InputDelta::repaint_requested` does, i.e.
/// a hover/scroll-target change or active capture, a press hitting a sense
/// target, changing focus, or with a `BUTTONS` watcher live; keys route through
/// focus and record.
///
/// Default is [`OnDelta`](Self::OnDelta). Use [`Always`](Self::Always) for
/// telemetry, hosts observing raw input, or build closures reading state that
/// bypasses the hit index.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InputPolicy {
    /// Re-record on any input event.
    Always,
    /// Re-record only when `InputDelta::repaint_requested` fired on an event since
    /// the last frame.
    #[default]
    OnDelta,
}

impl InputPolicy {
    /// The weakest [`InputSignal`] this policy re-records for; the gate is
    /// `signal >= policy.record_threshold()`.
    #[inline]
    pub(crate) const fn record_threshold(self) -> InputSignal {
        match self {
            Self::Always => InputSignal::Inert,
            Self::OnDelta => InputSignal::Repaint,
        }
    }
}

/// The strongest input signal seen since the last frame, which
/// [`InputPolicy`] thresholds against.
///
/// **Ordered:** each level implies the one below it (an event that could change
/// the screen also arrived), which one monotone level guarantees by
/// construction. Reset to [`None`](Self::None) once per frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum InputSignal {
    /// The host pushed nothing since the last frame. A frame may still run for
    /// animation wakes or repaint requests.
    #[default]
    None,
    /// Events arrived, none able to change the screen: a pointer move over inert
    /// surface, scroll with no target, a press that hit nothing. Records only
    /// under [`InputPolicy::Always`].
    Inert,
    /// An event could change the screen: a hover or scroll-target change, a
    /// capture-active move, a click, a key, a modifier change.
    Repaint,
}

impl InputSignal {
    /// Raise to at least `level`; monotone within a frame.
    #[inline]
    pub(crate) fn raise(&mut self, level: Self) {
        *self = (*self).max(level);
    }
}

/// What happens to the focused widget when the pointer presses somewhere that
/// isn't a focusable widget. Set via [`crate::Ui::set_focus_policy`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FocusPolicy {
    /// A press on a non-focusable widget or empty surface keeps focus, so e.g.
    /// clicking a Button while editing a field keeps the cursor there.
    PreserveOnMiss,
    /// A press anywhere not focusable clears focus (click-outside-to-blur). Default.
    #[default]
    ClearOnMiss,
}

#[cfg(test)]
mod tests {
    use crate::input::policy::{InputPolicy, InputSignal};

    /// "Could repaint" must imply "arrived at all": pinned by the `Ord` derive.
    #[test]
    fn repaint_implies_inert_implies_none() {
        assert!(InputSignal::Repaint > InputSignal::Inert);
        assert!(InputSignal::Inert > InputSignal::None);
        assert_eq!(InputSignal::default(), InputSignal::None);
    }

    /// `raise` is monotone, so fold order across a frame's events is irrelevant.
    #[test]
    fn raise_never_lowers() {
        let mut s = InputSignal::None;
        s.raise(InputSignal::Repaint);
        s.raise(InputSignal::Inert);
        assert_eq!(s, InputSignal::Repaint, "Inert must not lower Repaint");

        let mut s = InputSignal::None;
        s.raise(InputSignal::Inert);
        s.raise(InputSignal::Repaint);
        assert_eq!(s, InputSignal::Repaint);
    }

    /// The policies must land on different cuts: an inert event forces a record
    /// under `Always` and not `OnDelta`; a repaint-worthy one forces both.
    #[test]
    fn policies_cut_the_scale_differently() {
        let forces = |p: InputPolicy, s: InputSignal| s >= p.record_threshold();

        assert!(forces(InputPolicy::Always, InputSignal::Inert));
        assert!(!forces(InputPolicy::OnDelta, InputSignal::Inert));

        assert!(forces(InputPolicy::Always, InputSignal::Repaint));
        assert!(forces(InputPolicy::OnDelta, InputSignal::Repaint));

        assert!(!forces(InputPolicy::Always, InputSignal::None));
        assert!(!forces(InputPolicy::OnDelta, InputSignal::None));
    }
}
