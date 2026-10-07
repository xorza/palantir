//! `HostCore`: the composition root both hosts build on: [`UiResources`], the one CPU [`Frontend`] and
//! the one [`WgpuBackend`] every window renders through, built together to share registries and one
//! texture-dimension cap. It does not own the [`WindowDriver`]s; each host pairs its own.

use crate::app::App;
use crate::common::clipboard::Clipboard;
use crate::display::Display;
use crate::gpu::device::backend_config::BackendConfig;
use crate::gpu::device::backend_resources::BackendResources;
use crate::gpu::device::requested_gpu::Gpu;
use crate::gpu::surface::render_target::RenderTarget;
use crate::gpu::wgpu_backend::WgpuBackend;
use crate::host::window_driver::{CpuFrame, PresentPath, WindowDriver, WindowDriverBuilder};
use crate::renderer::frontend::Frontend;
use crate::renderer::texture_limit::TextureLimit;
use crate::text::shaper::TextShaper;
use crate::ui::resources::UiResources;
use crate::window::window_token::WindowToken;
use std::num::NonZeroU32;

/// App-global choices a host seals at build; `pixel_snap` is the host's, inherited by every driver.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct HostCoreConfig {
    pub(super) collect_gpu_stats: bool,
    pub(super) pixel_snap: bool,
}

#[derive(Debug)]
pub(super) struct HostCore {
    pub(super) resources: UiResources,
    pub(super) frontend: Frontend,
    pub(super) backend: WgpuBackend,
    pixel_snap: bool,
}

impl HostCore {
    /// `max_texture_dim` is passed in so surface clamp, [`TextureLimit`] and frontend clamp are one number.
    pub(super) fn new(
        gpu: Gpu,
        max_texture_dim: NonZeroU32,
        shaper: TextShaper,
        clipboard: Clipboard,
        config: HostCoreConfig,
    ) -> Self {
        let resources = UiResources::new(
            shaper,
            clipboard,
            TextureLimit::from_device(max_texture_dim),
        );
        let backend = WgpuBackend::new(
            gpu,
            BackendResources {
                text: resources.text(),
                images: resources.images(),
                icons: resources.icons(),
                gradient_atlas: resources.gradient_atlas(),
                gpu_pass_stats: &resources.diagnostics().gpu_pass_stats,
            },
            BackendConfig {
                collect_gpu_stats: config.collect_gpu_stats,
            },
        );
        let frontend = Frontend::new(max_texture_dim, resources.gradient_atlas().clone());
        Self {
            resources,
            frontend,
            backend,
            pixel_snap: config.pixel_snap,
        }
    }

    pub(super) fn driver(&self, token: WindowToken) -> WindowDriverBuilder<'_> {
        WindowDriver::builder(token, &self.resources, self.pixel_snap)
            .bake_cutouts(self.backend.bakes_cutouts())
    }

    /// Retires a closed window's render stream, freeing its `GpuView` targets, which owner-scoped eviction
    /// would otherwise hold until shutdown. The driver's `Drop` retires its directory entry.
    #[cfg_attr(
        not(feature = "winit"),
        expect(
            dead_code,
            reason = "multi-window lifecycle plumbing: every caller is \
                      under src/host/winit/, so a build without that \
                      feature has nothing to call it"
        )
    )]
    pub(super) fn retire(&mut self, driver: &WindowDriver) {
        self.backend.retire_render_owner(driver.render_owner);
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
        target: RenderTarget<'_>,
        mode: PresentPath,
    ) {
        driver.render_to_texture(&self.frontend.buffer, &mut self.backend, target, mode);
    }
}
