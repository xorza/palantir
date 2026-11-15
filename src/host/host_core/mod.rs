//! [`HostCore`]: the composition root of a host that paints: the [`HostCpu`] and the
//! [`HostDevice`], built together to share registries and one texture-dimension cap. It does not
//! own the [`WindowFrames`]s; each host pairs its own.

use crate::effect::effect_registry::EffectRegistry;
use crate::gpu::device::gpu::Gpu;
use crate::gpu::resource::wgpu_image_store::WgpuImageStore;
use crate::host::host_cpu::HostCpu;
use crate::host::host_device::{BackendSpec, HostDevice};
use crate::host::host_options::HostOptions;
use crate::image_registry::ImageRegistry;
use crate::text::shaper::TextShaper;
use crate::ui::resources::UiResources;
use crate::ui::window_frames::WindowFrames;
use crate::ui::window_frames::present::PresentStrategy;
use crate::window::window_token::WindowToken;
use std::rc::Rc;

/// A host's two halves, each its own type, so a host that always paints holds a device by
/// construction and a harness that paints nothing holds the [`HostCpu`] alone.
#[derive(Debug)]
pub(super) struct HostCore {
    pub(super) cpu: HostCpu,
    pub(super) device: HostDevice,
}

impl HostCore {
    /// The CPU half over `shaper`, then the device half on `gpu`, as
    /// [`HostDevice::attach`] builds it again.
    pub(super) fn new(gpu: Gpu, options: &HostOptions, shaper: TextShaper) -> Self {
        let image_store = Rc::new(WgpuImageStore::default());
        let mut cpu = HostCpu::over(
            UiResources::new(
                shaper,
                options.clipboard(),
                ImageRegistry::new(Rc::clone(&image_store)),
                EffectRegistry::default(),
            ),
            options,
        );
        let spec = BackendSpec {
            image_store,
            timing: options.timing,
        };
        let device = HostDevice::new(spec, gpu, &mut cpu);
        Self { cpu, device }
    }

    /// The frames of the window `token`, presenting by `strategy`, over a render stream of its own.
    pub(super) fn open(&mut self, token: WindowToken, strategy: PresentStrategy) -> WindowFrames {
        let mut frames = self.cpu.open(token, strategy);
        self.device.attach_window(&mut frames);
        frames
    }
}

#[cfg(test)]
mod tests;
