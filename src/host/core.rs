//! `HostCore`: the composition root both hosts build on: [`UiResources`], the one CPU [`Frontend`] and
//! the one [`WgpuBackend`] every window renders through, built together to share registries and one
//! texture-dimension cap. It does not own the [`WindowDriver`]s; each host pairs its own.

use crate::app::App;
use crate::common::clipboard::Clipboard;
use crate::gpu::device::backend_config::BackendConfig;
use crate::gpu::device::backend_resources::BackendResources;
use crate::gpu::device::gpu::Gpu;
use crate::gpu::resource::wgpu_image_store::WgpuImageStore;
use crate::gpu::surface::render_target::RenderTarget;
use crate::gpu::wgpu_backend::WgpuBackend;
use crate::host::window_driver::{
    CpuFrame, DriverConfig, FrameSpec, PresentPath, PresentStrategy, WindowDriver,
};
use crate::image_registry::ImageRegistry;
use crate::renderer::frontend::Frontend;
use crate::renderer::texture_limit::TextureLimit;
use crate::text::shaper::TextShaper;
use crate::ui::resources::{UiDevice, UiResources};
use crate::window::window_token::WindowToken;
use std::num::NonZeroU32;
use std::rc::Rc;

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
    max_texture_dim: NonZeroU32,
}

impl HostCore {
    /// The texture cap is read from `gpu` once, so the surface clamp, [`TextureLimit`] and the frontend clamp are one number.
    /// The image store comes first, so the registry every recorder clones holds it from the start.
    pub(super) fn new(gpu: Gpu, config: HostCoreConfig) -> Self {
        let max_texture_dim = gpu.max_texture_dim();
        let image_store = Rc::new(WgpuImageStore::new(&gpu));
        let resources = UiResources::new(
            config.shaper,
            config.clipboard,
            UiDevice {
                texture_limit: TextureLimit::from_device(max_texture_dim),
                images: ImageRegistry::new(Rc::clone(&image_store)),
            },
        );
        let backend = WgpuBackend::new(
            gpu,
            BackendResources {
                text: resources.text(),
                image_store: &image_store,
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
            max_texture_dim,
        }
    }

    /// The device's largest 2D texture side, read once: the cap on surfaces, images and atlases.
    #[cfg_attr(
        not(feature = "winit"),
        expect(dead_code, reason = "only a swapchain is sized by the platform")
    )]
    pub(super) const fn max_texture_dim(&self) -> NonZeroU32 {
        self.max_texture_dim
    }

    /// The device and queue every window of this host presents through.
    pub(super) const fn gpu(&self) -> &Gpu {
        self.backend.gpu()
    }

    /// A driver for the window `token`, presenting by `strategy`.
    pub(super) fn open(&self, token: WindowToken, strategy: PresentStrategy) -> WindowDriver {
        WindowDriver::new(
            token,
            &self.resources,
            DriverConfig {
                strategy,
                pixel_snap: self.pixel_snap,
                cutout_mode: self.backend.cutout_mode(),
            },
        )
    }

    /// Run the CPU half of the frame `spec` describes: note its target, hand over a screenshot
    /// the device finished, and record `app` under the display minted from `spec`.
    pub(super) fn record<T: App>(
        &mut self,
        driver: &mut WindowDriver,
        spec: FrameSpec,
        app: &mut T,
    ) -> CpuFrame {
        driver.note_target(&spec);
        driver.deliver_screenshot(&self.backend);
        let stamp = driver.stamp(&spec);
        driver.cpu_frame(&mut self.frontend, stamp, app)
    }

    /// The GPU half: draw the recorded frame into `target` by `mode`.
    pub(super) fn submit(
        &mut self,
        driver: &mut WindowDriver,
        target: RenderTarget<'_>,
        mode: PresentPath,
    ) {
        driver.render_to_texture(&self.frontend.buffer, &mut self.backend, target, mode);
    }

    /// Close a window: its render stream retires, freeing its `GpuView` targets, which
    /// owner-scoped eviction would otherwise hold until shutdown, and the driver's `Drop` retires
    /// its directory entry.
    #[cfg_attr(
        not(feature = "winit"),
        expect(
            dead_code,
            reason = "only the windowed host closes a window while it runs"
        )
    )]
    pub(super) fn close(&mut self, driver: WindowDriver) {
        self.backend.retire_render_owner(driver.render_owner);
        drop(driver);
    }
}
