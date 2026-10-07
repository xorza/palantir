//! Device startup and the retained authority that creates, configures and
//! presents native-window surfaces.

use std::num::NonZeroU32;
use std::sync::Arc;

use glam::UVec2;

use crate::gpu::device::device_requirements::DeviceRequirements;
use crate::gpu::device::power_preference::PowerPreference;
use crate::gpu::device::requested_gpu::Gpu;
use crate::gpu::device::requested_gpu::{GpuRequest, RequestedGpu};
use crate::gpu::error::{self, DriverError, SurfaceError};
use crate::gpu::surface::window_surface::WindowSurface;
use crate::window::vsync::Vsync;
use std::fmt;

const REQUIRED_SURFACE_USAGES: wgpu::TextureUsages = wgpu::TextureUsages::RENDER_ATTACHMENT;

/// Taken when offered. A GLES surface is the default framebuffer and
/// advertises only `RENDER_ATTACHMENT`, so requiring this made it unusable;
/// without it the backbuffer is drawn onto the surface
/// ([`Backbuffer::draw_onto`](crate::gpu::surface::backbuffer::Backbuffer::draw_onto)).
const OPTIONAL_SURFACE_USAGES: wgpu::TextureUsages = wgpu::TextureUsages::COPY_DST;

/// Device and swapchain choices a host seals at startup.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct HostGpuConfig {
    pub(crate) power_preference: PowerPreference,
    pub(crate) vsync: Vsync,
    /// Opt into timestamp and pipeline-statistics queries.
    pub(crate) collect_gpu_stats: bool,
}

/// Native-surface authority retained after startup.
#[derive(Debug)]
pub(crate) struct SurfaceManager {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    /// The host's device and queue; `HostCore` clones them from here.
    pub(crate) gpu: Gpu,
    /// `max_texture_dimension_2d` at device creation, cached for the resize clamp.
    pub(crate) max_texture_dim: NonZeroU32,
    /// App-global pacing; both states map to automatic policies every surface accepts.
    vsync: Vsync,
}

/// What [`SurfaceManager::start`] returns: the adapter-probe surface is
/// reused as the first window's swapchain.
#[derive(Debug)]
pub(crate) struct SurfaceStartup {
    pub(crate) surfaces: SurfaceManager,
    pub(crate) first_surface: WindowSurface,
}

impl SurfaceManager {
    /// Pick the shared adapter and device, and create the first native surface.
    /// `window` is generic to keep the windowing toolkit out of this module;
    /// `size` is its physical extent.
    pub(crate) fn start<W>(
        window: &Arc<W>,
        size: UVec2,
        cfg: HostGpuConfig,
    ) -> Result<SurfaceStartup, SurfaceError>
    where
        W: wgpu::DisplayAndWindowHandle + fmt::Debug + 'static,
    {
        // Built from the window's display handle so GLES finds its EGL display.
        let instance = GpuRequest::windowed_instance(window)?;
        let surface = create_surface(&instance, window)?;

        // Timing features are optional: `DeviceRequirements` drops those the
        // adapter lacks rather than failing.
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

/// Create a native surface for `window`; free because the adapter is picked
/// against it before a [`SurfaceManager`] exists.
fn create_surface<W>(
    instance: &wgpu::Instance,
    window: &Arc<W>,
) -> Result<wgpu::Surface<'static>, SurfaceError>
where
    W: wgpu::DisplayAndWindowHandle + fmt::Debug + 'static,
{
    instance
        .create_surface(Arc::clone(window))
        .map_err(|source| SurfaceError::Create {
            source: DriverError::new(source),
        })
}

impl SurfaceManager {
    /// A surface extent the device can back: at least one texel, at most
    /// `max_texture_dimension_2d`. Takes the limit so the host can clamp before
    /// borrowing the window.
    pub(crate) fn clamp_extent(max_texture_dim: NonZeroU32, size: UVec2) -> UVec2 {
        let max = max_texture_dim.get();
        UVec2::new(size.x.clamp(1, max), size.y.clamp(1, max))
    }

    /// Create a surface for an additional window.
    pub(crate) fn make_surface<W>(
        &self,
        window: &Arc<W>,
        size: UVec2,
    ) -> Result<WindowSurface, SurfaceError>
    where
        W: wgpu::DisplayAndWindowHandle + fmt::Debug + 'static,
    {
        let surface = create_surface(&self.instance, window)?;
        self.build_window_surface(surface, size)
    }

    /// Pick an sRGB format and bundle `surface` with a fresh configuration,
    /// without calling `surface.configure`; the host applies it on first paint.
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

/// The swapchain policy `vsync` asks for. Both are automatic policies every
/// surface accepts, so either applies to a live swapchain directly.
pub(super) const fn swapchain_mode(vsync: Vsync) -> wgpu::PresentMode {
    match vsync {
        Vsync::On => wgpu::PresentMode::AutoVsync,
        Vsync::Off => wgpu::PresentMode::AutoNoVsync,
    }
}

/// Which [`Vsync`] state `mode` paces like. Total over the driver's modes
/// because a surface reports the mode it resolved to. Free to keep
/// graphics-API types out of `crate::window`.
pub(super) const fn vsync_of(mode: wgpu::PresentMode) -> Vsync {
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
    // The pipeline writes linear values and relies on an sRGB swapchain.
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
        usage: REQUIRED_SURFACE_USAGES | (caps.usages & OPTIONAL_SURFACE_USAGES),
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
        desired_maximum_frame_latency: 1,
    })
}

#[cfg(test)]
mod tests;
