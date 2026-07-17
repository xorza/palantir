//! Wgpu startup and the retained [`SurfaceFactory`] used for additional
//! windows. The created device and queue move into the one shared backend.

use std::sync::Arc;

use glam::UVec2;
use winit::window::Window;

use crate::host::shared::HostShared;
use crate::host::winit::config::WinitHostConfig;
use crate::renderer::backend::{BackendConfig, WgpuBackend};

/// Surface-creation state retained after startup. Device/queue ownership moves
/// into `WgpuBackend`; later windows need only the instance and adapter.
#[derive(Debug)]
pub(crate) struct SurfaceFactory {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    /// `max_texture_dimension_2d` granted at device creation — fixed for
    /// the device's lifetime, cached so the host's per-event resize clamp
    /// doesn't re-query `device.limits()`.
    pub(crate) max_texture_dim: u32,
    /// Swapchain present mode applied to every window's surface — fixed
    /// at startup from `WinitHostConfig`, app-global.
    present_mode: wgpu::PresentMode,
}

/// A window's swapchain pieces, produced by [`SurfaceFactory::make_surface`]. The
/// swapchain color format lives on `config.format`.
#[derive(Debug)]
pub(crate) struct WindowSurface {
    pub(crate) surface: wgpu::Surface<'static>,
    pub(crate) config: wgpu::SurfaceConfiguration,
}

/// Startup result. The probe surface used for adapter selection is reused as
/// the first window's swapchain.
#[derive(Debug)]
pub(crate) struct GpuInit {
    pub(crate) surfaces: SurfaceFactory,
    pub(crate) backend: WgpuBackend,
    pub(crate) first_surface: WindowSurface,
}

impl GpuInit {
    /// Pick the shared adapter/device, move the device and queue into the
    /// backend, and retain only what later surface creation needs.
    pub(crate) fn new(window: &Arc<Window>, cfg: &WinitHostConfig, shared: &HostShared) -> Self {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.flags = desc.flags.with_env();
        let instance = wgpu::Instance::new(desc);
        let surface = instance
            .create_surface(window.clone())
            .expect("create surface");

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: cfg.power_preference,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .expect("request adapter");

        // Caller-driven opt-in via `WinitHostConfig::collect_gpu_stats`
        // — see field doc. When off, none of the timing-query features
        // are requested, so the per-frame `resolve_query_set` +
        // `map_async` + `device.poll(Poll)` + readback are all
        // dead-stripped. When on, the three optional features degrade
        // independently per adapter advertisement: the intersection with
        // `adapter.features()` below drops bits the adapter doesn't
        // support. `TIMESTAMP_QUERY` alone → pass begin/end only;
        // `+ TIMESTAMP_QUERY_INSIDE_PASSES` → per-batch attribution;
        // `+ PIPELINE_STATISTICS_QUERY` → vert/frag invocation counts.
        let timing_features = if cfg.collect_gpu_stats {
            wgpu::Features::TIMESTAMP_QUERY
                | wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES
                | wgpu::Features::PIPELINE_STATISTICS_QUERY
        } else {
            wgpu::Features::empty()
        };
        // `IMMEDIATES` carries the text backend's atlas-size params
        // (`renderer::backend::text::Params`) — register-mapped per-pass
        // instead of a uniform buffer + bind group. Unconditionally
        // required because every Metal/Vulkan/DX12 adapter exposes it
        // (WebGPU-only adapters are off-target for aperture).
        let required_features = (adapter.features() & timing_features) | wgpu::Features::IMMEDIATES;
        let mut required_limits = wgpu::Limits::default().using_resolution(adapter.limits());
        // 16 bytes covers `renderer::backend::text::Params` (vec2<u32>)
        // with room for the WGSL 16-byte uniform-struct rounding.
        required_limits.max_immediate_size = required_limits.max_immediate_size.max(16);
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("aperture.device"),
            required_features,
            required_limits,
            experimental_features: wgpu::ExperimentalFeatures::default(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        }))
        .expect("request device");

        let max_texture_dim = device.limits().max_texture_dimension_2d;
        let backend = WgpuBackend::new(
            device,
            queue,
            shared.backend_shared(),
            BackendConfig {
                collect_gpu_stats: cfg.collect_gpu_stats,
            },
        );
        let surfaces = SurfaceFactory {
            instance,
            adapter,
            max_texture_dim,
            present_mode: cfg.present_mode,
        };
        let size = window.inner_size();
        let first_surface =
            surfaces.build_window_surface(surface, UVec2::new(size.width, size.height));
        Self {
            surfaces,
            backend,
            first_surface,
        }
    }
}

impl SurfaceFactory {
    /// Create a surface for an additional window against the selected adapter.
    pub(crate) fn make_surface(&self, window: &Arc<Window>) -> WindowSurface {
        let surface = self
            .instance
            .create_surface(window.clone())
            .expect("create surface");
        let size = window.inner_size();
        self.build_window_surface(surface, UVec2::new(size.width, size.height))
    }

    /// Pick an sRGB swapchain format and bundle `surface` with a fresh
    /// `SurfaceConfiguration` into a [`WindowSurface`] — *without* calling
    /// `surface.configure` (distinct from `WgpuBackend::configure_surface`,
    /// which applies it). `WindowDriver::frame` applies it lazily on first
    /// paint (it notices `configured == None`), so there's no eager GPU
    /// reconfigure here.
    fn build_window_surface(&self, surface: wgpu::Surface<'static>, size: UVec2) -> WindowSurface {
        let caps = surface.get_capabilities(&self.adapter);
        // Color pipeline assumes an sRGB swapchain target — see the
        // colour section of AGENTS.md. Non-sRGB would skip the GPU
        // linear→sRGB encode and silently darken every paint.
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .expect("no sRGB-capable surface format");
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_DST,
            format,
            // Pinned to `Srgb` (not `Auto`) to state the colour contract in
            // code: our `is_srgb()` format pick encodes linear→sRGB on write,
            // so the swapchain must read its bytes as sRGB. Guaranteed valid —
            // a non-fp16 format lists in `caps.formats` only when its `Auto`
            // fallback (`Srgb`) is supported, so this can't fail configure.
            color_space: wgpu::SurfaceColorSpace::Srgb,
            width: size.x.max(1),
            height: size.y.max(1),
            present_mode: self.present_mode,
            alpha_mode: if caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::Opaque) {
                wgpu::CompositeAlphaMode::Opaque
            } else {
                caps.alpha_modes[0]
            },
            view_formats: vec![],
            // Smallest swapchain: 1 frame of latency → double-buffered
            // (two images), lowest input-to-photon latency.
            desired_maximum_frame_latency: 1,
        };
        WindowSurface { surface, config }
    }
}
