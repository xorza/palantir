//! [`DeviceCore`]: the half of a host's composition root that lives on its device.
//!
//! It is built by [`BackendSpec::open`], which startup
//! ([`HostCore::new`](crate::host::core::HostCore::new)) and a recovery from a lost device
//! ([`DeviceCore::attach`]) both run, so no code tells the two apart.

use crate::gpu::device::gpu::Gpu;
use crate::gpu::device::gpu_wait::GpuWait;
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::gpu::resource::wgpu_image_store::WgpuImageStore;
use crate::gpu::surface::render_target::RenderTarget;
use crate::gpu::wgpu_backend::backend_resources::BackendResources;
use crate::gpu::wgpu_backend::{FrameTiming, WgpuBackend};
use crate::host::cpu_core::CpuCore;
use crate::host::window_driver::{CpuFrame, FrameDepth, FrameSpec, PresentPath, WindowDriver};
use crate::renderer::texture_limit::TextureLimit;
use crate::ui::app::App;
use crate::ui::resources::UiResources;
use std::rc::Rc;

/// The one [`WgpuBackend`] every window of a host renders through, and what it is built from
/// again on the next device.
#[derive(Debug)]
pub(super) struct DeviceCore {
    spec: BackendSpec,
    backend: WgpuBackend,
}

/// What the backend is built from on each device the host attaches.
#[derive(Debug)]
pub(super) struct BackendSpec {
    /// Built with no device, so the registry every recorder clones holds it from the start.
    pub(super) image_store: Rc<WgpuImageStore>,
    pub(super) timing: FrameTiming,
}

impl DeviceCore {
    /// The backend `spec` builds on `gpu` over `resources`, as [`Self::attach`] builds it again.
    pub(super) fn new(spec: BackendSpec, gpu: Gpu, resources: &UiResources) -> Self {
        let backend = spec.open(gpu, resources);
        Self { spec, backend }
    }

    /// Draw with `gpu` from now on, in place of a device that was lost: every device-bound object
    /// is built again on it over `resources`, as at startup, and each of `drivers`, every window
    /// of the host, gets its stream on it.
    pub(super) fn attach<'a>(
        &mut self,
        gpu: Gpu,
        resources: &UiResources,
        drivers: impl IntoIterator<Item = &'a mut WindowDriver>,
    ) {
        self.backend = self.spec.open(gpu, resources);
        for driver in drivers {
            self.attach_window(driver);
        }
    }

    /// Give `driver` its render stream on the device attached now.
    pub(super) fn attach_window(&self, driver: &mut WindowDriver) {
        driver.attach(self.backend.open_stream(driver.render_owner()));
    }

    /// Block until the device finished every frame submitted so far and the copies of
    /// `driver`'s screenshots are back, also when other threads poll the device.
    pub(super) fn wait_for_gpu(&self, driver: &WindowDriver) -> GpuWait {
        self.gpu().wait_until(&|| driver.screenshots_landed())
    }

    /// The backend every window of the host draws through, for a test or a bench that reads it.
    #[cfg(feature = "internals")]
    pub(super) const fn backend(&self) -> &WgpuBackend {
        &self.backend
    }

    #[cfg(feature = "internals")]
    pub(super) const fn backend_mut(&mut self) -> &mut WgpuBackend {
        &mut self.backend
    }

    /// The device and queue every window of the host presents through.
    pub(super) const fn gpu(&self) -> &Gpu {
        self.backend.gpu()
    }

    /// Run the CPU half of a frame that paints, on `cpu`: build the pipelines of its target's
    /// format, hand over a screenshot the device finished, drop what evicted panes held, then
    /// [record](CpuCore::record).
    pub(super) fn record<T: App>(
        &mut self,
        cpu: &mut CpuCore,
        driver: &mut WindowDriver,
        spec: FrameSpec,
        app: &mut T,
    ) -> CpuFrame {
        self.backend.prepare_format(spec.target.format);
        driver.deliver_screenshot(&self.backend);
        driver.forget_evicted_panes(&mut self.backend);
        cpu.record(driver, spec, app, FrameDepth::Paint)
    }

    /// The GPU half: draw the frame recorded through `cpu` into `target` by `mode`.
    pub(super) fn submit(
        &mut self,
        cpu: &CpuCore,
        driver: &mut WindowDriver,
        target: RenderTarget<'_>,
        mode: PresentPath,
    ) {
        driver.render_to_texture(&cpu.frontend.buffer, &mut self.backend, target, mode);
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

impl BackendSpec {
    /// The backend on `gpu`, at startup and after a loss alike: the group-0 texture binding is
    /// made with the device, the image store holds textures on it, the CPU resources take its
    /// limit, and the backend is built over them. Refused: the binding made by the image store,
    /// which pipelines took from it only because the store attached first. On a first attach the
    /// store and the gradient atlas hold nothing yet, so the same steps do nothing extra.
    ///
    /// # Panics
    ///
    /// If `gpu` cannot run Palantir's pipelines, see
    /// [`DeviceRequirements`](crate::DeviceRequirements): a caller's mistake every painting host
    /// meets here, at startup and at a recovery alike.
    fn open(&self, gpu: Gpu, resources: &UiResources) -> WgpuBackend {
        if let Err(unmet) = gpu.requirements_met() {
            panic!("a host's device cannot run Palantir: {unmet}");
        }
        let texture_binding = TextureBinding::new(&gpu);
        self.image_store.attach(&gpu, texture_binding.clone());
        resources.attach_device(TextureLimit::from_device(gpu.max_texture_dim()));
        WgpuBackend::new(
            gpu,
            BackendResources {
                text: resources.text(),
                image_store: &self.image_store,
                icons: resources.icons(),
                gradient_atlas: resources.gradient_atlas(),
                texture_binding,
            },
            self.timing,
        )
    }
}
