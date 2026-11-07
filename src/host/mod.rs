//! The host layer between the OS/GPU and the [`Ui`](crate::Ui) recorder.
//!
//! [`HostCore`](host_core::HostCore) bundles [`UiResources`](crate::ui::resources::UiResources) and the CPU frontend ([`HostCpu`](host_cpu::HostCpu)) with the GPU backend ([`HostDevice`](host_device::HostDevice)); each window's [`WindowFrames`](crate::ui::window_frames::WindowFrames) runs through it. [`winit`], [`offscreen`] and [`harness`] are the three hosts; each owns its time and hands it to each frame. Recorder vocabulary lives at the crate root so the `Ui` API doesn't depend on the host.

pub(crate) mod clock;
pub(crate) mod harness;
mod headless_window;
mod host_core;
mod host_cpu;
mod host_device;
mod host_options;
pub(crate) mod offscreen;
mod system_preferences;
mod wake_signal;
#[cfg(feature = "winit")]
pub(crate) mod winit;
