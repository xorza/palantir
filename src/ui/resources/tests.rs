use crate::common::clipboard::Clipboard;
use crate::primitives::image::Image;
use crate::primitives::texture_id::TextureId;
use crate::renderer::texture_limit::TextureLimit;
use crate::text::shaper::TextShaper;
use crate::ui::Ui;
use crate::ui::resources::UiResources;
use std::num::NonZeroU32;

/// The bundle's ceiling is the one `Ui::register_image` enforces and
/// the one it reports, and a rejection stops before the registry — so
/// it queues no upload and consumes no id.
#[test]
fn the_bundle_ceiling_gates_registration_and_is_what_ui_reports() {
    let resources = UiResources::new(
        TextShaper::test_mono(),
        Clipboard::default(),
        TextureLimit::from_device(NonZeroU32::new(4).unwrap()),
    );
    let images = resources.images.clone();
    let ui = Ui::new(resources);
    assert_eq!(ui.max_image_dimension(), NonZeroU32::new(4));

    let accepted = ui.register_image(img(4, 4)).unwrap();
    assert_eq!(accepted.id(), TextureId(1));
    assert_eq!(
        ui.register_image(img(5, 1)).unwrap_err().max_dimension,
        4,
        "an over-limit source is rejected against the bundle's ceiling",
    );
    let next = ui.register_image(img(1, 1)).unwrap();
    assert_eq!(next.id(), TextureId(2), "a rejection consumes no id");

    let mut uploaded = Vec::new();
    images.drain_pending(|id, _| uploaded.push(id));
    assert_eq!(
        uploaded,
        vec![accepted.id(), next.id()],
        "and queues no upload",
    );
}

fn img(w: u32, h: u32) -> Image {
    Image::from_rgba8(w, h, vec![0u8; (w * h * 4) as usize])
}

#[test]
fn images_and_gpu_views_share_one_texture_id_authority() {
    let resources = UiResources::isolated_mono();
    let gpu_view_id = resources.texture_ids.reserve();
    let image = Image::from_rgba8(1, 1, vec![0, 0, 0, 0]);
    let image_id = resources.images.register(image).id();

    assert_eq!(gpu_view_id, TextureId(1));
    assert_eq!(image_id, TextureId(2));
}
