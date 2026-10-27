//! Everything a frame's recorder asks of its window host.

use crate::primitives::geometry::rect::Rect;
use crate::window::cursor_icon::CursorIcon;
use crate::window::screenshot::{ScreenshotId, ScreenshotRequest};
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
    /// The images this frame asked for, one per distinct region; the driver takes them once the
    /// frame is recorded.
    pub(crate) screenshots: Vec<ScreenshotRequest>,
    /// The id the next new ask takes; `None` before the first.
    next_screenshot: Option<ScreenshotId>,
}

impl WindowRequests {
    /// Ask for an image of the logical `region`, already clamped to the window and not empty.
    /// Asking a region again in the same frame, as a replayed record pass does, returns the same
    /// id.
    pub(crate) fn ask_screenshot(&mut self, region: Rect) -> ScreenshotId {
        debug_assert!(!region.is_paint_empty(), "an empty region asks nothing");
        if let Some(asked) = self.screenshots.iter().find(|asked| asked.region == region) {
            return asked.id;
        }
        let id = self.next_screenshot.unwrap_or(ScreenshotId::FIRST);
        self.next_screenshot = Some(id.next());
        self.screenshots.push(ScreenshotRequest { region, id });
        id
    }

    /// Open a record pass: the levels a pass asserts afresh, the cursor and the IME area, start
    /// unset, so a pass that does not ask leaves the default.
    pub(crate) fn begin_pass(&mut self) {
        self.levels.cursor = CursorIcon::default();
        self.levels.ime_area = None;
    }

    /// Move this frame's commands onto `out` and return the levels the host applies afterwards. Settles the pending close first: a `close_requested` that app code did not veto becomes `token`'s close command, for every host alike.
    ///
    /// The levels are copied, not taken: the recorder keeps them and reads `vsync` back through [`Ui::vsync`](crate::Ui::vsync).
    ///
    /// Uses [`WindowCommands::append`] so the recorder keeps its buffer's capacity.
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
