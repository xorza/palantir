//! [`Gpu`] — the shared wgpu context (instance / adapter / device / queue)
//! built once at startup and reused by every window. No winit-event or
//! app-contract concern lives here; it only creates surfaces and
//! per-window [`WindowRenderer`]s.

use std::sync::Arc;

use glam::UVec2;
use winit::window::Window;

use crate::text::TextShaper;
use crate::window_renderer::{WindowRenderer, WindowRendererConfig};
use crate::winit_host::config::WinitHostConfig;

/// Shared GPU context — built once on the first `resumed` and retained
/// for the host's lifetime so additional windows reuse one device/queue.
/// wgpu's `Device`/`Queue` are `Arc`-backed; cloning them into each
/// window's [`WindowRenderer`] shares one GPU context for free.
pub(crate) struct Gpu {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    /// Read by the host's resize handler for the max-texture clamp; the
    /// rest of the wgpu handles stay private to this module.
    pub(crate) device: wgpu::Device,
    queue: wgpu::Queue,
    /// Swapchain present mode applied to every window's surface — fixed
    /// at startup from `WinitHostConfig`, app-global.
    present_mode: wgpu::PresentMode,
    /// Whether the device was created with the timing-query features.
    /// Threaded into every window's `WindowRenderer` so the backend opts into
    /// instrumentation only when the device actually supports it.
    collect_gpu_stats: bool,
}

/// A window's swapchain pieces, produced by [`Gpu::make_surface`]. The
/// swapchain color format lives on `config.format`.
pub(crate) struct WindowSurface {
    pub(crate) surface: wgpu::Surface<'static>,
    pub(crate) config: wgpu::SurfaceConfiguration,
}

/// The shared [`Gpu`] plus the first window's surface, returned together
/// by [`Gpu::create`] — the probe surface used to select the adapter is
/// reused as that window's swapchain rather than recreated.
pub(crate) struct GpuInit {
    pub(crate) gpu: Gpu,
    pub(crate) first_surface: WindowSurface,
}

impl Gpu {
    /// Build the shared context, picking an adapter compatible with the
    /// first window's `surface`. Returns the context alongside that
    /// window's configured surface.
    pub(crate) fn create(window: &Arc<Window>, cfg: &WinitHostConfig) -> GpuInit {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let surface = instance
            .create_surface(window.clone())
            .expect("create surface");

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: cfg.power_preference,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
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
        // (WebGPU-only adapters are off-target for palantir).
        let required_features = (adapter.features() & timing_features) | wgpu::Features::IMMEDIATES;
        let mut required_limits = wgpu::Limits::default().using_resolution(adapter.limits());
        // 16 bytes covers `renderer::backend::text::Params` (vec2<u32>)
        // with room for the WGSL 16-byte uniform-struct rounding.
        required_limits.max_immediate_size = required_limits.max_immediate_size.max(16);
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("palantir.device"),
            required_features,
            required_limits,
            experimental_features: wgpu::ExperimentalFeatures::default(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        }))
        .expect("request device");

        let gpu = Self {
            instance,
            adapter,
            device,
            queue,
            present_mode: cfg.present_mode,
            collect_gpu_stats: cfg.collect_gpu_stats,
        };
        let size = window.inner_size();
        let first_surface = gpu.configure_surface(surface, UVec2::new(size.width, size.height));
        GpuInit { gpu, first_surface }
    }

    /// Create + configure a surface for an additional window against the
    /// already-built adapter/device.
    pub(crate) fn make_surface(&self, window: &Arc<Window>) -> WindowSurface {
        let surface = self
            .instance
            .create_surface(window.clone())
            .expect("create surface");
        let size = window.inner_size();
        self.configure_surface(surface, UVec2::new(size.width, size.height))
    }

    /// Pick an sRGB swapchain format and build the `SurfaceConfiguration`.
    /// `WindowRenderer::frame` configures the surface lazily on first paint (it
    /// notices `configured == None`), so no eager `surface.configure`
    /// here.
    fn configure_surface(&self, surface: wgpu::Surface<'static>, size: UVec2) -> WindowSurface {
        let caps = surface.get_capabilities(&self.adapter);
        // Color pipeline assumes an sRGB swapchain target — see the
        // colour section of CLAUDE.md. Non-sRGB would skip the GPU
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

    /// Build a fresh per-window [`WindowRenderer`] sharing this device/queue.
    pub(crate) fn make_renderer(&self, format: wgpu::TextureFormat) -> WindowRenderer {
        WindowRenderer::with_options(
            self.device.clone(),
            self.queue.clone(),
            format,
            TextShaper::with_bundled_fonts(),
            WindowRendererConfig {
                collect_gpu_stats: self.collect_gpu_stats,
            },
        )
    }
}
