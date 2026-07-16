//! The host layer — everything between the OS/GPU and the [`Ui`](crate::Ui)
//! recorder. [`HostContext`](context::HostContext) is the app-global shared
//! bag every window's `Ui` and the one shared `WgpuBackend` clone from;
//! [`WindowRenderer`](window_renderer::WindowRenderer) owns each window's
//! record store and drives frames through that backend; [`winit`] and
//! [`offscreen`] are the two
//! drivers (swapchain windows / render-to-texture); [`clock`] is the injected
//! per-frame time source. The backend-agnostic *vocabulary* the recorder
//! shares with this layer (`Display`, `WindowConfig`/`WindowToken`,
//! `DebugOverlayConfig`) deliberately lives at the crate root, not here — the
//! `Ui` API must not depend on the host machinery.

pub(crate) mod clock;
pub(crate) mod context;
pub(crate) mod offscreen;
pub(crate) mod window_renderer;
pub(crate) mod winit;
