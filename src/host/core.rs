//! `HostCore`: the composition root both hosts build on: [`UiResources`], the one CPU [`Frontend`] and
//! the one [`WgpuBackend`] every window renders through, built together to share registries and one
//! texture-dimension cap. It does not own the [`WindowDriver`]s; each host pairs its own.

use crate::app::App;
use crate::common::clipboard::Clipboard;
use crate::display::Display;
use crate::gpu::device::backend_config::BackendConfig;
use crate::gpu::device::backend_resources::BackendResources;
use crate::gpu::device::gpu::Gpu;
use crate::gpu::surface::render_target::RenderTarget;
use crate::gpu::wgpu_backend::WgpuBackend;
use crate::host::window_driver::{
    CpuFrame, PresentPath, Presentation, WindowDriver, WindowDriverBuilder,
};
use crate::renderer::frontend::Frontend;
use crate::renderer::texture_limit::TextureLimit;
use crate::text::shaper::TextShaper;
use crate::ui::resources::UiResources;
use crate::window::window_token::WindowToken;

/// What a host seals at build beside its device; `pixel_snap` is the host's, inherited by every driver.
#[derive(Debug)]
pub(super) struct HostCoreConfig {
    pub(super) shaper: TextShaper,
    pub(super) clipboard: Clipboard,
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
    /// The texture cap is read from `gpu` once, so the surface clamp, [`TextureLimit`] and the frontend clamp are one number.
    pub(super) fn new(gpu: Gpu, config: HostCoreConfig) -> Self {
        let max_texture_dim = gpu.max_texture_dim();
        let resources = UiResources::new(
            config.shaper,
            config.clipboard,
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

    /// The device and queue every window of this host presents through.
    #[cfg_attr(
        not(feature = "winit"),
        expect(
            dead_code,
            reason = "only the windowed host presents through the device itself"
        )
    )]
    pub(super) const fn gpu(&self) -> &Gpu {
        self.backend.gpu()
    }

    pub(super) fn driver(&self, token: WindowToken) -> WindowDriverBuilder<'_> {
        WindowDriver::builder(token, &self.resources, self.pixel_snap)
            .cutout_mode(self.backend.cutout_mode())
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
        driver.render_to_texture(
            &self.frontend.buffer,
            &mut self.backend,
            Presentation { target, mode },
        );
    }
}
