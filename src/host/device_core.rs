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
use crate::image_registry::texture_limit::TextureLimit;
use crate::ui::app::App;
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
    /// The backend `spec` builds on `gpu` over `cpu`, as [`Self::attach`] builds it again.
    pub(super) fn new(spec: BackendSpec, gpu: Gpu, cpu: &mut CpuCore) -> Self {
        let backend = spec.open(gpu, cpu);
        Self { spec, backend }
    }

    /// Draw with `gpu` from now on, in place of a device that was lost: every device-bound object
    /// is built again on it over `cpu`, as at startup, and each of `drivers`, every window of the
    /// host, gets its stream on it.
    pub(super) fn attach<'a>(
        &mut self,
        gpu: Gpu,
        cpu: &mut CpuCore,
        drivers: impl IntoIterator<Item = &'a mut WindowDriver>,
    ) {
        self.backend = self.spec.open(gpu, cpu);
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
        let placements = self.backend.cutout_placements_mut();
        cpu.record(driver, spec, app, FrameDepth::Paint(placements))
    }

    /// The GPU half: draw the frame recorded through `cpu` into `target` by `mode`.
    pub(super) fn submit(
        &mut self,
        cpu: &mut CpuCore,
        driver: &mut WindowDriver,
        target: RenderTarget<'_>,
        mode: PresentPath,
    ) {
        driver.render_to_texture(&mut cpu.frontend, &mut self.backend, target, mode);
    }

    /// Close a window: the backend drops what it keeps for its render owner, `GpuView` targets
    /// that owner-scoped eviction would otherwise hold until shutdown and the cutout cells its
    /// plan holds, its render stream drops with it, and the driver's `Drop` retires its directory
    /// entry.
    #[cfg_attr(
        all(not(feature = "winit"), not(test)),
        expect(
            dead_code,
            reason = "only the windowed host closes a window while it runs, and the tests"
        )
    )]
    pub(super) fn close(&mut self, mut driver: WindowDriver) {
        driver.retire(&mut self.backend);
        drop(driver);
    }
}

impl BackendSpec {
    /// The backend on `gpu`, at startup and after a loss alike: the group-0 texture binding is
    /// made with the device, the image store holds textures on it, the CPU core takes its limit,
    /// and the backend is built over it. Refused: the binding made by the image store,
    /// which pipelines took from it only because the store attached first. On a first attach the
    /// store and the gradient atlas hold nothing yet, so the same steps do nothing extra.
    ///
    /// # Panics
    ///
    /// If `gpu` cannot run Palantir's pipelines, see
    /// [`DeviceRequirements`](crate::DeviceRequirements): a caller's mistake every painting host
    /// meets here, at startup and at a recovery alike.
    fn open(&self, gpu: Gpu, cpu: &mut CpuCore) -> WgpuBackend {
        if let Err(unmet) = gpu.requirements_met() {
            panic!("a host's device cannot run Palantir: {unmet}");
        }
        let texture_binding = TextureBinding::new(&gpu);
        self.image_store.attach(&gpu, texture_binding.clone());
        cpu.attach_device(TextureLimit::from_device(gpu.max_texture_dim()));
        WgpuBackend::new(
            gpu,
            BackendResources {
                text: cpu.resources.text(),
                image_store: &self.image_store,
                icons: cpu.resources.icons(),
                gradient_rows: cpu.frontend.gradient_rows(),
                texture_binding,
            },
            self.timing,
        )
    }
}
