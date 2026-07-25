//! Wgpu startup and the retained [`SurfaceManager`] used to create, configure,
//! and present native-window surfaces.

use std::num::NonZeroU32;
use std::sync::Arc;

use glam::UVec2;
use winit::window::{Window as WinitWindow, WindowId};

use crate::host::winit::config::WinitHostConfig;
use crate::host::winit::error::WinitHostError;

const REQUIRED_IMMEDIATE_SIZE: u32 = 16;
const REQUIRED_SURFACE_USAGES: wgpu::TextureUsages =
    wgpu::TextureUsages::RENDER_ATTACHMENT.union(wgpu::TextureUsages::COPY_DST);

/// Native-surface authority retained after startup. The cloned device/queue
/// handles refer to the same GPU objects owned by `WgpuBackend`.
#[derive(Debug)]
pub(super) struct SurfaceManager {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    /// `max_texture_dimension_2d` granted at device creation — fixed for
    /// the device's lifetime, cached so the host's per-event resize clamp
    /// doesn't re-query `device.limits()`.
    pub(super) max_texture_dim: NonZeroU32,
    /// App-global presentation policy requested through `WinitHostConfig`.
    /// Each surface negotiates it against its own capabilities.
    requested_present_mode: wgpu::PresentMode,
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
    pub(super) device: wgpu::Device,
    pub(super) queue: wgpu::Queue,
    pub(super) first_surface: WindowSurface,
}

#[derive(Debug)]
struct DeviceRequirements {
    features: wgpu::Features,
    limits: wgpu::Limits,
}

impl GpuInit {
    /// Pick the shared adapter/device and create the first native surface.
    pub(super) fn new(
        window: &Arc<WinitWindow>,
        cfg: &WinitHostConfig,
    ) -> Result<Self, WinitHostError> {
        if wgpu::Instance::enabled_backend_features().is_empty() {
            return Err(WinitHostError::NoGpuBackend);
        }
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.flags = desc.flags.with_env();
        let instance = wgpu::Instance::new(desc);
        let surface = instance
            .create_surface(window.clone())
            .map_err(|source| WinitHostError::CreateSurface { source })?;

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: cfg.power_preference,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .map_err(|source| WinitHostError::RequestAdapter { source })?;

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
        let requirements =
            device_requirements(adapter.features(), adapter.limits(), timing_features)?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("aperture.device"),
            required_features: requirements.features,
            required_limits: requirements.limits,
            experimental_features: wgpu::ExperimentalFeatures::default(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        }))
        .map_err(|source| WinitHostError::RequestDevice { source })?;

        let max_texture_dim = NonZeroU32::new(device.limits().max_texture_dimension_2d)
            .expect("device texture dimension limit is zero");
        let surfaces = SurfaceManager {
            instance,
            adapter,
            device: device.clone(),
            queue: queue.clone(),
            max_texture_dim,
            requested_present_mode: cfg.present_mode,
        };
        let size = window.inner_size();
        let first_surface = surfaces.build_window_surface(
            surface,
            UVec2::new(size.width, size.height),
            window.id(),
        )?;
        Ok(Self {
            surfaces,
            device,
            queue,
            first_surface,
        })
    }
}

fn device_requirements(
    supported_features: wgpu::Features,
    supported_limits: wgpu::Limits,
    optional_features: wgpu::Features,
) -> Result<DeviceRequirements, WinitHostError> {
    let required = wgpu::Features::IMMEDIATES;
    if !supported_features.contains(required) {
        return Err(WinitHostError::MissingAdapterFeatures {
            required,
            supported: supported_features,
        });
    }

    let features = required | (supported_features & optional_features);
    let mut limits = wgpu::Limits::default().using_resolution(supported_limits.clone());
    // This covers `renderer::backend::text::Params` (vec2<u32>) with
    // WGSL's 16-byte uniform-struct rounding.
    limits.max_immediate_size = limits.max_immediate_size.max(REQUIRED_IMMEDIATE_SIZE);
    let mut failed_limit = None;
    limits.check_limits_with_fail_fn(&supported_limits, true, |name, required, supported| {
        failed_limit = Some(WinitHostError::UnsupportedAdapterLimit {
            name,
            required,
            supported,
        });
    });
    if let Some(error) = failed_limit {
        return Err(error);
    }

    Ok(DeviceRequirements { features, limits })
}

