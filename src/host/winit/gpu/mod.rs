//! Wgpu startup and the retained [`SurfaceManager`] used to create, configure,
//! and present native-window surfaces.

use std::num::NonZeroU32;
use std::sync::Arc;

use glam::UVec2;
use winit::window::{Window as WinitWindow, WindowId};

use crate::host::device_requirements::DeviceRequirements;
use crate::host::gpu_request::{GpuRequest, RequestedGpu};
use crate::host::winit::config::WinitHostConfig;
use crate::host::winit::error::WinitHostError;
use crate::window::vsync::Vsync;
use crate::window::window_token::WindowToken;

const REQUIRED_SURFACE_USAGES: wgpu::TextureUsages =
    wgpu::TextureUsages::RENDER_ATTACHMENT.union(wgpu::TextureUsages::COPY_DST);

/// Native-surface authority retained after startup. The cloned device/queue
/// handles refer to the same GPU objects owned by `WgpuBackend`.
#[derive(Debug)]
pub(super) struct SurfaceManager {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    /// The host's device and queue, held here because `configure` and
    /// `present` are this type's to run. `HostCore` clones them from here
    /// rather than being handed a second pair alongside — one holder, and a
    /// borrow for whoever else needs them.
    pub(super) device: wgpu::Device,
    pub(super) queue: wgpu::Queue,
    /// `max_texture_dimension_2d` granted at device creation — fixed for
    /// the device's lifetime, cached so the host's per-event resize clamp
    /// doesn't re-query `device.limits()`.
    pub(super) max_texture_dim: NonZeroU32,
    /// App-global presentation policy requested through `WinitHostConfig`.
    /// Each surface negotiates it against its own capabilities.
    requested_present_mode: wgpu::PresentMode,
}

impl SurfaceManager {
    /// A surface extent the device can actually back: at least one texel,
    /// at most `max_texture_dimension_2d`.
    ///
    /// Takes the limit rather than `&self` because the winit event handler
    /// has to read it before it borrows the window it is about to resize,
    /// and that borrow is what had the clamp written a second time there.
    pub(super) fn clamp_extent(max_texture_dim: NonZeroU32, size: UVec2) -> UVec2 {
        let max = max_texture_dim.get();
        UVec2::new(size.x.clamp(1, max), size.y.clamp(1, max))
    }
}

/// A window's swapchain pieces, produced by [`SurfaceManager::make_surface`]. The
/// swapchain color format lives on `config.format`.
#[derive(Debug)]
pub(super) struct WindowSurface {
    pub(super) surface: wgpu::Surface<'static>,
    pub(super) config: wgpu::SurfaceConfiguration,
}

/// Startup result. The probe surface used for adapter selection is reused as
/// the first window's swapchain.
#[derive(Debug)]
pub(super) struct GpuInit {
    pub(super) surfaces: SurfaceManager,
    pub(super) first_surface: WindowSurface,
}

impl GpuInit {
    /// Pick the shared adapter/device and create the first native surface.
    pub(super) fn new(
        token: WindowToken,
        window: &Arc<WinitWindow>,
        cfg: &WinitHostConfig,
    ) -> Result<Self, WinitHostError> {
        let instance = GpuRequest::instance()?;
        let surface = create_surface(&instance, token, window)?;

        // Caller-driven opt-in via `WinitHostConfig::collect_gpu_stats`
        // — see field doc. When off, none of the timing-query features
        // are requested, so the per-frame `resolve_query_set` +
        // `map_async` + `device.poll(Poll)` + readback are all
        // dead-stripped. When on, the three optional features degrade
        // independently per adapter advertisement: `DeviceRequirements`
        // intersects them with what the adapter offers and drops the rest,
        // rather than failing. `TIMESTAMP_QUERY` alone → pass begin/end only;
        // `+ TIMESTAMP_QUERY_INSIDE_PASSES` → per-batch attribution;
        // `+ PIPELINE_STATISTICS_QUERY` → vert/frag invocation counts.
        let timing_features = if cfg.collect_gpu_stats {
            DeviceRequirements::GPU_TIMING_FEATURES
        } else {
            wgpu::Features::empty()
        };
        let RequestedGpu {
            adapter,
            device,
            queue,
        } = GpuRequest {
            label: "palantir.device",
            power_preference: cfg.power_preference,
            optional: timing_features,
            compatible_surface: Some(&surface),
        }
        .open(&instance)?;

        let max_texture_dim = DeviceRequirements::max_texture_dim(&device);
        let surfaces = SurfaceManager {
            instance,
            adapter,
            device,
            queue,
            max_texture_dim,
            requested_present_mode: cfg.present_mode,
        };
        let first_surface = surfaces.window_surface(surface, window)?;
        Ok(Self {
            surfaces,
            first_surface,
        })
    }
}

/// Create a native surface for `window`, naming the window in the error.
///
/// A free function because [`GpuInit::new`] needs one *before* it has a
/// [`SurfaceManager`] — the adapter is picked against this very surface.
fn create_surface(
    instance: &wgpu::Instance,
    token: WindowToken,
    window: &Arc<WinitWindow>,
) -> Result<wgpu::Surface<'static>, WinitHostError> {
    instance
        .create_surface(window.clone())
        .map_err(|source| WinitHostError::CreateSurface { token, source })
}

