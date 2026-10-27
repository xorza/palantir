//! The device and queue Palantir draws with.

use crate::gpu::device::device_requirements::DeviceRequirements;
use crate::gpu::error::UnmetRequirements;
use std::num::NonZeroU32;

/// A graphics device and its queue.
///
/// An application that opened its own wraps the pair here for
/// [`OffscreenHost::builder`](crate::OffscreenHost::builder); a host that
/// used [`RequestedGpu`](crate::RequestedGpu) finds it in
/// [`gpu`](crate::RequestedGpu::gpu). Cloning
/// clones the handles, not the device.
#[derive(Clone, Debug)]
pub struct Gpu {
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
}

impl Gpu {
    /// Draw through a device the caller owns. It must have been requested with
    /// [`DeviceRequirements`]; the host builders check that.
    pub const fn new(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        Self { device, queue }
    }

    pub(crate) fn requirements_met(&self) -> Result<(), UnmetRequirements> {
        DeviceRequirements::met_by(&self.device)
    }

    /// The device's `max_texture_dimension_2d`: the ceiling on registered
    /// images, `GpuView` targets, and the glyph and gradient atlases.
    ///
    /// A zero limit panics rather than joining [`DeviceRequirements::met_by`]'s
    /// `Result`: no adapter reports one.
    pub(crate) fn max_texture_dim(&self) -> NonZeroU32 {
        NonZeroU32::new(self.device.limits().max_texture_dimension_2d)
            .expect("device texture dimension limit is zero")
    }

    /// Block until the device has finished every submission, running the map callbacks of their
    /// copies.
    ///
    /// # Panics
    ///
    /// When the device is lost.
    pub(crate) fn wait(&self) {
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .expect("the device finishes its submissions");
    }
}