impl SurfaceManager {
    /// Create a surface for an additional window against the selected adapter.
    pub(super) fn make_surface(
        &self,
        window: &Arc<WinitWindow>,
    ) -> Result<WindowSurface, WinitHostError> {
        let surface = self
            .instance
            .create_surface(window.clone())
            .map_err(|source| WinitHostError::CreateSurface { source })?;
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
            size,
            self.max_texture_dim,
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
    max_texture_dim: NonZeroU32,
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
    let max_texture_dim = max_texture_dim.get();
    Ok(wgpu::SurfaceConfiguration {
        usage: REQUIRED_SURFACE_USAGES,
        format,
        color_space: wgpu::SurfaceColorSpace::Srgb,
        width: size.x.clamp(1, max_texture_dim),
        height: size.y.clamp(1, max_texture_dim),
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
    use std::num::NonZeroU32;

    use glam::UVec2;
    use wgpu::{
        CompositeAlphaMode, Features, PresentMode, SurfaceCapabilities, SurfaceColorSpaces,
        SurfaceFormatCapabilities, TextureFormat, TextureUsages,
    };

    use crate::host::winit::error::WinitHostError;
    use crate::host::winit::gpu::{
        REQUIRED_IMMEDIATE_SIZE, REQUIRED_SURFACE_USAGES, build_surface_config,
        device_requirements, negotiate_present_mode,
    };

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

    #[test]
    fn device_requirements_validate_hard_capabilities_and_degrade_optional_features() {
        let supported_limits = wgpu::Limits {
            max_immediate_size: REQUIRED_IMMEDIATE_SIZE,
            ..wgpu::Limits::default()
        };
        let supported_features = Features::IMMEDIATES | Features::TIMESTAMP_QUERY;
        let optional = Features::TIMESTAMP_QUERY | Features::PIPELINE_STATISTICS_QUERY;

        let requirements =
            device_requirements(supported_features, supported_limits.clone(), optional).unwrap();
        assert_eq!(
            requirements.features,
            Features::IMMEDIATES | Features::TIMESTAMP_QUERY
        );
        assert_eq!(
            requirements.limits.max_immediate_size,
            REQUIRED_IMMEDIATE_SIZE
        );

        let missing_feature =
            device_requirements(Features::empty(), supported_limits.clone(), optional).unwrap_err();
        assert!(matches!(
            missing_feature,
            WinitHostError::MissingAdapterFeatures {
                required,
                supported,
            } if required == Features::IMMEDIATES && supported.is_empty()
        ));

        let mut insufficient_limits = supported_limits;
        insufficient_limits.max_immediate_size = REQUIRED_IMMEDIATE_SIZE - 1;
        let missing_limit =
            device_requirements(Features::IMMEDIATES, insufficient_limits, Features::empty())
                .unwrap_err();
        assert!(matches!(
            missing_limit,
            WinitHostError::UnsupportedAdapterLimit {
                name: "max_immediate_size",
                required: 16,
                supported: 15,
            }
        ));
    }

    fn compatible_caps() -> SurfaceCapabilities {
        let format = TextureFormat::Bgra8UnormSrgb;
        SurfaceCapabilities {
            formats: vec![format],
            format_capabilities: vec![SurfaceFormatCapabilities {
                format,
                color_spaces: SurfaceColorSpaces::SRGB,
            }],
            present_modes: vec![PresentMode::Fifo],
            alpha_modes: vec![CompositeAlphaMode::Opaque],
            usages: REQUIRED_SURFACE_USAGES,
        }
    }

    #[test]
    fn surface_config_enforces_renderer_contract_and_clamps_dimensions() {
        let max_texture_dim = NonZeroU32::new(4096).unwrap();
        let config = build_surface_config(
            &compatible_caps(),
            UVec2::new(0, u32::MAX),
            max_texture_dim,
            PresentMode::Mailbox,
        )
        .unwrap();

        assert_eq!(config.usage, REQUIRED_SURFACE_USAGES);
        assert_eq!(config.format, TextureFormat::Bgra8UnormSrgb);
        assert_eq!(config.color_space, wgpu::SurfaceColorSpace::Srgb);
        assert_eq!(config.width, 1);
        assert_eq!(config.height, 4096);
        assert_eq!(config.present_mode, PresentMode::AutoNoVsync);
        assert_eq!(config.alpha_mode, CompositeAlphaMode::Opaque);
        assert_eq!(config.desired_maximum_frame_latency, 1);
    }

    #[test]
    fn surface_config_rejects_each_missing_hard_capability() {
        let max_texture_dim = NonZeroU32::new(4096).unwrap();

        let mut incompatible = compatible_caps();
        incompatible.formats.clear();
        assert!(matches!(
            build_surface_config(
                &incompatible,
                UVec2::splat(100),
                max_texture_dim,
                PresentMode::Fifo,
            ),
            Err(WinitHostError::IncompatibleSurface)
        ));

        let mut no_alpha_mode = compatible_caps();
        no_alpha_mode.alpha_modes.clear();
        assert!(matches!(
            build_surface_config(
                &no_alpha_mode,
                UVec2::splat(100),
                max_texture_dim,
                PresentMode::Fifo,
            ),
            Err(WinitHostError::IncompatibleSurface)
        ));

        let mut no_srgb = compatible_caps();
        no_srgb.formats = vec![TextureFormat::Bgra8Unorm];
        no_srgb.format_capabilities = vec![SurfaceFormatCapabilities {
            format: TextureFormat::Bgra8Unorm,
            color_spaces: SurfaceColorSpaces::SRGB,
        }];
        assert!(matches!(
            build_surface_config(
                &no_srgb,
                UVec2::splat(100),
                max_texture_dim,
                PresentMode::Fifo,
            ),
            Err(WinitHostError::MissingSrgbSurface)
        ));

        let mut no_copy = compatible_caps();
        no_copy.usages = TextureUsages::RENDER_ATTACHMENT;
        assert!(matches!(
            build_surface_config(
                &no_copy,
                UVec2::splat(100),
                max_texture_dim,
                PresentMode::Fifo,
            ),
            Err(WinitHostError::MissingSurfaceUsages {
                required,
                supported,
            }) if required == REQUIRED_SURFACE_USAGES
                && supported == TextureUsages::RENDER_ATTACHMENT
        ));
    }
}
