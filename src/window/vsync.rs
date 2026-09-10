//! Whether presentation waits for the display's refresh.

/// Whether a window's swapchain waits for the display's refresh before
/// presenting, requested through [`Ui::set_vsync`](crate::Ui::set_vsync) at
/// runtime and [`WinitHostBuilder::vsync`](crate::WinitHostBuilder::vsync) at
/// startup.
/// Backend-agnostic like [`CursorIcon`](crate::window::cursor_icon::CursorIcon) — the two states map onto the
/// host's *automatic* present policies, so the backend still picks the
/// concrete swapchain mode each surface actually supports.
///
/// Deliberately two-state, and the only presentation vocabulary Palantir
/// has. "Wait for vblank or don't" is the whole of what the question means
/// to the person a control is put in front of, and each state maps to an
/// *automatic* swapchain policy every surface accepts, so there is nothing
/// to negotiate. The finer modes (Mailbox, Immediate, FifoRelaxed) are a
/// game's concern, and would cost a mirrored enum, two conversions and a
/// negotiation pass to name.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Vsync {
    /// Present in step with the display. Tear-free, frame rate capped to
    /// the refresh rate.
    #[default]
    On,
    /// Present as soon as a frame is ready. Uncapped, and may tear.
    Off,
}