impl SurfaceManager {
    /// Create a surface for an additional window against the selected adapter.
    pub(super) fn make_surface(
        &self,
        token: WindowToken,
        window: &Arc<WinitWindow>,
    ) -> Result<WindowSurface, WinitHostError> {
        let surface = create_surface(&self.instance, token, window)?;
        self.window_surface(surface, window)
    }

    /// Configure `surface` against the window it was created for.
    ///
    /// Apart from [`Self::make_surface`], which creates the surface and
    /// calls this, because startup cannot take those two steps together:
    /// [`GpuInit::new`] needs the surface to pick the adapter, and only
    /// the adapter gives it a `SurfaceManager` to configure with.
    fn window_surface(
        &self,
        surface: wgpu::Surface<'static>,
        window: &Arc<WinitWindow>,
    ) -> Result<WindowSurface, WinitHostError> {
        let size = window.inner_size();
        self.build_window_surface(surface, UVec2::new(size.width, size.height), window.id())
    }

    pub(super) fn configure(&self, surface: &wgpu::Surface, config: &wgpu::SurfaceConfiguration) {
        surface.configure(&self.device, config);
    }

    pub(super) fn present(&self, frame: wgpu::SurfaceTexture) {
        self.queue.present(frame);
    }

    /// Pick an sRGB swapchain format and bundle `surface` with a fresh
    /// `SurfaceConfiguration` into a [`WindowSurface`] — *without* calling
    /// `surface.configure`.
    /// [`Window`](crate::host::winit::window::Window) applies it
    /// lazily on first paint, so there's no eager GPU reconfigure here.
    fn build_window_surface(
        &self,
        surface: wgpu::Surface<'static>,
        size: UVec2,
        window_id: WindowId,
    ) -> Result<WindowSurface, WinitHostError> {
        let caps = surface.get_capabilities(&self.adapter);
        let config = build_surface_config(
            &caps,
            Self::clamp_extent(self.max_texture_dim, size),
            self.requested_present_mode,
        )?;
        if config.present_mode != self.requested_present_mode {
            tracing::warn!(
                ?window_id,
                requested = ?self.requested_present_mode,
                fallback = ?config.present_mode,
                supported = ?caps.present_modes,
                "requested present mode is unsupported by this surface"
            );
        }
        Ok(WindowSurface { surface, config })
    }
}

fn build_surface_config(
    caps: &wgpu::SurfaceCapabilities,
    size: UVec2,
    requested_present_mode: wgpu::PresentMode,
) -> Result<wgpu::SurfaceConfiguration, WinitHostError> {
    if caps.formats.is_empty() || caps.present_modes.is_empty() || caps.alpha_modes.is_empty() {
        return Err(WinitHostError::IncompatibleSurface);
    }
    if !caps.usages.contains(REQUIRED_SURFACE_USAGES) {
        return Err(WinitHostError::MissingSurfaceUsages {
            required: REQUIRED_SURFACE_USAGES,
            supported: caps.usages,
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
        .ok_or(WinitHostError::MissingSrgbSurface)?;
    let present_mode = negotiate_present_mode(requested_present_mode, &caps.present_modes);
    Ok(wgpu::SurfaceConfiguration {
        usage: REQUIRED_SURFACE_USAGES,
        format,
        color_space: wgpu::SurfaceColorSpace::Srgb,
        width: size.x,
        height: size.y,
        present_mode,
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

/// The present mode a [`Vsync`] setting asks for. Both map to *automatic*
/// policies, which every surface accepts — wgpu resolves each against what
/// the surface actually supports — so unlike an explicit mode from
/// [`WinitHostConfig`] this needs no negotiation
/// and can be applied to a live swapchain directly.
pub(super) fn present_mode(vsync: Vsync) -> wgpu::PresentMode {
    match vsync {
        Vsync::On => wgpu::PresentMode::AutoVsync,
        Vsync::Off => wgpu::PresentMode::AutoNoVsync,
    }
}

/// Which of [`Vsync`]'s two states `mode` paces like — the classification
/// [`present_mode`] is a right inverse of.
///
/// Not injective, and that is the point: an explicit mode from
/// [`WinitHostConfig`] is a finer choice than the runtime toggle can express,
/// so this answers what the toggle would have to say about it. A swapchain
/// opened on `Mailbox` reads back as [`Vsync::Off`] and stays `Mailbox` until
/// something asks for [`Vsync::On`] — the explicit choice survives a control
/// that writes its own value back every frame.
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

fn negotiate_present_mode(
    requested: wgpu::PresentMode,
    supported: &[wgpu::PresentMode],
) -> wgpu::PresentMode {
    match requested {
        wgpu::PresentMode::AutoVsync | wgpu::PresentMode::AutoNoVsync => requested,
        explicit if supported.contains(&explicit) => explicit,
        // An explicit mode the surface will not take falls back to the
        // automatic policy that paces the same way. [`vsync_of`] is that
        // classification and [`present_mode`] its right inverse, so a
        // present mode wgpu adds is placed in one match rather than three.
        explicit => present_mode(vsync_of(explicit)),
    }
}

#[cfg(test)]
mod tests;
