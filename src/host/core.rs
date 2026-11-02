//! `HostCore`: the composition root both hosts build on: [`UiResources`], the one CPU [`Frontend`] and
//! the one [`WgpuBackend`] every window renders through, built together to share registries and one
//! texture-dimension cap. It does not own the [`WindowDriver`]s; each host pairs its own.
//!
//! The CPU half is built once. The device half is built by [`BackendSpec::open`], which startup
//! ([`HostCore::new`]) and a recovery from a lost device ([`HostCore::attach`]) both run, so no
//! code tells the two apart. A core built by [`HostCore::cpu`] has no device half: it records and
//! composes, and paints nothing.

use crate::gpu::device::backend_resources::BackendResources;
use crate::gpu::device::gpu::Gpu;
use crate::gpu::device::gpu_wait::GpuWait;
use crate::gpu::resource::wgpu_image_store::WgpuImageStore;
use crate::gpu::surface::render_target::RenderTarget;
use crate::gpu::wgpu_backend::{FrameTiming, WgpuBackend};
use crate::host::host_options::HostOptions;
use crate::host::window_driver::{
    CpuFrame, DriverConfig, FrameDepth, FrameSpec, PresentPath, PresentStrategy, WindowDriver,
};
use crate::image_registry::ImageRegistry;
use crate::renderer::frontend::Frontend;
use crate::renderer::texture_limit::TextureLimit;
use crate::text::shaper::TextShaper;
use crate::ui::app::App;
use crate::ui::resources::UiResources;
use crate::window::window_token::WindowToken;
use std::rc::Rc;

#[derive(Debug)]
pub(super) struct HostCore {
    pub(super) resources: UiResources,
    pub(super) frontend: Frontend,
    /// `None` on a core built by [`Self::cpu`].
    device: Option<DeviceHalf>,
    pixel_snap: bool,
}

/// What a core holds on its device, and what it builds them from again on the next.
#[derive(Debug)]
struct DeviceHalf {
    spec: BackendSpec,
    backend: WgpuBackend,
}

/// What the device half is built from on each device the host attaches.
#[derive(Debug)]
struct BackendSpec {
    /// Built with no device, so the registry every recorder clones holds it from the start.
    image_store: Rc<WgpuImageStore>,
    timing: FrameTiming,
}

impl HostCore {
    /// The CPU half over `shaper`, then the device half on `gpu`, as [`Self::attach`] builds it
    /// again.
    /// `options.pixel_snap` is the host's, inherited by every driver.
    pub(super) fn new(gpu: Gpu, options: &HostOptions, shaper: TextShaper) -> Self {
        let image_store = Rc::new(WgpuImageStore::default());
        let mut core = Self::over(
            UiResources::new(
                shaper,
                options.clipboard(),
                ImageRegistry::new(Rc::clone(&image_store)),
            ),
            options,
        );
        let spec = BackendSpec {
            image_store,
            timing: if options.collect_gpu_stats {
                FrameTiming::On
            } else {
                FrameTiming::Off
            },
        };
        let backend = spec.open(gpu, &core.resources);
        core.device = Some(DeviceHalf { spec, backend });
        core
    }

    /// The CPU half alone, over `shaper`: frames record and compose, and paint nothing. Its
    /// images go nowhere and its recorders read no texture ceiling, as no device holds them, so
    /// it never [attaches](Self::attach) one.
    pub(super) fn cpu(options: &HostOptions, shaper: TextShaper) -> Self {
        Self::over(
            UiResources::new(shaper, options.clipboard(), ImageRegistry::default()),
            options,
        )
    }

    /// The CPU half over `resources`, which other cores may share.
    pub(super) fn over(resources: UiResources, options: &HostOptions) -> Self {
        let frontend = Frontend::new(
            Rc::clone(resources.device()),
            resources.gradient_atlas().clone(),
        );
        Self {
            resources,
            frontend,
            device: None,
            pixel_snap: options.pixel_snap,
        }
    }

