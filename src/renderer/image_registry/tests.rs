use crate::host::test_gpu::headless_test_gpu;
use crate::primitives::color::srgba_u8::SrgbaU8;
use crate::primitives::image::Image;
use crate::primitives::texture_id::TextureId;
use crate::renderer::backend::image_binding::ImageBinding;
use crate::renderer::backend::image_gpu::ImageGpu;
use crate::renderer::image_registry::ImageRegistry;
use crate::renderer::texture_id_source::TextureIdSource;
use glam::UVec2;

fn reg() -> ImageRegistry {
    ImageRegistry::new(TextureIdSource::default())
}

fn img(w: u32, h: u32) -> Image {
    Image::blank(UVec2::new(w, h))
}

#[test]
fn register_mints_unique_ids_and_keeps_the_size() {
    let reg = reg();
    let a = reg.register(&img(2, 3));
    let b = reg.register(&img(4, 5));
    assert_ne!(a.id(), b.id());
    assert_ne!(a.id().0, 0);
    assert_eq!(a.size(), UVec2::new(2, 3));
    assert_eq!(b.size(), UVec2::new(4, 5));
}

#[test]
fn dimensions_above_u16_are_preserved() {
    const WIDTH: u32 = u16::MAX as u32 + 1;
    let handle = reg().register(&img(WIDTH, 1));
    assert_eq!(handle.size(), UVec2::new(WIDTH, 1));
}

/// A 0×0 image is a logic error caught at construction — before it
/// can reach `register` and blow up in the GPU upload.
#[test]
#[should_panic(expected = "RGBA8 dimensions must be non-zero")]
fn zero_sized_image_panics_at_construction() {
    let _ = img(0, 0);
}

#[test]
fn ids_are_minted_in_registration_order() {
    let reg = reg();
    assert_eq!(reg.register(&img(1, 1)).id(), TextureId(1));
    assert_eq!(reg.register(&img(1, 1)).id(), TextureId(2));
}

/// Every update moves the generation the shape hash reads, so a rewritten
/// texture repaints under its unchanged id.
#[test]
fn every_update_bumps_the_generation() {
    let image = img(1, 1);
    let h = reg().register(&image);
    let start = h.generation();
    h.update(&image);
    h.update(&image);
    assert_eq!(h.generation(), start.wrapping_add(2));
}

#[test]
#[should_panic(expected = "an image update must match the registered size")]
fn an_update_of_another_size_panics() {
    let h = reg().register(&img(2, 2));
    h.update(&img(2, 3));
}

/// With a backend attached, a registration is a resident texture at once,
/// an update lands, and the last handle's drop frees it.
#[test]
fn a_gpu_texture_lives_exactly_as_long_as_its_handle() {
    let gpu = headless_test_gpu();
    let reg = reg();
    reg.attach(ImageGpu::new(
        gpu.device.clone(),
        gpu.queue.clone(),
        ImageBinding::new(&gpu.device),
    ));
    let mut image = img(2, 2);
    let h = reg.register(&image);
    assert_eq!(reg.gpu().resident(), 1);
    image.texels_mut().fill(SrgbaU8::hex(0x4cd3ff));
    h.update(&image);
    assert_eq!(h.generation(), 1);
    let clone = h.clone();
    drop(h);
    assert_eq!(reg.gpu().resident(), 1, "a clone keeps the texture");
    drop(clone);
    assert_eq!(reg.gpu().resident(), 0);
}
