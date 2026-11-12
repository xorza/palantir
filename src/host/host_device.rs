//! [`HostDevice`]: the half of a host's composition root that lives on its device.
//!
//! It is built by [`BackendSpec::open`], which startup
//! ([`HostCore::new`](crate::host::host_core::HostCore::new)) and a recovery from a lost device
//! ([`HostDevice::attach`]) both run, so no code tells the two apart.

use crate::gpu::device::gpu::Gpu;
use crate::gpu::device::gpu_wait::GpuWait;
use crate::gpu::resource::texture_binding::TextureBinding;
use crate::gpu::resource::wgpu_effect_store::WgpuEffectStore;
use crate::gpu::resource::wgpu_image_store::WgpuImageStore;
use crate::gpu::surface::render_target::RenderTarget;
use crate::gpu::wgpu_backend::backend_resources::BackendResources;
use crate::gpu::wgpu_backend::{FrameTiming, WgpuBackend};
use crate::host::host_cpu::HostCpu;
use crate::image_registry::texture_limit::TextureLimit;
use crate::ui::app::App;
use crate::ui::window_frames::WindowFrames;
use crate::ui::window_frames::present::{BuildCtx, BuiltFrame, CpuFrame, FrameSpec};
use std::rc::Rc;

/// The one [`WgpuBackend`] every window of a host renders through.
#[derive(Debug)]
pub(super) struct HostDevice {
    backend: WgpuBackend,
}

/// What the backend is built from on each device the host attaches; the old backend holds both
/// for the next.
#[derive(Debug)]
pub(super) struct BackendSpec {
    /// Built with no device, so the registry every recorder clones holds it from the start.
    pub(super) image_store: Rc<WgpuImageStore>,
    /// Built with no device, so the registry every recorder clones loads through it from the start.
    pub(super) effect_store: Rc<WgpuEffectStore>,
    pub(super) timing: FrameTiming,
}

impl HostDevice {
    /// The backend `spec` builds on `gpu` over `cpu`, as [`Self::attach`] builds it again.
    pub(super) fn new(spec: &BackendSpec, gpu: Gpu, cpu: &mut HostCpu) -> Self {
        Self {
            backend: spec.open(gpu, cpu),
        }
    }

    /// Draw with `gpu` from now on, in place of a device that was lost: every device-bound object
    /// is built again on it over `cpu`, as at startup, and each of `windows`, every window of the
    /// host, gets its stream on it.
    pub(super) fn attach<'a>(
        &mut self,
        gpu: Gpu,
        cpu: &mut HostCpu,
        windows: impl IntoIterator<Item = &'a mut WindowFrames>,
    ) {
        let spec = BackendSpec {
            image_store: Rc::clone(self.backend.image_store()),
            effect_store: Rc::clone(self.backend.effect_store()),
            timing: self.backend.frame_timing(),
        };
        self.backend = spec.open(gpu, cpu);
        for frames in windows {
            self.attach_window(frames);
        }
    }

    /// Give `frames` its render stream on the device attached now.
    pub(super) fn attach_window(&self, frames: &mut WindowFrames) {
        frames.attach(self.backend.open_stream(frames.render_owner()));
    }

    /// Block until the device finished every frame submitted so far and the copies of
    /// `frames`' screenshots are back, also when other threads poll the device.
    pub(super) fn wait_for_gpu(&self, frames: &WindowFrames) -> GpuWait {
        self.gpu().wait_until(&|| frames.screenshots_landed())
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

    /// The CPU half of a frame that paints, the one sequence every painting host runs: build the
    /// pipelines of its target's format, hand over a screenshot the device finished, drop what
    /// evicted backdrops held, then record `app` and build the draw list through `cpu`'s frontend.
    pub(super) fn frame<'c, T: App>(
        &mut self,
        cpu: &'c mut HostCpu,
        frames: &mut WindowFrames,
        spec: FrameSpec,
        app: &mut T,
    ) -> CpuFrame<'c> {
        self.backend.prepare_format(spec.target.format);
        frames.deliver_screenshot(&self.backend);
        frames.forget_evicted_backdrops(&mut self.backend);
        let report = frames.record(spec, app);
        frames.build_paint(
            BuildCtx {
                frontend: &mut cpu.frontend,
                placing: self.backend.cutout_placing(),
            },
            report,
        )
    }

    /// Whether `built` has a target to reach: it draws something, on a device that lives. A lost
    /// device paints nothing, and its recovery wakes the window again.
    pub(super) fn will_submit(&self, built: &BuiltFrame<'_>) -> bool {
        !matches!(built, BuiltFrame::Still) && !self.gpu().is_lost()
    }

    /// The GPU half: draw `built` into `target`.
    pub(super) fn submit(
        &mut self,
        frames: &mut WindowFrames,
        target: RenderTarget<'_>,
        built: BuiltFrame<'_>,
    ) {
        frames.submit(&mut self.backend, target, built);
    }

    /// Close a window: the backend drops what it keeps for its render owner, `GpuView` targets
    /// that owner-scoped eviction would otherwise hold until shutdown and the pins on the cutout
    /// tables its plan holds, its render stream drops with it, and its
    /// [`WindowFrames`]'s `Drop` retires its directory entry.
    #[cfg_attr(
        all(not(feature = "winit"), not(test)),
        expect(
            dead_code,
            reason = "only the windowed host closes a window while it runs, and the tests"
        )
    )]
    pub(super) fn close(&mut self, mut frames: WindowFrames) {
        frames.retire(&mut self.backend);
        drop(frames);
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
    fn open(&self, gpu: Gpu, cpu: &mut HostCpu) -> WgpuBackend {
        if let Err(unmet) = gpu.requirements_met() {
            panic!("a host's device cannot run Palantir: {unmet}");
        }
        let texture_binding = TextureBinding::new(&gpu);
        self.image_store.attach(&gpu, texture_binding.clone());
        self.effect_store.attach(&gpu, &texture_binding);
        cpu.attach_device(TextureLimit::from_device(gpu.max_texture_dim()));
        WgpuBackend::new(
            gpu,
            BackendResources {
                text: cpu.resources.text(),
                image_store: &self.image_store,
                effect_store: &self.effect_store,
                icons: cpu.resources.icons(),
                gradient_rows: cpu.frontend.gradient_rows(),
                texture_binding,
            },
            self.timing,
        )
    }
}
