/// When the per-frame classification gate decides whether input requires
/// re-recording, this enum picks the signal it consults.
///
/// `Always` matches the legacy behavior: any input event whatsoever —
/// including a pointer move over inert surface — forces a full
/// record→measure→arrange→cascade→encode pass. `OnDelta` consults the
/// finer-grained [`InputDelta::requests_repaint`](crate::InputDelta)
/// instead: pointer moves only force a record when the hover/scroll
/// target changed or a capture is active; scroll over a non-scroll
/// surface is dropped; a press records when it hits a sense target,
/// changes focus, or a `BUTTONS` watcher is live — a press on
/// fully inert surface is observably a no-op and stays on the
/// paint-anim path. Keys / IME route through focus and record.
///
/// Default is [`OnDelta`](Self::OnDelta) — the right behavior for
/// almost every app. Use [`Always`](Self::Always) only for telemetry,
/// host integrations that observe raw input without the reactive
/// [`Ui`](crate::Ui) queries, or any case where the build closure
/// observes state widgets don't route through the hit index.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InputPolicy {
    /// Re-record on any input event.
    Always,
    /// Re-record only when [`InputDelta::requests_repaint`](crate::InputDelta)
    /// fired on at least one event since the last frame.
    #[default]
    OnDelta,
}

impl InputPolicy {
    /// The weakest [`InputSignal`] this policy re-records for. The frame
    /// gate is then `signal >= policy.record_threshold()` — the policy
    /// names a cut on one ordered scale rather than selecting between two
    /// separately-tracked booleans.
    #[inline]
    pub(crate) fn record_threshold(self) -> InputSignal {
        match self {
            Self::Always => InputSignal::Inert,
            Self::OnDelta => InputSignal::Repaint,
        }
    }
}

/// The strongest input signal seen since the last frame — what
/// [`InputPolicy`] thresholds against.
///
/// **Ordered, and the order is the point.** Each level implies the one
/// below it: an event that could change the screen is also an event that
/// arrived. Tracking the two separately invited them to drift, since
/// nothing tied "repaint-worthy" to "arrived at all"; as one monotone
/// level the implication holds by construction and the frame gate is a
/// comparison.
///
/// Reset to [`None`](Self::None) once per frame, alongside the per-frame
/// event queues.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum InputSignal {
    /// The host pushed nothing since the last frame. A frame may still
    /// run — animation wakes and explicit repaint requests are separate
    /// signals — but input is not what forces it.
    #[default]
    None,
    /// Events arrived, none of which could change what is on screen: a
    /// pointer move over inert surface, scroll with no scroll target, a
    /// press that hit nothing and moved no focus. Enough to disqualify
    /// the paint-anim-only short-circuit, since the app's record closure
    /// may observe raw input the hit index knows nothing about.
    Inert,
    /// At least one event could change what is on screen — a hover or
    /// scroll-target change, a capture-active move, a click, a key, IME
    /// text, a modifier change.
    Repaint,
}

impl InputSignal {
    /// Raise to at least `level`. Monotone within a frame: an `Inert`
    /// event arriving after a `Repaint` one cannot lower the signal.
    #[inline]
    pub(crate) fn raise(&mut self, level: Self) {
        *self = (*self).max(level);
    }
}

/// What happens to the currently-focused widget when the user presses
/// the pointer somewhere that *isn't* a focusable widget. Set via
/// [`crate::Ui::set_focus_policy`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FocusPolicy {
    /// Pressing on a non-focusable widget or empty surface preserves
    /// the current focus. Friendlier for sketches and tooling UIs
    /// where every other widget is a Button — clicking a Button while
    /// editing a field keeps the cursor in the field.
    PreserveOnMiss,
    /// Pressing anywhere that isn't a focusable widget clears focus.
    /// Native-app convention on most platforms (click-outside-to-blur).
    /// Default.
    #[default]
    ClearOnMiss,
}

#[cfg(test)]
mod tests {
    use crate::input::policy::{InputPolicy, InputSignal};

    /// The whole reason the two booleans became one ordinal: "could
    /// repaint" must imply "arrived at all". As separate flags nothing
    /// enforced that; here it is the `Ord` derive, so pin it.
    #[test]
    fn repaint_implies_inert_implies_none() {
        assert!(InputSignal::Repaint > InputSignal::Inert);
        assert!(InputSignal::Inert > InputSignal::None);
        assert_eq!(InputSignal::default(), InputSignal::None);
    }

    /// `raise` is monotone — a later weaker event cannot lower a signal
    /// already raised, which is what makes fold order irrelevant across
    /// a frame's events.
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

    /// The policies must land on *different* cuts, or the setting does
    /// nothing. Pins the behavioural difference, not just the values:
    /// an inert event forces a record under `Always` and not under
    /// `OnDelta`, while a repaint-worthy one forces both.
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
