//! Device startup and the retained authority that creates, configures and
//! presents native-window surfaces.

use std::num::NonZeroU32;
use std::sync::Arc;

use glam::UVec2;

use crate::gpu::device_requirements::DeviceRequirements;
use crate::gpu::error::{self, DriverError, SurfaceError};
use crate::gpu::power_preference::PowerPreference;
use crate::gpu::requested_gpu::Gpu;
use crate::gpu::requested_gpu::{GpuRequest, RequestedGpu};
use crate::gpu::window_surface::WindowSurface;
use crate::window::vsync::Vsync;

const REQUIRED_SURFACE_USAGES: wgpu::TextureUsages =
    wgpu::TextureUsages::RENDER_ATTACHMENT.union(wgpu::TextureUsages::COPY_DST);

/// The device-and-swapchain choices a host seals once at startup and every
/// window then inherits.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct HostGpuConfig {
    pub(crate) power_preference: PowerPreference,
    pub(crate) vsync: Vsync,
    /// Opt into timestamp and pipeline-statistics queries. Off leaves the
    /// per-frame readback dead-stripped.
    pub(crate) collect_gpu_stats: bool,
}

/// Native-surface authority retained after startup. The cloned device and
/// queue handles refer to the same objects `WgpuBackend` owns.
#[derive(Debug)]
pub(crate) struct SurfaceManager {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    /// The host's device and queue, held here because `configure` and
    /// `present` are this type's to run. `HostCore` clones them from here
    /// rather than being handed a second pair alongside — one holder, and a
    /// clone for whoever else needs them.
    pub(crate) gpu: Gpu,
    /// `max_texture_dimension_2d` granted at device creation — fixed for
    /// the device's lifetime, cached so the host's per-event resize clamp
    /// doesn't re-query `device.limits()`.
    pub(crate) max_texture_dim: NonZeroU32,
    /// App-global pacing requested through the host config. Every surface
    /// accepts it, because both states map to an *automatic* policy the
    /// driver resolves per surface.
    vsync: Vsync,
}

/// What [`SurfaceManager::start`] hands back. The probe surface used for
/// adapter selection is reused as the first window's swapchain, so the two
/// come out together or not at all.
#[derive(Debug)]
pub(crate) struct SurfaceStartup {
    pub(crate) surfaces: SurfaceManager,
    pub(crate) first_surface: WindowSurface,
}

impl SurfaceManager {
    /// Pick the shared adapter and device, and create the first native
    /// surface against them.
    ///
    /// `window` is anything the platform can hand a surface for; taking it as
    /// a type parameter is what keeps the windowing toolkit out of this
    /// module. `size` is the window's physical extent, which the caller reads
    /// from its own window type.
    pub(crate) fn start<W>(
        window: &Arc<W>,
        size: UVec2,
        cfg: HostGpuConfig,
    ) -> Result<SurfaceStartup, SurfaceError>
    where
        W: wgpu::DisplayAndWindowHandle + 'static,
    {
        let instance = GpuRequest::instance()?;
        let surface = create_surface(&instance, window)?;

        // Caller-driven opt-in through `HostGpuConfig::collect_gpu_stats`.
        // When on, the three optional features degrade independently per
        // adapter advertisement: `DeviceRequirements` intersects them with
        // what the adapter offers and drops the rest, rather than failing.
        // `TIMESTAMP_QUERY` alone → pass begin/end only;
        // `+ TIMESTAMP_QUERY_INSIDE_PASSES` → per-batch attribution;
        // `+ PIPELINE_STATISTICS_QUERY` → vertex/fragment invocation counts.
        let timing_features = if cfg.collect_gpu_stats {
            DeviceRequirements::GPU_TIMING_FEATURES
        } else {
            wgpu::Features::empty()
        };
        let RequestedGpu { adapter, gpu } = GpuRequest {
            label: "palantir.device",
            power_preference: cfg.power_preference,
            optional: timing_features,
            compatible_surface: Some(&surface),
        }
        .open(&instance)?;

        let max_texture_dim = gpu.max_texture_dim();
        let surfaces = SurfaceManager {
            instance,
            adapter,
            gpu,
            max_texture_dim,
            vsync: cfg.vsync,
        };
        let first_surface = surfaces.build_window_surface(surface, size)?;
        Ok(SurfaceStartup {
            surfaces,
            first_surface,
        })
    }
}

/// Create a native surface for `window`.
///
/// A free function because [`SurfaceManager::start`] needs one *before* it has
/// a [`SurfaceManager`] — the adapter is picked against this very surface.
fn create_surface<W>(
    instance: &wgpu::Instance,
    window: &Arc<W>,
) -> Result<wgpu::Surface<'static>, SurfaceError>
where
    W: wgpu::DisplayAndWindowHandle + 'static,
{
    instance
        .create_surface(Arc::clone(window))
        .map_err(|source| SurfaceError::Create {
            source: DriverError::new(source),
        })
}

