//! The device Palantir draws with, and the one place it asks a driver for one.

use std::num::NonZeroU32;

use pollster::FutureExt;

use crate::gpu::device::device_requirements::DeviceRequirements;
use crate::gpu::device::power_preference::PowerPreference;
use crate::gpu::error::{DriverError, GpuRequestError, UnmetRequirements};

/// A graphics device and its queue.
///
/// An application that opened its own wraps the pair here for
/// [`OffscreenHost::builder`](crate::OffscreenHost::builder); a host that
/// used [`RequestedGpu`] finds it in [`gpu`](RequestedGpu::gpu). Cloning
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
}

/// An adapter and the device opened on it, meeting [`DeviceRequirements`].
///
/// [`Self::headless`] is the short way to a device for screenshots,
/// thumbnails, server-side compositing and tests. An application that owns a
/// device should use [`DeviceRequirements::negotiate`] instead, so Palantir's
/// needs fold into its own request.
#[derive(Debug)]
pub struct RequestedGpu {
    /// The adapter that answered the request.
    pub adapter: wgpu::Adapter,
    /// The device it opened, and that device's queue.
    pub gpu: Gpu,
}

impl RequestedGpu {
    /// Open one with no surface attached, taking the `optional` features that
    /// are available. Blocks.
    /// caller's executor to interleave with.
    pub fn headless(
        power_preference: PowerPreference,
        optional: wgpu::Features,
    ) -> Result<Self, GpuRequestError> {
        let instance = GpuRequest::instance()?;
        GpuRequest {
            label: "palantir.headless.device",
            power_preference,
            optional,
            compatible_surface: None,
        }
        .open(&instance)
    }
}

/// What to ask a driver for, and the steps that ask it: backend check,
/// instance, adapter, requirements negotiation, device. Shared by both hosts
/// so they cannot drift; only the surface differs.
#[derive(Debug)]
pub(crate) struct GpuRequest<'a> {
    /// Names the device in a debugger and in wgpu's messages.
    pub(crate) label: &'static str,
    pub(crate) power_preference: PowerPreference,
    /// Taken when the adapter has them; see [`DeviceRequirements::negotiate`].
    pub(crate) optional: wgpu::Features,
    /// The surface the adapter must present to; `None` when headless.
    pub(crate) compatible_surface: Option<&'a wgpu::Surface<'static>>,
}

impl GpuRequest<'_> {
    /// The instance every host requests through, built from the environment
    /// so `WGPU_BACKEND` selects a backend. Without a display handle, GLES
    /// falls back to `EGL_MESA_platform_surfaceless`, which suits the
    /// offscreen and bench hosts.
    ///
    /// Separate from [`Self::open`] because a windowed host needs the instance
    /// to create its surface first.
    pub(crate) fn instance() -> Result<wgpu::Instance, GpuRequestError> {
        Self::check_backend()?;
        Ok(wgpu::Instance::new(
            wgpu::InstanceDescriptor::new_without_display_handle_from_env(),
        ))
    }

    /// The instance a windowed host requests through, carrying the display
    /// `window` lives on.
    ///
    /// GLES picks its EGL display from the handle; without it the adapter
    /// comes from a surfaceless display and is rejected as incompatible with
    /// the window's surface. Other backends ignore it.
    #[cfg(feature = "winit")]
    #[expect(
        clippy::absolute_paths,
        reason = "gated on `winit`, so it names the path inline instead of a cfg'd import"
    )]
    pub(crate) fn windowed_instance<W>(
        window: &std::sync::Arc<W>,
    ) -> Result<wgpu::Instance, GpuRequestError>
    where
        W: wgpu::rwh::HasDisplayHandle + std::fmt::Debug + Send + Sync + 'static,
    {
        Self::check_backend()?;
        Ok(wgpu::Instance::new(
            wgpu::InstanceDescriptor::new_with_display_handle_from_env(Box::new(
                std::sync::Arc::clone(window),
            )),
        ))
    }

    const fn check_backend() -> Result<(), GpuRequestError> {
        if wgpu::Instance::enabled_backend_features().is_empty() {
            return Err(GpuRequestError::NoBackend);
        }
        Ok(())
    }

    /// Pick an adapter and open a device on it.
    pub(crate) fn open(self, instance: &wgpu::Instance) -> Result<RequestedGpu, GpuRequestError> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: adapter_policy(self.power_preference),
                compatible_surface: self.compatible_surface,
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            })
            .block_on()
            .map_err(|source| GpuRequestError::RequestAdapter {
                source: DriverError::new(source),
            })?;

        // Which GPU won the `power_preference` sort decides a session's frame
        // times (a hybrid laptop's wrong pick means cross-adapter copies), and
        // nothing else reports it.
        let info = adapter.get_info();
        tracing::info!(
            name = %info.name,
            backend = ?info.backend,
            device_type = ?info.device_type,
            driver = %info.driver,
            driver_info = %info.driver_info,
            requested = ?self.power_preference,
            "selected gpu adapter"
        );

        let requirements = DeviceRequirements::negotiate(&adapter, self.optional)
            .map_err(|source| GpuRequestError::Requirements { source })?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some(self.label),
                required_features: requirements.features,
                required_limits: requirements.limits,
                experimental_features: wgpu::ExperimentalFeatures::default(),
                // Long-lived atlases, buffers and a staging belt reused every
                // frame: the shape `Performance` is for.
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .block_on()
            .map_err(|source| GpuRequestError::RequestDevice {
                source: DriverError::new(source),
            })?;

        Ok(RequestedGpu {
            adapter,
            gpu: Gpu::new(device, queue),
        })
    }
}

/// [`PowerPreference`] in the driver's own vocabulary. Not a `From` impl:
/// the target type is foreign.
const fn adapter_policy(preference: PowerPreference) -> wgpu::PowerPreference {
    match preference {
        PowerPreference::Any => wgpu::PowerPreference::None,
        PowerPreference::LowPower => wgpu::PowerPreference::LowPower,
        PowerPreference::HighPerformance => wgpu::PowerPreference::HighPerformance,
    }
}
