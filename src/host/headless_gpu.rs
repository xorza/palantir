//! A device to render on when there is no window to get one from.

use pollster::FutureExt;

use crate::host::device_requirements::DeviceRequirements;
use crate::host::error::HeadlessGpuError;

/// An adapter and device requested without a surface, meeting
/// [`DeviceRequirements`].
///
/// [`OffscreenHost`](crate::OffscreenHost) renders to a texture and so needs
/// no window, but it still needs a device, and the features that device must
/// carry are Palantir's business rather than the caller's. This is the short
/// way to a usable one: screenshots, thumbnails, server-side compositing, and
/// tests that want a frame without a compositor in the loop.
///
/// An application that already owns a device should keep it and go through
/// [`DeviceRequirements::negotiate`] instead — Palantir's needs fold into its
/// own request rather than replacing it.
#[derive(Debug)]
pub struct HeadlessGpu {
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl HeadlessGpu {
    /// Request one, taking the `optional` features that turn out to be
    /// available.
    ///
    /// Blocks. Adapter and device requests are futures that resolve against
    /// the driver rather than an async runtime, so there is nothing here for a
    /// caller's executor to interleave with.
    pub fn new(
        power_preference: wgpu::PowerPreference,
        optional: wgpu::Features,
    ) -> Result<Self, HeadlessGpuError> {
        if wgpu::Instance::enabled_backend_features().is_empty() {
            return Err(HeadlessGpuError::NoBackend);
        }
        // `_from_env` so `WGPU_BACKEND` reaches headless callers too. A bug
        // that reproduces on one backend is otherwise out of reach: the pick
        // would be whatever the adapter sort lands on, whatever was asked for.
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference,
                compatible_surface: None,
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            })
            .block_on()
            .map_err(|source| HeadlessGpuError::RequestAdapter { source })?;

        let requirements = DeviceRequirements::negotiate(&adapter, optional)
            .map_err(|source| HeadlessGpuError::Requirements { source })?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("palantir.headless.device"),
                required_features: requirements.features,
                required_limits: requirements.limits,
                experimental_features: wgpu::ExperimentalFeatures::default(),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::Off,
            })
            .block_on()
            .map_err(|source| HeadlessGpuError::RequestDevice { source })?;

        Ok(Self {
            adapter,
            device,
            queue,
        })
    }
}
