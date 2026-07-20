//! Wgpu startup and the retained [`SurfaceManager`] used to create, configure,
//! and present native-window surfaces.

use std::sync::Arc;

use glam::UVec2;
use winit::window::{Window as WinitWindow, WindowId};

use crate::host::shared::HostShared;
use crate::host::winit::config::WinitHostConfig;
use crate::renderer::backend::{BackendConfig, WgpuBackend};

/// Native-surface authority retained after startup. The cloned device/queue
/// handles refer to the same GPU objects owned by `WgpuBackend`.
#[derive(Debug)]
pub(crate) struct SurfaceManager {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    /// `max_texture_dimension_2d` granted at device creation — fixed for
    /// the device's lifetime, cached so the host's per-event resize clamp
    /// doesn't re-query `device.limits()`.
    pub(crate) max_texture_dim: u32,
    /// App-global presentation policy requested through `WinitHostConfig`.
    /// Each surface negotiates it against its own capabilities.
    requested_present_mode: wgpu::PresentMode,
}

/// A window's swapchain pieces, produced by [`SurfaceManager::make_surface`]. The
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
    pub(crate) surfaces: SurfaceManager,
    pub(crate) backend: WgpuBackend,
    pub(crate) first_surface: WindowSurface,
}

impl GpuInit {
    /// Pick the shared adapter/device and give the renderer and native-surface
    /// manager handles to the same device and queue.
    pub(crate) fn new(
        window: &Arc<WinitWindow>,
        cfg: &WinitHostConfig,
        shared: &HostShared,
    ) -> Self {
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
        let surfaces = SurfaceManager {
            instance,
            adapter,
            device: device.clone(),
            queue: queue.clone(),
            max_texture_dim,
            requested_present_mode: cfg.present_mode,
        };
        let backend = WgpuBackend::new(
            device,
            queue,
            shared.backend_shared(),
            BackendConfig {
                collect_gpu_stats: cfg.collect_gpu_stats,
            },
        );
        let size = window.inner_size();
        let first_surface = surfaces.build_window_surface(
            surface,
            UVec2::new(size.width, size.height),
            window.id(),
        );
        Self {
            surfaces,
            backend,
            first_surface,
        }
    }
}

impl SurfaceManager {
    /// Create a surface for an additional window against the selected adapter.
    pub(crate) fn make_surface(&self, window: &Arc<WinitWindow>) -> WindowSurface {
        let surface = self
            .instance
            .create_surface(window.clone())
            .expect("create surface");
        let size = window.inner_size();
        self.build_window_surface(surface, UVec2::new(size.width, size.height), window.id())
    }

    pub(crate) fn configure(&self, surface: &wgpu::Surface, config: &wgpu::SurfaceConfiguration) {
        surface.configure(&self.device, config);
    }

    pub(crate) fn present(&self, frame: wgpu::SurfaceTexture) {
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
    ) -> WindowSurface {
        let caps = surface.get_capabilities(&self.adapter);
        let present_mode = negotiate_present_mode(self.requested_present_mode, &caps.present_modes);
        if present_mode != self.requested_present_mode {
            tracing::warn!(
                ?window_id,
                requested = ?self.requested_present_mode,
                fallback = ?present_mode,
                supported = ?caps.present_modes,
                "requested present mode is unsupported by this surface"
            );
        }
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
            present_mode,
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

fn negotiate_present_mode(
    requested: wgpu::PresentMode,
    supported: &[wgpu::PresentMode],
) -> wgpu::PresentMode {
    match requested {
        wgpu::PresentMode::AutoVsync | wgpu::PresentMode::AutoNoVsync => requested,
        explicit if supported.contains(&explicit) => explicit,
        wgpu::PresentMode::Fifo | wgpu::PresentMode::FifoRelaxed => wgpu::PresentMode::AutoVsync,
        wgpu::PresentMode::Immediate | wgpu::PresentMode::Mailbox => wgpu::PresentMode::AutoNoVsync,
    }
}

#[cfg(test)]
mod tests {
    use wgpu::PresentMode;

    use crate::host::winit::gpu::negotiate_present_mode;

    #[derive(Debug)]
    struct PresentModeCase {
        requested: PresentMode,
        supported: Vec<PresentMode>,
        expected: PresentMode,
    }

    #[test]
    fn present_mode_negotiation_preserves_supported_modes_and_policy() {
        let cases = [
            PresentModeCase {
                requested: PresentMode::AutoVsync,
                supported: vec![],
                expected: PresentMode::AutoVsync,
            },
            PresentModeCase {
                requested: PresentMode::AutoNoVsync,
                supported: vec![PresentMode::Fifo],
                expected: PresentMode::AutoNoVsync,
            },
            PresentModeCase {
                requested: PresentMode::Fifo,
                supported: vec![PresentMode::Fifo],
                expected: PresentMode::Fifo,
            },
            PresentModeCase {
                requested: PresentMode::FifoRelaxed,
                supported: vec![PresentMode::Fifo, PresentMode::FifoRelaxed],
                expected: PresentMode::FifoRelaxed,
            },
            PresentModeCase {
                requested: PresentMode::Immediate,
                supported: vec![PresentMode::Immediate],
                expected: PresentMode::Immediate,
            },
            PresentModeCase {
                requested: PresentMode::Mailbox,
                supported: vec![PresentMode::Mailbox],
                expected: PresentMode::Mailbox,
            },
            PresentModeCase {
                requested: PresentMode::Fifo,
                supported: vec![],
                expected: PresentMode::AutoVsync,
            },
            PresentModeCase {
                requested: PresentMode::FifoRelaxed,
                supported: vec![PresentMode::Fifo],
                expected: PresentMode::AutoVsync,
            },
            PresentModeCase {
                requested: PresentMode::Immediate,
                supported: vec![PresentMode::Fifo],
                expected: PresentMode::AutoNoVsync,
            },
            PresentModeCase {
                requested: PresentMode::Mailbox,
                supported: vec![PresentMode::Fifo],
                expected: PresentMode::AutoNoVsync,
            },
        ];

        for case in cases {
            assert_eq!(
                negotiate_present_mode(case.requested, &case.supported),
                case.expected,
                "{case:?}"
            );
        }
    }

    #[test]
    fn present_mode_is_negotiated_independently_for_each_surface() {
        let requested = PresentMode::Mailbox;
        let bootstrap_mode =
            negotiate_present_mode(requested, &[PresentMode::Fifo, PresentMode::Mailbox]);
        let secondary_mode = negotiate_present_mode(requested, &[PresentMode::Fifo]);

        assert_eq!(bootstrap_mode, PresentMode::Mailbox);
        assert_eq!(secondary_mode, PresentMode::AutoNoVsync);
        assert_ne!(bootstrap_mode, secondary_mode);
    }
}
