//! [`HostCpu`]: the half of a host's composition root that needs no device.

use crate::effect::effect_registry::EffectRegistry;
use crate::gpu::render_owner_id::RenderOwners;
use crate::host::host_options::HostOptions;
use crate::image_registry::ImageRegistry;
use crate::image_registry::texture_limit::TextureLimit;
use crate::renderer::frontend::Frontend;
use crate::text::shaper::TextShaper;
use crate::ui::resources::UiResources;
use crate::ui::window_frames::WindowFrames;
use crate::ui::window_frames::present::{PresentStrategy, WindowSpec};
use crate::window::window_token::WindowToken;

/// The [`UiResources`] every window of a host shares, and the one CPU [`Frontend`] each frame
/// composes through. A host that paints pairs it with a
/// [`HostDevice`](crate::host::host_device::HostDevice) in a
/// [`HostCore`](crate::host::host_core::HostCore); a harness below
/// [`Depth::Gpu`](crate::Depth::Gpu) holds it alone. Refused: one core with an optional device
/// half, which every host that always paints unwrapped, and which a harness stored beside its own
/// fact of whether it paints.
#[derive(Debug)]
pub(super) struct HostCpu {
    pub(super) resources: UiResources,
    pub(super) frontend: Frontend,
    /// Where each window [`Self::open`] opens takes its render owner.
    render_owners: RenderOwners,
}

impl HostCpu {
    /// The core of a host that paints nothing, over `shaper`. Its images go nowhere and its
    /// recorders read no texture ceiling, as no device holds them.
    pub(super) fn new(options: &HostOptions, shaper: TextShaper) -> Self {
        Self::over(
            UiResources::new(
                shaper,
                options.clipboard(),
                ImageRegistry::default(),
                EffectRegistry::default(),
            ),
            options,
        )
    }

    /// The core over `resources`, which other cores may share.
    pub(super) fn over(resources: UiResources, options: &HostOptions) -> Self {
        let frontend = Frontend::new();
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

    /// The frames of the window `token`, presenting by `strategy`, with no render stream: a host
    /// that paints gives it one ([`HostCore::open`](crate::host::host_core::HostCore::open)).
    pub(super) fn open(&mut self, token: WindowToken, strategy: PresentStrategy) -> WindowFrames {
        let spec = WindowSpec {
            token,
            owner: self.render_owners.reserve(),
            strategy,
        };
        WindowFrames::new(spec, &self.resources)
    }
}