    /// Draw with `gpu` from now on, in place of a device that was lost: every device-bound object
    /// is built again on it, as at startup, and each of `drivers`, every window of the host, gets
    /// its stream on it.
    pub(super) fn attach<'a>(
        &mut self,
        gpu: Gpu,
        drivers: impl IntoIterator<Item = &'a mut WindowDriver>,
    ) {
        let half = self.device.as_mut().expect(NO_DEVICE);
        half.backend = half.spec.open(gpu, &self.resources);
        for driver in drivers {
            self.attach_window(driver);
        }
    }

    /// Block until the device finished every frame submitted so far and the copies of
    /// `driver`'s screenshots are back, also when other threads poll the device.
    pub(super) fn wait_for_gpu(&self, driver: &WindowDriver) -> GpuWait {
        self.gpu().wait_until(&|| driver.screenshots_landed())
    }

    /// Whether the core has a device half to paint with.
    pub(super) const fn paints(&self) -> bool {
        self.device.is_some()
    }

    /// The backend every window of a painting host draws through.
    pub(super) const fn backend(&self) -> &WgpuBackend {
        &self.device.as_ref().expect(NO_DEVICE).backend
    }

    pub(super) const fn backend_mut(&mut self) -> &mut WgpuBackend {
        &mut self.device.as_mut().expect(NO_DEVICE).backend
    }

    /// The device and queue every window of this host presents through.
    pub(super) const fn gpu(&self) -> &Gpu {
        self.backend().gpu()
    }

    /// A driver for the window `token`, presenting by `strategy`, over a render stream of its own.
    pub(super) fn open(&self, token: WindowToken, strategy: PresentStrategy) -> WindowDriver {
        let mut driver = WindowDriver::new(
            token,
            &self.resources,
            DriverConfig {
                strategy,
                pixel_snap: self.pixel_snap,
            },
        );
        self.attach_window(&mut driver);
        driver
    }

    /// Give `driver` its render stream on the device attached now; a deviceless core has none to
    /// give.
    fn attach_window(&self, driver: &mut WindowDriver) {
        if let Some(half) = &self.device {
            driver.attach(half.backend.open_stream(driver.render_owner()));
        }
    }

    /// Run the CPU half of the frame `spec` describes to `depth`: note its target, hand over a
    /// screenshot the device finished, and record `app` under the display minted from `spec`.
    pub(super) fn record<T: App>(
        &mut self,
        driver: &mut WindowDriver,
        spec: FrameSpec,
        app: &mut T,
        depth: FrameDepth,
    ) -> CpuFrame {
        driver.note_target(&spec);
        if let Some(half) = &mut self.device {
            half.backend.prepare_format(spec.format);
            driver.deliver_screenshot(&half.backend);
            driver.forget_evicted_panes(&mut half.backend);
        }
        let stamp = driver.stamp(&spec);
        driver.cpu_frame(&mut self.frontend, stamp, app, depth)
    }

    /// The GPU half: draw the recorded frame into `target` by `mode`.
    pub(super) fn submit(
        &mut self,
        driver: &mut WindowDriver,
        target: RenderTarget<'_>,
        mode: PresentPath,
    ) {
        let backend = &mut self.device.as_mut().expect(NO_DEVICE).backend;
        driver.render_to_texture(&self.frontend.buffer, backend, target, mode);
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
        self.backend_mut()
            .retire_render_owner(driver.render_owner());
        drop(driver);
    }
}

const NO_DEVICE: &str = "a host that paints builds its core with a device";

