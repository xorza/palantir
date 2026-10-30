//! The facts of the device a host draws with, which every recorder and the frontend read.

use crate::renderer::texture_limit::TextureLimit;
use std::cell::Cell;

/// The device's texture ceiling, and how many devices the host attached: one shared value that
/// every `UiResources` clone and the frontend hold, so a host that attaches a new device after a
/// loss updates them all at once.
#[derive(Debug, Default)]
pub(crate) struct SharedDevice {
    texture_limit: Cell<TextureLimit>,
    epoch: Cell<u32>,
}

impl SharedDevice {
    /// The ceiling for any texture palantir allocates or accepts on the device attached now.
    pub(crate) const fn texture_limit(&self) -> TextureLimit {
        self.texture_limit.get()
    }

    /// How many devices the host attached: 0 before the first, one more for each after a loss.
    pub(crate) const fn epoch(&self) -> u32 {
        self.epoch.get()
    }

    /// Note a device of `texture_limit` attached, a new epoch.
    pub(crate) fn attach(&self, texture_limit: TextureLimit) {
        self.texture_limit.set(texture_limit);
        self.epoch.set(self.epoch.get() + 1);
    }
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::renderer::shared_device::SharedDevice;
    use crate::renderer::texture_limit::TextureLimit;

    impl SharedDevice {
        /// A value with `texture_limit` already attached, as a host's first attach leaves it.
        pub(crate) fn attached(texture_limit: TextureLimit) -> Self {
            let device = Self::default();
            device.attach(texture_limit);
            device
        }
    }
}
