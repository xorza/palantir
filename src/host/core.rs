//! `HostCore` — the composition root both hosts build on: the app-global
//! [`HostShared`] resources, the one CPU [`Frontend`], and the one
//! [`WgpuBackend`] every window renders through.
//!
//! The three are constructed together because they are not independent: the
//! backend's capability bundle, the frontend's gradient-atlas handle, and both
//! halves' texture-dimension cap all derive from the same `HostShared` over
//! the same device. Building them apart means re-deriving that wiring per
//! host, which is what this type exists to stop.
//!
//! It deliberately does *not* own the [`WindowDriver`]s. Each host pairs a
//! driver with whatever its target needs — a swapchain and native handle for
//! [`WinitHost`](crate::WinitHost), nothing at all for
//! [`OffscreenHost`](crate::OffscreenHost) — so the map lives with the host
//! and the drivers are passed back in per frame.

use crate::Display;
use crate::app::App;
use crate::common::clipboard::Clipboard;
use crate::host::device_requirements::DeviceRequirements;
use crate::host::shared::HostShared;
use crate::host::window_driver::{CpuFrame, PresentMode, WindowDriver, WindowDriverBuilder};
use crate::renderer::backend::WgpuBackend;
use crate::renderer::backend::backend_config::BackendConfig;
use crate::renderer::frontend::Frontend;
use crate::renderer::texture_limit::TextureLimit;
use crate::text::shaper::TextShaper;
use crate::window::window_token::WindowToken;

#[derive(Debug)]
pub(super) struct HostCore {
    pub(super) shared: HostShared,
    /// Shared CPU encode/compose allocations, reused serially across windows.
    pub(super) frontend: Frontend,
    /// The one shared GPU renderer every window draws through (pipelines,
    /// atlases, image textures).
    pub(super) backend: WgpuBackend,
}

impl HostCore {
    pub(super) fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        shaper: TextShaper,
        clipboard: Clipboard,
        config: BackendConfig,
    ) -> Self {
        let max_texture_dim = DeviceRequirements::max_texture_dim(&device);
        let shared = HostShared::with_clipboard(
            shaper,
            clipboard,
            TextureLimit::from_device(max_texture_dim),
        );
        let backend = WgpuBackend::new(device, queue, shared.backend_resources(), config);
        let frontend = Frontend::new(max_texture_dim.get(), shared.gradient_atlas.clone());
        Self {
            shared,
            frontend,
            backend,
        }
    }

    /// Start building a render stream for `token` against these shared
    /// resources. Defaults suit a swapchain window — see
    /// [`WindowDriver::builder`].
    pub(super) fn driver(&self, token: WindowToken) -> WindowDriverBuilder<'_> {
        self.shared.resources.windows.set_live(token, true);
        WindowDriver::builder(token, &self.shared)
    }

    /// Retire a closed window's render stream, freeing the `GpuView` targets
    /// scoped to it. Backend eviction is owner-scoped, so a closed window's
    /// targets have no submit left to be absent from and would otherwise be
    /// held until host shutdown.
    #[cfg_attr(
        not(feature = "winit"),
        expect(
            dead_code,
            reason = "multi-window lifecycle plumbing: every caller is under                       src/host/winit/, so a build without that feature has                       nothing to call it"
        )
    )]
    pub(super) fn retire(&mut self, driver: &WindowDriver) {
        self.backend.retire_render_owner(driver.render_owner);
        self.shared.resources.windows.set_live(driver.token, false);
    }

    pub(super) fn cpu_frame<T: App>(
        &mut self,
        driver: &mut WindowDriver,
        display: Display,
        app: &mut T,
    ) -> CpuFrame {
        driver.cpu_frame(&mut self.frontend, display, app)
    }

    pub(super) fn submit(
        &mut self,
        driver: &mut WindowDriver,
        target: &wgpu::Texture,
        mode: PresentMode,
    ) {
        driver.render_to_texture(&self.frontend.buffer, &mut self.backend, target, mode);
    }
}
