//! The device Palantir draws with, and the one place it asks a driver for
//! one.

use std::num::NonZeroU32;

use pollster::FutureExt;

use crate::gpu::device_requirements::DeviceRequirements;
use crate::gpu::error::{GpuRequestError, UnmetRequirements};
use crate::gpu::power_preference::PowerPreference;

/// A graphics device and its queue, as Palantir holds them.
///
/// The one way a device reaches a host. An application that opened its own
/// wraps the pair here and hands it to
/// [`OffscreenHost::builder`](crate::OffscreenHost::builder); a host that
/// opened its own through [`RequestedGpu`] finds it on the
/// [`gpu`](RequestedGpu::gpu) field. Either way the host layer holds a device
/// without naming a graphics-API type.
///
/// Cloning clones the handles, not the device. Every clone addresses the one
/// device the application owns.
#[derive(Clone, Debug)]
pub struct Gpu {
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
}

impl Gpu {
    /// Draw through a device the caller owns.
    ///
    /// It has to have been requested with what Palantir needs — see
    /// [`DeviceRequirements`]. [`OffscreenHost::builder`](crate::OffscreenHost::builder)
    /// checks that before it builds anything.
    pub fn new(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        Self { device, queue }
    }

    pub(crate) fn requirements_met(&self) -> Result<(), UnmetRequirements> {
        DeviceRequirements::met_by(&self.device)
    }

    /// The device's `max_texture_dimension_2d`, which every host has to read
    /// and hand on: it is the ceiling on a registered image, on a `GpuView`
    /// target, and on the glyph and gradient atlases.
    ///
    /// Here rather than at each host, because both spelled the same
    /// `NonZeroU32::new(..).expect(..)` with the same message. A zero limit is
    /// not a device Palantir can draw on at all, so it panics rather than
    /// joining [`DeviceRequirements::met_by`]'s `Result`: no adapter reports
    /// one, and threading an error for it would put a match on every host's
    /// startup path for a case that cannot arise.
    pub(crate) fn max_texture_dim(&self) -> NonZeroU32 {
        NonZeroU32::new(self.device.limits().max_texture_dimension_2d)
            .expect("device texture dimension limit is zero")
    }
}

/// An adapter and the device opened on it, meeting [`DeviceRequirements`].
///
/// [`OffscreenHost`](crate::OffscreenHost) renders to a texture and so needs
/// no window, but it still needs a device, and the features that device must
/// carry are Palantir's business rather than the caller's.
/// [`Self::headless`] is the short way to a usable one: screenshots,
/// thumbnails, server-side compositing, and tests that want a frame without a
/// compositor in the loop. The windowed host opens its own the same way,
/// with the window's surface attached.
///
/// An application that already owns a device should keep it and go through
/// [`DeviceRequirements::negotiate`] instead — Palantir's needs fold into its
/// own request rather than replacing it.
#[derive(Debug)]
pub struct RequestedGpu {
    /// The adapter that answered the request.
    pub adapter: wgpu::Adapter,
    /// The device it opened, and that device's queue.
    pub gpu: Gpu,
}

impl RequestedGpu {
    /// Open one with no surface attached, taking the `optional` features that
    /// turn out to be available.
    ///
    /// Blocks. Adapter and device requests are futures that resolve against
    /// the driver rather than an async runtime, so there is nothing here for a
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

/// What to ask a driver for, and the steps that ask it.
///
/// Both hosts want the same five: check that a backend is compiled in, build
/// an instance, pick an adapter, negotiate what to require of it, open the
/// device. Written once per host they drifted in every step that had a
/// choice — one applied `WGPU_BACKEND` through the `_from_env` descriptor
/// while the other hand-applied two of the three env hooks; one asked for
/// `MemoryHints::Performance` and the other took the default; one logged the
/// adapter it picked and the other did not; and each raised its own error
/// enum with its own wording for the same four failures.
///
/// The surface is the only thing that genuinely differs, and it is a field.
#[derive(Debug)]
pub(crate) struct GpuRequest<'a> {
    /// Names the device in a debugger and in wgpu's own messages.
    pub(crate) label: &'static str,
    pub(crate) power_preference: PowerPreference,
    /// Taken when the adapter has them, dropped when it does not — see
    /// [`DeviceRequirements::negotiate`].
    pub(crate) optional: wgpu::Features,
    /// The surface the adapter has to be able to present to. `None` is the
    /// headless case, where nothing constrains the pick but the power policy.
    pub(crate) compatible_surface: Option<&'a wgpu::Surface<'static>>,
}

impl GpuRequest<'_> {
    /// The instance every host requests through.
    ///
    /// Built from the environment, which is what makes `WGPU_BACKEND=dx12` a
    /// usable A/B when a session's frame times or artifacts look
    /// backend-specific. Without the backends half every backend wgpu was
    /// built with is enumerated and the pick is whatever the adapter sort
    /// lands on — on Windows that is Vulkan before Dx12, with a *stable* sort,
    /// so a tie between two same-device-type adapters is decided by
    /// enumeration order and nothing else.
    ///
    /// Separate from [`Self::open`] because the windowed host has to create
    /// its surface from the instance before it can ask for an adapter that
    /// presents to it.
    pub(crate) fn instance() -> Result<wgpu::Instance, GpuRequestError> {
        if wgpu::Instance::enabled_backend_features().is_empty() {
            return Err(GpuRequestError::NoBackend);
        }
        Ok(wgpu::Instance::new(
            wgpu::InstanceDescriptor::new_without_display_handle_from_env(),
        ))
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
            .map_err(|source| GpuRequestError::RequestAdapter { source })?;

        // Which physical GPU and backend won the `power_preference` sort is
        // the single most load-bearing fact about a session's frame times, and
        // nothing else reports it: on a hybrid laptop the "wrong" pick renders
        // on the iGPU while the display hangs off the dGPU, so every present
        // becomes a cross-adapter copy. Log it once, wherever the device came
        // from — a headless bench that picked the software rasterizer is the
        // same surprise as a window that did.
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
                // Palantir keeps long-lived atlases, vertex buffers and a
                // staging belt, and re-uses all three every frame. That is
                // the shape `Performance` is for, whether the frames go to a
                // swapchain or to a texture.
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .block_on()
            .map_err(|source| GpuRequestError::RequestDevice { source })?;

        Ok(RequestedGpu {
            adapter,
            gpu: Gpu::new(device, queue),
        })
    }
}

/// [`PowerPreference`] in the driver's own vocabulary.
///
/// A free function rather than a `From` impl: the target type is foreign, and
/// this crate does not implement a foreign trait for a foreign type.
fn adapter_policy(preference: PowerPreference) -> wgpu::PowerPreference {
    match preference {
        PowerPreference::Any => wgpu::PowerPreference::None,
        PowerPreference::LowPower => wgpu::PowerPreference::LowPower,
        PowerPreference::HighPerformance => wgpu::PowerPreference::HighPerformance,
    }
}
