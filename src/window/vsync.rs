//! Whether presentation waits for the display's refresh.

/// Whether a window's swapchain waits for the display's refresh before
/// presenting, requested through [`Ui::set_vsync`](crate::Ui::set_vsync).
/// Backend-agnostic like [`CursorIcon`](crate::window::cursor_icon::CursorIcon) — the two states map onto the
/// host's *automatic* present policies, so the backend still picks the
/// concrete swapchain mode each surface actually supports.
///
/// Deliberately two-state. The full presentation vocabulary (Fifo,
/// Mailbox, Immediate, …) stays a startup knob on the host's own config,
/// where naming backend types is fine; this is the runtime toggle an
/// application puts in front of a user, and "wait for vblank or don't" is
/// the whole of what that question means to them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Vsync {
    /// Present in step with the display. Tear-free, frame rate capped to
    /// the refresh rate.
    #[default]
    On,
    /// Present as soon as a frame is ready. Uncapped, and may tear.
    Off,
}
