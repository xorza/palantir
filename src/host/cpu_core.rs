//! [`CpuCore`]: the half of a host's composition root that needs no device.

use crate::gpu::render_owner_id::RenderOwners;
use crate::host::host_options::HostOptions;
use crate::host::window_driver::{
    CpuFrame, DriverSpec, FrameDepth, FrameSpec, PresentStrategy, WindowDriver,
};
use crate::image_registry::ImageRegistry;
use crate::image_registry::texture_limit::TextureLimit;
use crate::renderer::frontend::Frontend;
use crate::text::shaper::TextShaper;
use crate::ui::app::App;
use crate::ui::resources::UiResources;
use crate::window::window_token::WindowToken;
use std::rc::Rc;

/// The [`UiResources`] every window of a host shares, and the one CPU [`Frontend`] each frame
/// composes through. A host that paints pairs it with a
/// [`DeviceCore`](crate::host::device_core::DeviceCore) in a
/// [`HostCore`](crate::host::core::HostCore); a harness below
/// [`Depth::Gpu`](crate::Depth::Gpu) holds it alone. Refused: one core with an optional device
/// half, which every host that always paints unwrapped, and which a harness stored beside its own
/// fact of whether it paints.
#[derive(Debug)]
pub(super) struct CpuCore {
    pub(super) resources: UiResources,
    pub(super) frontend: Frontend,
    /// Where each window [`Self::open`] opens takes its render owner.
    render_owners: RenderOwners,
}

impl CpuCore {
    /// The core of a host that paints nothing, over `shaper`. Its images go nowhere and its
    /// recorders read no texture ceiling, as no device holds them.
    pub(super) fn new(options: &HostOptions, shaper: TextShaper) -> Self {
        Self::over(
            UiResources::new(shaper, options.clipboard(), ImageRegistry::default()),
            options,
        )
    }

    /// The core over `resources`, which other cores may share.
    pub(super) fn over(resources: UiResources, options: &HostOptions) -> Self {
        let frontend = Frontend::new(Rc::clone(resources.device()));
        resources.settings().set_pixel_snap(options.pixel_snap);
        Self {
            resources,
            frontend,
            render_owners: RenderOwners::default(),
        }
    }

    /// The CPU side of a device attach, at startup and after a loss alike: the recorders note the
    /// device's `texture_limit`, and the frontend starts its gradient atlas over, as the device's
    /// texture holds none of its rows.
    pub(super) fn attach_device(&mut self, texture_limit: TextureLimit) {
        self.resources.attach_device(texture_limit);
        self.frontend.attach_device(texture_limit);
    }

    /// A driver for the window `token`, presenting by `strategy`, with no render stream: a host
    /// that paints gives it one ([`HostCore::open`](crate::host::core::HostCore::open)).
    pub(super) fn open(&mut self, token: WindowToken, strategy: PresentStrategy) -> WindowDriver {
        let spec = DriverSpec {
            token,
            owner: self.render_owners.reserve(),
            strategy,
        };
        WindowDriver::new(spec, &self.resources)
    }

    /// Run the CPU half of the frame `spec` describes to `depth`: note its target, and record
    /// `app` under the display minted from `spec`. A frame that paints runs through
    /// [`DeviceCore::record`](crate::host::device_core::DeviceCore::record), which hands over
    /// the device's part first.
    pub(super) fn record<T: App>(
        &mut self,
        driver: &mut WindowDriver,
        spec: FrameSpec,
        app: &mut T,
        depth: FrameDepth,
    ) -> CpuFrame {
        driver.note_target(&spec);
        let stamp = driver.stamp(&spec);
        driver.cpu_frame(&mut self.frontend, stamp, app, depth)
    }
}
