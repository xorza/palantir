use std::num::NonZeroU32;

use glam::UVec2;
use wgpu::{
    CompositeAlphaMode, PresentMode, SurfaceCapabilities, SurfaceColorSpaces,
    SurfaceFormatCapabilities, TextureFormat, TextureUsages,
};

use crate::host::winit::error::WinitHostError;
use crate::host::winit::gpu::{
    REQUIRED_SURFACE_USAGES, build_surface_config, negotiate_present_mode, present_mode, vsync_of,
};
use crate::window::vsync::Vsync;

/// `Window::set_vsync` compares in [`Vsync`]'s vocabulary rather than
/// wgpu's, and this is why it can: every present mode classifies, and
/// `present_mode` is a right inverse of the classification. So a
/// swapchain opened on an explicit mode reports the state it actually
/// paces like, and a recorder writing that same state back — which is
/// what a checkbox bound to `Ui::vsync` does every frame — leaves the
/// explicit choice standing instead of flattening it to an automatic one.
#[test]
fn every_present_mode_classifies_and_the_classification_round_trips() {
    for (mode, expected) in [
        (PresentMode::AutoVsync, Vsync::On),
        (PresentMode::Fifo, Vsync::On),
        (PresentMode::FifoRelaxed, Vsync::On),
        (PresentMode::AutoNoVsync, Vsync::Off),
        (PresentMode::Immediate, Vsync::Off),
        (PresentMode::Mailbox, Vsync::Off),
    ] {
        assert_eq!(vsync_of(mode), expected, "{mode:?}");
    }
    for vsync in [Vsync::On, Vsync::Off] {
        assert_eq!(vsync_of(present_mode(vsync)), vsync, "{vsync:?}");
    }
}

/// The runtime vsync toggle maps onto *automatic* policies on purpose:
/// every surface accepts those, so switching a live swapchain needs no
/// capability re-query — which is what lets `Window::set_present_mode`
/// assign straight into the config.
#[test]
fn vsync_maps_to_automatic_present_modes_that_survive_negotiation() {
    assert_eq!(present_mode(Vsync::On), PresentMode::AutoVsync);
    assert_eq!(present_mode(Vsync::Off), PresentMode::AutoNoVsync);
    assert_eq!(Vsync::default(), Vsync::On, "vsync is on unless asked off");

    // `[]` is the worst case a surface can report. An explicit mode would
    // be rewritten here; both automatic ones pass through untouched.
    for vsync in [Vsync::On, Vsync::Off] {
        let mode = present_mode(vsync);
        assert_eq!(
            negotiate_present_mode(mode, &[]),
            mode,
            "{vsync:?} must not depend on what the surface enumerates"
        );
    }
}

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
