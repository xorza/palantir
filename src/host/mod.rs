//! The host layer between the OS/GPU and the [`Ui`](crate::Ui) recorder.
//!
//! [`HostCore`](core::HostCore) bundles [`UiResources`](crate::ui::resources::UiResources) with the CPU frontend and GPU backend; [`WindowDriver`](window_driver::WindowDriver) drives each window's `Ui` through it. [`winit`] and [`offscreen`] are the two drivers; [`clock`] injects time. Recorder vocabulary lives at the crate root so the `Ui` API doesn't depend on the host.

pub(crate) mod clock;
mod core;
pub(crate) mod offscreen;
mod window_driver;
#[cfg(feature = "winit")]
pub(crate) mod winit;
