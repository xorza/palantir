/// When the per-frame [`Ui::classify_frame`](crate::Ui::classify_frame)
/// gate decides "did any input arrive that requires re-recording?",
/// this enum picks the signal it consults.
///
/// `Always` matches the legacy behavior: any input event whatsoever —
/// including a pointer move over inert surface — forces a full
/// record→measure→arrange→cascade→encode pass. `OnDelta` consults the
/// finer-grained [`InputDelta::requests_repaint`](crate::input::InputDelta)
/// instead: pointer moves only force a record when the hover/scroll
/// target changed or a capture is active; scroll over a non-scroll
/// surface is dropped; a press records when it hits a sense target,
/// changes focus, or a `BUTTONS` subscriber is live — a press on
/// fully inert surface is observably a no-op and stays on the
/// paint-anim path. Keys / IME route through focus and record.
///
/// Default is [`OnDelta`](Self::OnDelta) — the right behavior for
/// almost every app. Use [`Always`](Self::Always) only for telemetry,
/// custom canvases that paint raw pointer position without declaring
/// `Sense::HOVER`, or any case where the build closure observes
/// pointer state widgets don't route through the hit index.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InputPolicy {
    /// Re-record on any input event.
    Always,
    /// Re-record only when [`InputDelta::requests_repaint`](crate::input::InputDelta)
    /// fired on at least one event since the last frame.
    #[default]
    OnDelta,
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