impl SurfaceManager {
    /// A surface extent the device can actually back: at least one texel,
    /// at most `max_texture_dimension_2d`.
    ///
    /// Takes the limit rather than `&self` because the host's event handler
    /// has to read it before it borrows the window it is about to resize,
    /// and that borrow is what had the clamp written a second time there.
    pub(crate) fn clamp_extent(max_texture_dim: NonZeroU32, size: UVec2) -> UVec2 {
        let max = max_texture_dim.get();
        UVec2::new(size.x.clamp(1, max), size.y.clamp(1, max))
    }

    /// Create and configure a swapchain for an additional window against the
    /// selected adapter.
    pub(crate) fn make_surface<W>(
        &self,
        window: &Arc<W>,
        size: UVec2,
    ) -> Result<WindowSurface, SurfaceError>
    where
        W: wgpu::DisplayAndWindowHandle + 'static,
    {
        let surface = create_surface(&self.instance, window)?;
        self.build_window_surface(surface, size)
    }

    /// Pick an sRGB swapchain format and bundle `surface` with a fresh
    /// configuration — *without* calling `surface.configure`. The host
    /// applies it lazily on the first paint, so there is no eager device
    /// reconfigure here.
    fn build_window_surface(
        &self,
        surface: wgpu::Surface<'static>,
        size: UVec2,
    ) -> Result<WindowSurface, SurfaceError> {
        let caps = surface.get_capabilities(&self.adapter);
        let config = build_surface_config(
            &caps,
            Self::clamp_extent(self.max_texture_dim, size),
            self.vsync,
        )?;
        Ok(WindowSurface::new(surface, config))
    }
}

/// The swapchain policy `vsync` asks for.
///
/// Both are *automatic* policies, which every surface accepts — the driver
/// resolves each against what the surface actually supports — so there is
/// nothing to negotiate and either can be applied to a live swapchain
/// directly.
///
/// A free function rather than a `From` impl: the target type is foreign, and
/// this crate does not implement a foreign trait for a foreign type.
pub(super) fn swapchain_mode(vsync: Vsync) -> wgpu::PresentMode {
    match vsync {
        Vsync::On => wgpu::PresentMode::AutoVsync,
        Vsync::Off => wgpu::PresentMode::AutoNoVsync,
    }
}

/// Which of [`Vsync`]'s two states `mode` paces like.
///
/// Total over the driver's vocabulary although Palantir only ever asks for
/// the two automatic policies, because a surface reports back the mode it
/// resolved them to.
///
/// A free function although `From<wgpu::PresentMode> for Vsync` would compile:
/// that impl would have to live in [`Vsync`]'s own file, which would put a
/// graphics-API type in `crate::window`.
pub(super) fn vsync_of(mode: wgpu::PresentMode) -> Vsync {
    match mode {
        wgpu::PresentMode::AutoVsync | wgpu::PresentMode::Fifo | wgpu::PresentMode::FifoRelaxed => {
            Vsync::On
        }
        wgpu::PresentMode::AutoNoVsync
        | wgpu::PresentMode::Immediate
        | wgpu::PresentMode::Mailbox => Vsync::Off,
    }
}

fn build_surface_config(
    caps: &wgpu::SurfaceCapabilities,
    size: UVec2,
    vsync: Vsync,
) -> Result<wgpu::SurfaceConfiguration, SurfaceError> {
    if caps.formats.is_empty() || caps.present_modes.is_empty() || caps.alpha_modes.is_empty() {
        return Err(SurfaceError::Incompatible);
    }
    if !caps.usages.contains(REQUIRED_SURFACE_USAGES) {
        return Err(SurfaceError::MissingUsages {
            missing: error::flag_names((REQUIRED_SURFACE_USAGES - caps.usages).iter_names()),
        });
    }
    // The color pipeline writes linear values and relies on an sRGB
    // swapchain for the final encode.
    let format = caps
        .formats
        .iter()
        .copied()
        .find(|format| {
            format.is_srgb()
                && caps
                    .color_spaces(*format)
                    .contains(wgpu::SurfaceColorSpaces::SRGB)
        })
        .ok_or(SurfaceError::MissingSrgb)?;
    Ok(wgpu::SurfaceConfiguration {
        usage: REQUIRED_SURFACE_USAGES,
        format,
        color_space: wgpu::SurfaceColorSpace::Srgb,
        width: size.x,
        height: size.y,
        present_mode: swapchain_mode(vsync),
        alpha_mode: if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::Opaque) {
            wgpu::CompositeAlphaMode::Opaque
        } else {
            caps.alpha_modes[0]
        },
        view_formats: vec![],
        // One frame of latency maps to a double-buffered swapchain.
        desired_maximum_frame_latency: 1,
    })
}

#[cfg(test)]
mod tests;
