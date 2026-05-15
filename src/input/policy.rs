/// When the per-frame [`Ui::classify_frame`](crate::Ui::classify_frame)
/// gate decides "did any input arrive that requires re-recording?",
/// this enum picks the signal it consults.
///
/// `Always` matches the legacy behavior: any input event whatsoever —
/// including a pointer move over inert surface — forces a full
/// record→measure→arrange→cascade→encode pass. `OnDelta` consults the
/// finer-grained [`InputDelta::requests_repaint`](super::InputDelta)
/// instead: pointer moves only force a record when the hover/scroll
/// target changed or a capture is active; scroll over a non-scroll
/// surface is dropped; clicks / keys / IME still always record (their
/// `requests_repaint` is unconditionally `true`).
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
    /// Re-record only when [`InputDelta::requests_repaint`](super::InputDelta)
    /// fired on at least one event since the last frame.
    #[default]
    OnDelta,
}
