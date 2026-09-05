use crate::common::clipboard::Clipboard;
use crate::diagnostics::DebugOverlayConfig;
use crate::primitives::image::Image;
use crate::primitives::texture_id::TextureId;
use crate::renderer::texture_limit::TextureLimit;
use crate::text::shaper::TextShaper;
use crate::ui::Ui;
use crate::ui::resources::UiResources;
use glam::UVec2;
use std::num::NonZeroU32;

#[test]
fn the_bundle_ceiling_gates_registration_and_is_what_ui_reports() {
    let limit = TextureLimit::from_device(NonZeroU32::new(4).unwrap());
    let resources = UiResources::new(TextShaper::test_mono(), Clipboard::default(), limit);
    assert_eq!(resources.texture_limit(), limit);
    let ui = Ui::new(resources);
    assert_eq!(ui.max_image_dimension(), NonZeroU32::new(4));

    let accepted = ui.register_image(&img(4, 4)).unwrap();
    assert_eq!(
        ui.register_image(&img(5, 1)).unwrap_err().max_dimension,
        4,
        "an over-limit source is rejected against the bundle's ceiling",
    );
    let next = ui.register_image(&img(1, 1)).unwrap();
    assert!(
        next.id().0 > accepted.id().0,
        "a registration after a rejection still gets a fresh id",
    );
}

fn img(w: u32, h: u32) -> Image {
    Image::from_rgba8(w, h, vec![0u8; (w * h * 4) as usize])
}

/// The sequence is process-wide, so the ids are only ever compared
/// against each other: another test registering an image on another
/// thread lands between any two reserves here, which is exactly the
/// interleaving the one sequence exists to survive.
#[test]
fn no_two_texture_ids_repeat_across_hosts_or_kinds() {
    let host = UiResources::isolated_mono();
    let window = host.clone();
    let elsewhere = UiResources::isolated_mono();

    let gpu_view = TextureId::reserve();
    let first = host.register_image(&img(2, 3)).unwrap();
    let second = window.register_image(&img(4, 5)).unwrap();
    let foreign = elsewhere.register_image(&img(6, 7)).unwrap();

    let ids = [gpu_view, first.id(), second.id(), foreign.id()];
    assert!(
        ids.windows(2).all(|pair| pair[0].0 < pair[1].0),
        "one sequence, whichever host or kind asked: {ids:?}",
    );
    assert_eq!(first.size(), UVec2::new(2, 3));
    assert_eq!(second.size(), UVec2::new(4, 5));
    assert_eq!(foreign.size(), UVec2::new(6, 7));
}

#[test]
fn dimensions_above_u16_are_preserved_without_a_gpu() {
    const WIDTH: u32 = u16::MAX as u32 + 1;
    let resources = UiResources::isolated_mono();
    let handle = resources.register_image(&img(WIDTH, 1)).unwrap();
    assert_eq!(handle.size(), UVec2::new(WIDTH, 1));
}

#[test]
fn diagnostics_are_shared_across_clones() {
    let host = UiResources::isolated_mono();
    let ui = host.clone();
    assert_eq!(
        host.diagnostics().overlay.get(),
        DebugOverlayConfig::default()
    );

    ui.diagnostics().overlay.set(DebugOverlayConfig {
        damage_rect: true,
        ..DebugOverlayConfig::default()
    });

    assert!(host.diagnostics().overlay.get().damage_rect);
    assert!(ui.diagnostics().overlay.get().damage_rect);
    assert!(
        ui.diagnostics().overlay.take_change(),
        "the write raises the host's repaint signal",
    );
    assert!(
        !host.diagnostics().overlay.take_change(),
        "and the ask lowers it, for the one host that shares the flags",
    );
}

#[test]
fn clipboard_is_shared_within_one_host_and_isolated_between_hosts() {
    let first = UiResources::isolated_mono();
    let first_window = first.clone();
    let second_window = first.clone();
    let second = UiResources::isolated_mono();

    first_window.clipboard().set("shared").unwrap();

    assert_eq!(second_window.clipboard().get().unwrap(), "shared");
    assert_eq!(second.clipboard().get().unwrap(), "");
}
