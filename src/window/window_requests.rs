//! Everything a frame's recorder asks of its window host.

use crate::window::window_commands::WindowCommands;
use crate::window::window_output::WindowOutput;

/// Deferred recorder output consumed by the window host after a frame.
///
/// **Split by what a host can do about it**, because that is the one
/// distinction every host has to act on. `commands` are *edges*: one-shot
/// lifecycle requests that mean nothing unless something services them, so
/// a host that cannot has to say so rather than swallow them. `levels` are
/// *settings*: the recorder retains them, so a host with nothing to apply
/// them to leaves the app's own view of them intact by doing nothing. Only
/// `vsync` reads back through `Ui` — the field doc below says why `cursor`
/// has nothing to read.
///
/// That split is why `WindowDriver::drain_window_output` hands the two
/// halves to different places, and why the offscreen host can reject one
/// half while accepting the other without either being an arbitrary
/// per-field choice.
#[derive(Debug, Default)]
pub(crate) struct WindowRequests {
    pub(crate) commands: WindowCommands,
    /// Whether app code vetoed the current close request.
    pub(crate) close_vetoed: bool,
    /// The cursor and presentation pacing this window is currently set to.
    ///
    /// Both are levels rather than edges, but they are retained for
    /// opposite reasons. `cursor` is re-asserted by whoever still wants it
    /// each record pass, and retained only so a `PaintOnly` frame — which
    /// runs no pass — does not flicker it back to the default; app code
    /// writes it every frame and so has nothing to read back, which is why
    /// `Ui` offers no reader for it. `vsync` is seeded from the swapchain
    /// the host actually opened, so it answers
    /// [`Ui::set_vsync`](crate::Ui::set_vsync) truthfully from the first
    /// frame and a recorder never has to re-assert it. Reconfiguring a
    /// swapchain is expensive, so the *host* compares these against what
    /// is in force and acts only on a real flip.
    pub(crate) levels: WindowOutput,
}
