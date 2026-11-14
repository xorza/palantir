//! Whether presentation waits for the display's refresh.

/// Whether a window's swapchain waits for the display's refresh.
///
/// Set through [`Ui::set_vsync`](crate::Ui::set_vsync) or [`WinitHostBuilder::vsync`](crate::WinitHostBuilder::vsync). Each state maps to an automatic present policy every surface accepts, so nothing is negotiated.
///
/// An enum, not the `bool` a two-valued setting takes, as presentation has more modes than two,
/// as `wgpu::PresentMode` lists them (mailbox, relaxed FIFO), and one may join these.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Vsync {
    /// Tear-free; capped to the refresh rate.
    #[default]
    On,
    /// Present as soon as a frame is ready. Uncapped, and may tear.
    Off,
}
