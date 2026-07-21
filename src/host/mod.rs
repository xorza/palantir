//! The host layer — everything between the OS/GPU and the [`Ui`](crate::Ui)
//! recorder. [`HostShared`](shared::HostShared) owns the app-global resources
//! exposed to each `Ui` and the shared renderer;
//! [`WindowDriver`](window_driver::WindowDriver) owns each window's `Ui`
//! and drives frames through one host-owned CPU frontend and GPU backend; the
//! `Ui` owns its retained record store. [`winit`] and
//! [`offscreen`] are the two
//! drivers (swapchain windows / render-to-texture); [`clock`] is the injected
//! per-frame time source. The backend-agnostic *vocabulary* the recorder
//! shares with this layer (`Display`, `WindowConfig`/`WindowToken`,
//! `DebugOverlayConfig`) deliberately lives at the crate root, not here — the
//! `Ui` API must not depend on the host machinery.

pub(crate) mod clock;
pub(crate) mod offscreen;
pub(crate) mod shared;
#[cfg(feature = "internals")]
pub(crate) mod test_gpu;
pub(crate) mod window_driver;
#[cfg(feature = "winit-host")]
pub(crate) mod winit;
