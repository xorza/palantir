//! Present-mode classification and surface configuration against adapter capabilities.

use wgpu::{
    CompositeAlphaMode, SurfaceCapabilities, SurfaceColorSpaces, SurfaceFormatCapabilities,
    TextureFormat, TextureUsages,
};

use super::*;

/// Every resolvable mode classifies and [`swapchain_mode`] inverts it, so a per-frame `Ui::vsync` write-back reconfigures nothing.
#[test]
fn every_present_mode_classifies_and_the_classification_round_trips() {
    for (mode, expected) in [
        (wgpu::PresentMode::AutoVsync, Vsync::On),
        (wgpu::PresentMode::Fifo, Vsync::On),
        (wgpu::PresentMode::FifoRelaxed, Vsync::On),
        (wgpu::PresentMode::AutoNoVsync, Vsync::Off),
        (wgpu::PresentMode::Immediate, Vsync::Off),
        (wgpu::PresentMode::Mailbox, Vsync::Off),
    ] {
        assert_eq!(vsync_of(mode), expected, "{mode:?}");
    }
    for vsync in [Vsync::On, Vsync::Off] {
        assert_eq!(vsync_of(swapchain_mode(vsync)), vsync, "{vsync:?}");
    }
}

#[test]
fn both_vsync_states_map_to_automatic_policies() {
    assert_eq!(swapchain_mode(Vsync::On), wgpu::PresentMode::AutoVsync);
    assert_eq!(swapchain_mode(Vsync::Off), wgpu::PresentMode::AutoNoVsync);
    assert_eq!(Vsync::default(), Vsync::On, "vsync is on unless asked off");
}

fn compatible_caps() -> SurfaceCapabilities {
    let format = TextureFormat::Bgra8UnormSrgb;
    SurfaceCapabilities {
        formats: vec![format],
        format_capabilities: vec![SurfaceFormatCapabilities {
            format,
            color_spaces: SurfaceColorSpaces::SRGB,
        }],
        present_modes: vec![wgpu::PresentMode::Fifo],
        alpha_modes: vec![CompositeAlphaMode::Opaque],
        usages: REQUIRED_SURFACE_USAGES | OPTIONAL_SURFACE_USAGES,
    }
}

#[test]
fn surface_config_enforces_renderer_contract_and_clamps_dimensions() {
    let max_texture_dim = NonZeroU32::new(4096).unwrap();
    let size = SurfaceManager::clamp_extent(max_texture_dim, UVec2::new(0, u32::MAX));
    assert_eq!(size, UVec2::new(1, 4096));

    let config = build_surface_config(&compatible_caps(), size, Vsync::Off).unwrap();

    assert_eq!(
        config.usage,
        REQUIRED_SURFACE_USAGES | OPTIONAL_SURFACE_USAGES,
        "a surface that offers the copy is configured with it"
    );
    assert_eq!(config.format, TextureFormat::Bgra8UnormSrgb);
    assert_eq!(config.color_space, wgpu::SurfaceColorSpace::Srgb);
    assert_eq!(config.width, 1);
    assert_eq!(config.height, 4096);
    assert_eq!(config.present_mode, wgpu::PresentMode::AutoNoVsync);
    assert_eq!(config.alpha_mode, CompositeAlphaMode::Opaque);
    assert_eq!(config.desired_maximum_frame_latency, 1);

    let synced = build_surface_config(&compatible_caps(), size, Vsync::On).unwrap();
    assert_eq!(synced.present_mode, wgpu::PresentMode::AutoVsync);
    assert_ne!(synced.present_mode, config.present_mode);
}

/// A GLES swapchain advertises only the attachment usage, so the copy usage is negotiated and its absence is no error.
#[test]
fn a_surface_without_the_copy_usage_is_configured_without_it() {
    let mut draw_only = compatible_caps();
    draw_only.usages = REQUIRED_SURFACE_USAGES;

    let config = build_surface_config(&draw_only, UVec2::splat(100), Vsync::On)
        .expect("a surface that can be drawn into is usable");

    assert_eq!(config.usage, REQUIRED_SURFACE_USAGES);
    assert!(
        !config.usage.contains(TextureUsages::COPY_DST),
        "asking for a usage the surface does not have is what failed the window"
    );
}

#[test]
fn surface_config_rejects_each_missing_hard_capability() {
    let mut incompatible = compatible_caps();
    incompatible.formats.clear();
    assert!(matches!(
        build_surface_config(&incompatible, UVec2::splat(100), Vsync::On),
        Err(SurfaceError::Incompatible)
    ));

    let mut no_alpha_mode = compatible_caps();
    no_alpha_mode.alpha_modes.clear();
    assert!(matches!(
        build_surface_config(&no_alpha_mode, UVec2::splat(100), Vsync::On),
        Err(SurfaceError::Incompatible)
    ));

    let mut no_srgb = compatible_caps();
    no_srgb.formats = vec![TextureFormat::Bgra8Unorm];
    no_srgb.format_capabilities = vec![SurfaceFormatCapabilities {
        format: TextureFormat::Bgra8Unorm,
        color_spaces: SurfaceColorSpaces::SRGB,
    }];
    assert!(matches!(
        build_surface_config(&no_srgb, UVec2::splat(100), Vsync::On),
        Err(SurfaceError::MissingSrgb)
    ));

    let mut no_attachment = compatible_caps();
    no_attachment.usages = TextureUsages::COPY_DST;
    let unmet = build_surface_config(&no_attachment, UVec2::splat(100), Vsync::On).unwrap_err();
    let SurfaceError::MissingUsages { missing } = &unmet else {
        panic!("{unmet:?}");
    };
    assert!(
        missing.contains("RENDER_ATTACHMENT") && !missing.contains("COPY_DST"),
        "the report must name the usage the surface lacks, not the one it has: {missing}"
    );
}
