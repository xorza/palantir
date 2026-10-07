//! Everything a frame's recorder asks of its window host.

use crate::window::window_commands::WindowCommands;
use crate::window::window_output::WindowOutput;
use crate::window::window_token::WindowToken;

/// Deferred recorder output consumed by the window host after a frame, split by what a host can do about it: `commands` are one-shot edges a host that cannot service must say so rather than swallow; `levels` are retained settings, so a host with nothing to apply them leaves the app's view intact. Only `vsync` reads back through `Ui`.
#[derive(Debug, Default)]
pub(crate) struct WindowRequests {
    pub(crate) commands: WindowCommands,
    /// Whether app code vetoed the current close request.
    pub(crate) close_vetoed: bool,
    /// The cursor and presentation pacing this window is set to; levels, retained for opposite reasons. `cursor` is re-asserted each record pass and kept only so a `PaintOnly` frame does not flicker it to default (hence no `Ui` reader). `vsync` is seeded from the opened swapchain, so [`Ui::set_vsync`](crate::Ui::set_vsync) answers truthfully from frame one. The host acts only on a real flip, since reconfiguring a swapchain is expensive.
    pub(crate) levels: WindowOutput,
}

impl WindowRequests {
    /// Move this frame's commands onto `out` and return the levels the host applies afterwards. Settles the pending close first: a `close_requested` that app code did not veto becomes `token`'s close command, for every host alike.
    ///
    /// The levels are copied, not taken: the recorder keeps them and reads `vsync` back through [`Ui::vsync`](crate::Ui::vsync).
    ///
    /// Uses [`WindowCommands::append`] so the recorder keeps its buffers' capacity.
    ///
    /// **The veto's one-frame life is enforced here** for every host (the offscreen one drains through this too), so no caller clears it and `Ui::set_window_facts` can assert.
    pub(crate) fn drain(
        &mut self,
        token: WindowToken,
        close_requested: bool,
        out: &mut WindowCommands,
    ) -> WindowOutput {
        if close_requested && !self.close_vetoed {
            self.commands.close(token);
        }
        out.append(&mut self.commands);
        self.close_vetoed = false;
        self.levels
    }
}

#[cfg(test)]
mod tests;
