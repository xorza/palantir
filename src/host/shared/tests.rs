use glam::UVec2;
use std::num::NonZeroU32;

use crate::diagnostics::DebugOverlayConfig;
use crate::host::shared::HostShared;
use crate::primitives::image::Image;
use crate::primitives::texture_id::TextureId;
use crate::renderer::texture_limit::TextureLimit;
use crate::text::shaper::TextShaper;

#[test]
fn diagnostics_are_shared_across_capability_bundles() {
    let shared = HostShared::new(TextShaper::test_mono(), TextureLimit::default());
    let ui = shared.resources.clone();
    assert_eq!(
        shared.resources.diagnostics.overlay.get(),
        DebugOverlayConfig::default()
    );

    ui.diagnostics.overlay.set(DebugOverlayConfig {
        damage_rect: true,
        ..DebugOverlayConfig::default()
    });

    assert!(shared.resources.diagnostics.overlay.get().damage_rect);
    assert!(ui.diagnostics.overlay.get().damage_rect);
    assert!(
        ui.diagnostics.overlay.take_change(),
        "the write raises the host's repaint signal",
    );
    assert!(
        !shared.resources.diagnostics.overlay.take_change(),
        "and the ask lowers it, for the one host that shares the flags",
    );
}

#[test]
fn backend_and_ui_share_text_images_and_gpu_stats() {
    let limit = TextureLimit::from_device(NonZeroU32::new(1).unwrap());
    let shared = HostShared::new(TextShaper::test_mono(), limit);
    let ui = shared.resources.clone();
    let backend = shared.backend_resources();

    assert!(ui.text.shares_cache_with(&backend.text));
    assert_eq!(
        ui.texture_limit, limit,
        "the recorder bundle carries the ceiling the host was built with",
    );
    let image = ui.images.register(&Image::blank(UVec2::ONE));
    assert_eq!(
        backend.images.register(&Image::blank(UVec2::ONE)).id(),
        TextureId(image.id().0 + 1),
        "one id authority behind both handles",
    );
    backend.gpu_pass_stats.record_pass_ns(2_500_000);
    assert_eq!(ui.diagnostics.gpu_pass_stats.last_pass_ms(), Some(2.5));
}

#[test]
fn clipboard_is_shared_within_one_host_and_isolated_between_hosts() {
    let first = HostShared::new(TextShaper::test_mono(), TextureLimit::default());
    let first_window = first.resources.clone();
    let second_window = first.resources.clone();
    let second = HostShared::new(TextShaper::test_mono(), TextureLimit::default()).resources;

    first_window.clipboard.set("shared").unwrap();

    assert_eq!(second_window.clipboard.get().unwrap(), "shared");
    assert_eq!(second.clipboard.get().unwrap(), "");
}
