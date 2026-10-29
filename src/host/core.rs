//! `HostCore`: the composition root both hosts build on: [`UiResources`], the one CPU [`Frontend`] and
//! the one [`WgpuBackend`] every window renders through, built together to share registries and one
//! texture-dimension cap. It does not own the [`WindowDriver`]s; each host pairs its own.

use crate::app::App;
use crate::gpu::device::backend_resources::BackendResources;
use crate::gpu::device::gpu::Gpu;
use crate::gpu::resource::wgpu_image_store::WgpuImageStore;
use crate::gpu::surface::render_target::RenderTarget;
use crate::gpu::wgpu_backend::{FrameTiming, WgpuBackend};
use crate::host::host_options::HostOptions;
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
    /// `options.pixel_snap` is the host's, inherited by every driver.
    pub(super) fn new(gpu: Gpu, options: &HostOptions, shaper: TextShaper) -> Self {
        let max_texture_dim = gpu.max_texture_dim();
        let image_store = Rc::new(WgpuImageStore::new(&gpu));
        let resources = UiResources::new(
            shaper,
            options.clipboard(),
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
            },
            if options.collect_gpu_stats {
                FrameTiming::On
            } else {
                FrameTiming::Off
            },
        );
        let frontend = Frontend::new(max_texture_dim, resources.gradient_atlas().clone());
        Self {
            resources,
            frontend,
            backend,
            pixel_snap: options.pixel_snap,
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

    /// A driver for the window `token`, presenting by `strategy`, over a render stream of its own.
    pub(super) fn open(&self, token: WindowToken, strategy: PresentStrategy) -> WindowDriver {
        let stream = self
            .backend
            .open_stream(self.resources.render_owners().reserve());
        WindowDriver::new(
            token,
            &self.resources,
            DriverConfig {
                strategy,
                pixel_snap: self.pixel_snap,
            },
            stream,
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
        self.backend.prepare_format(spec.format);
        driver.deliver_screenshot(&self.backend);
        driver.forget_evicted_panes(&mut self.backend);
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

    /// Close a window: the backend drops what it keeps for its render owner, `GpuView` targets
    /// that owner-scoped eviction would otherwise hold until shutdown, its render stream drops
    /// with it, and the driver's `Drop` retires its directory entry.
    #[cfg_attr(
        not(feature = "winit"),
        expect(
            dead_code,
            reason = "only the windowed host closes a window while it runs"
        )
    )]
    pub(super) fn close(&mut self, driver: WindowDriver) {
        self.backend.retire_render_owner(driver.render_owner());
        drop(driver);
    }
}

#[cfg(test)]
mod tests {
    use crate::gpu::surface::owned_target::OwnedTarget;
    use crate::gpu::surface::render_target::TargetFormat;
    use crate::gpu::test_gpu::headless_test_gpu;
    use crate::host::core::HostCore;
    use crate::host::host_options::HostOptions;
    use crate::host::window_driver::{CpuFrame, FrameSpec, PresentStrategy};
    use crate::internals::record_app::RecordApp;
    use crate::text::font_scope::FontScope;
    use crate::text::shaper::TextShaper;
    use crate::window::window_token::WindowToken;
    use glam::UVec2;
    use std::time::Duration;

    /// Two windows on one host render as two owners and read their own GPU samples: a submit
    /// publishes into its own window's handle, so a window that has not drawn reads nothing. The
    /// record of a frame builds its format's pipelines, ahead of its submit.
    #[test]
    fn each_window_reads_its_own_gpu_sample() {
        let gpu = headless_test_gpu();
        let options = HostOptions {
            fonts: FontScope::Bundled,
            shaper: None,
            collect_gpu_stats: false,
            pixel_snap: true,
            #[cfg(feature = "system-clipboard")]
            system_clipboard: false,
        };
        let mut core = HostCore::new(gpu.handles(), &options, TextShaper::test_mono());
        let mut drawn = core.open(WindowToken(0), PresentStrategy::BackbufferCopy);
        let idle = core.open(WindowToken(1), PresentStrategy::BackbufferCopy);
        assert_ne!(drawn.render_owner(), idle.render_owner());
        let size = UVec2::splat(16);
        let target = OwnedTarget::window(
            &gpu.handles(),
            "palantir.test.core",
            size,
            TargetFormat::RGBA8_SRGB,
        );
        let spec = FrameSpec {
            physical: size,
            format: TargetFormat::RGBA8_SRGB,
            gives_copy: target.as_target().gives_copy(),
            system_scale: 1.0,
            refresh_millihertz: None,
            now: Duration::ZERO,
        };
        let CpuFrame { mode, .. } = core.record(&mut drawn, spec, &mut RecordApp::new(|_| {}));
        assert!(
            core.backend.has_format_pipelines(spec.format),
            "the record built the target format's pipelines, so the submit compiles none"
        );
        core.submit(&mut drawn, target.as_target(), mode);
        assert!(drawn.ui.gpu_pass_stats().last_main_pass_cpu().is_some());
        assert_eq!(idle.ui.gpu_pass_stats().last_main_pass_cpu(), None);
    }
}
