//! Present-mode classification and surface configuration against adapter capabilities.

use wgpu::{
    CompositeAlphaMode, SurfaceCapabilities, SurfaceColorSpaces, SurfaceFormatCapabilities,
    TextureFormat, TextureUsages,
};

use super::*;

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
fn surface_config_enforces_renderer_contract() {
    let size = UVec2::new(1, 4096);

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
    assert_eq!(
        *missing,
        TextureUsages::RENDER_ATTACHMENT,
        "the usage the surface lacks, not the one it has"
    );
    assert_eq!(
        unmet.to_string(),
        "window surface cannot do RENDER_ATTACHMENT, which Palantir's compositor needs"
    );
}
