//! The host layer between the OS/GPU and the [`Ui`](crate::Ui) recorder.
//!
//! [`HostCore`](core::HostCore) bundles [`UiResources`](crate::ui::resources::UiResources) and the CPU frontend ([`CpuCore`](cpu_core::CpuCore)) with the GPU backend ([`DeviceCore`](device_core::DeviceCore)); [`WindowDriver`](window_driver::WindowDriver) drives each window's `Ui` through it. [`winit`] and [`offscreen`] are the two drivers; each owns its time and hands it to each frame. Recorder vocabulary lives at the crate root so the `Ui` API doesn't depend on the host.

pub(crate) mod clock;
mod core;
mod cpu_core;
mod device_core;
pub(crate) mod harness;
mod headless_window;
mod host_options;
pub(crate) mod offscreen;
mod system_preferences;
mod wake_signal;
mod window_driver;
#[cfg(feature = "winit")]
pub(crate) mod winit;
