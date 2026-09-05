//! The host layer — everything between the OS/GPU and the [`Ui`](crate::Ui)
//! recorder. [`UiResources`](crate::ui::resources::UiResources) is the
//! app-global bundle each `Ui` and the shared renderer are built over;
//! [`HostCore`](core::HostCore) bundles those resources with the one CPU
//! frontend and GPU backend both hosts build on;
//! [`WindowDriver`](window_driver::WindowDriver) owns each window's `Ui`
//! and drives frames through that core; the
//! `Ui` owns its retained record store. [`winit`] and
//! [`offscreen`] are the two
//! drivers (swapchain windows / render-to-texture); [`clock`] is the injected
//! per-frame time source. The backend-agnostic *vocabulary* the recorder
//! shares with this layer (`Display`, `WindowConfig`/`WindowToken`,
//! `DebugOverlayConfig`) deliberately lives at the crate root, not here — the
//! `Ui` API must not depend on the host machinery.

#[cfg(feature = "bench")]
pub(crate) mod bench_gpu;
pub(crate) mod clock;
mod core;
pub(crate) mod device_requirements;
pub(crate) mod error;
pub(crate) mod gpu_request;
pub(crate) mod offscreen;
#[cfg(feature = "internals")]
pub(crate) mod test_gpu;
mod window_driver;
#[cfg(feature = "winit")]
pub(crate) mod winit;