impl BackendSpec {
    /// The device half on `gpu`, at startup and after a loss alike: the image store holds
    /// textures on it, the CPU resources take its limit, and the backend is built over both. On a
    /// first attach the store and the gradient atlas hold nothing yet, so the same steps do
    /// nothing extra.
    fn open(&self, gpu: Gpu, resources: &UiResources) -> WgpuBackend {
        self.image_store.attach(&gpu);
        resources.attach_device(TextureLimit::from_device(gpu.max_texture_dim()));
        WgpuBackend::new(
            gpu,
            BackendResources {
                text: resources.text(),
                image_store: &self.image_store,
                icons: resources.icons(),
                gradient_atlas: resources.gradient_atlas(),
            },
            self.timing,
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::gpu::device::gpu::Gpu;
    use crate::gpu::surface::owned_target::OwnedTarget;
    use crate::gpu::surface::render_target::TargetFormat;
    use crate::gpu::test_gpu::headless_test_gpu;
    use crate::gpu::test_gpu::isolated_headless_test_gpu;
    use crate::host::core::HostCore;
    use crate::host::host_options::HostOptions;
    use crate::host::window_driver::WindowDriver;
    use crate::host::window_driver::{CpuFrame, FrameDepth, FrameSpec, PresentStrategy};
    use crate::primitives::layout::sizing::Sizing;
    use crate::primitives::paint::chrome::Chrome;
    use crate::primitives::paint::color::RgbaF32;
    use crate::text::font_scope::FontScope;
    use crate::text::shaper::TextShaper;
    use crate::ui::Ui;
    use crate::widget_core::configure::Configure;
    use crate::widgets::block::Block;
    use crate::window::window_token::WindowToken;
    use glam::UVec2;
    use std::cell::Cell;
    use std::time::Duration;

    /// What every test core is built with: bundled fonts, no GPU timing.
    const OPTIONS: HostOptions = HostOptions {
        fonts: FontScope::Bundled,
        shaper: None,
        collect_gpu_stats: false,
        pixel_snap: true,
        #[cfg(feature = "system-clipboard")]
        system_clipboard: false,
    };

    /// Two windows on one host render as two owners and read their own GPU samples: a submit
    /// publishes into its own window's handle, so a window that has not drawn reads nothing. The
    /// record of a frame builds its format's pipelines, ahead of its submit.
    #[test]
    fn each_window_reads_its_own_gpu_sample() {
        let gpu = headless_test_gpu();
        let mut core = HostCore::new(gpu.handles(), &OPTIONS, TextShaper::test_mono());
        let mut drawn = core.open(WindowToken(0), PresentStrategy::BackbufferCopy);
        let idle = core.open(WindowToken(1), PresentStrategy::BackbufferCopy);
        assert_ne!(drawn.render_owner(), idle.render_owner());
        let size = UVec2::splat(16);
        let target =
            OwnedTarget::window(&gpu, "palantir.test.core", size, TargetFormat::RGBA8_SRGB);
        let spec = FrameSpec {
            physical: size,
            format: TargetFormat::RGBA8_SRGB,
            gives_copy: target.as_target().gives_copy(),
            system_scale: 1.0,
            refresh_millihertz: None,
            now: Duration::ZERO,
        };
        let CpuFrame { mode, .. } =
            core.record(&mut drawn, spec, &mut |_: &mut Ui| {}, FrameDepth::Paint);
        assert!(
            core.backend().has_format_pipelines(spec.format),
            "the record built the target format's pipelines, so the submit compiles none"
        );
        core.submit(&mut drawn, target.as_target(), mode);
        assert!(drawn.ui.gpu_pass_stats().last_main_pass_cpu().is_some());
        assert_eq!(idle.ui.gpu_pass_stats().last_main_pass_cpu(), None);
    }

    /// A recovery reaches every window of a host, as the windowed host attaches all of them: after
    /// a loss, each of two windows paints its own fill on the new device in its first frame, and
    /// sees the device restored in that frame alone.
    #[test]
    fn attaching_a_new_device_reaches_every_window() {
        const FILLS: [u32; 2] = [0xd23f7a, 0x2f6fd0];
        let size = UVec2::splat(8);
        let spec_on = |target: &OwnedTarget| FrameSpec {
            physical: size,
            format: TargetFormat::RGBA8_SRGB,
            gives_copy: target.as_target().gives_copy(),
            system_scale: 1.0,
            refresh_millihertz: None,
            now: Duration::ZERO,
        };
        let paint = |core: &mut HostCore, driver: &mut WindowDriver, gpu: &Gpu, fill: u32| {
            let target = OwnedTarget::window(
                gpu,
                "palantir.test.core.recover",
                size,
                TargetFormat::RGBA8_SRGB,
            );
            let restored = Cell::new(false);
            let mut app = |ui: &mut Ui| {
                restored.set(ui.device_restored());
                Block::new()
                    .id_salt("fill")
                    .size(Sizing::FILL)
                    .chrome(Chrome::fill(RgbaF32::hex(fill)))
                    .show(ui);
            };
            let CpuFrame { mode, .. } =
                core.record(driver, spec_on(&target), &mut app, FrameDepth::Paint);
            core.submit(driver, target.as_target(), mode);
            let [r, g, b, _] = target.read_back().get_pixel(4, 4).0;
            (restored.get(), u32::from_be_bytes([0, r, g, b]))
        };

        let first = isolated_headless_test_gpu();
        let mut core = HostCore::new(first.handles(), &OPTIONS, TextShaper::test_mono());
        let mut windows = [
            core.open(WindowToken(0), PresentStrategy::BackbufferCopy),
            core.open(WindowToken(1), PresentStrategy::BackbufferCopy),
        ];
        for (window, fill) in windows.iter_mut().zip(FILLS) {
            assert_eq!(paint(&mut core, window, &first, fill), (false, fill));
        }
        first.lose();

        let second = isolated_headless_test_gpu();
        core.attach(second.handles(), windows.iter_mut());
        for (window, fill) in windows.iter_mut().zip(FILLS) {
            assert_eq!(paint(&mut core, window, &second, fill), (true, fill));
            assert_eq!(paint(&mut core, window, &second, fill), (false, fill));
        }
    }
}
