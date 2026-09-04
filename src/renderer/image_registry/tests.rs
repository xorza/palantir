use crate::host::test_gpu;
use crate::primitives::color::srgba_u8::SrgbaU8;
use crate::primitives::image::Image;
use crate::primitives::texture_id::TextureId;
use crate::renderer::image_registry::ImageRegistry;
use crate::renderer::image_registry::image_handle::ImageHandle;
use glam::UVec2;
use std::rc::Rc;

#[test]
fn a_gpu_texture_lives_exactly_as_long_as_its_handle() {
    let gpu = test_gpu::headless_test_gpu();
    let registry = ImageRegistry::new(gpu.device.clone(), gpu.queue.clone());
    let gpu_store = Rc::downgrade(registry.gpu.as_ref().unwrap());
    let shared = registry.clone();
    let mut image = Image::blank(UVec2::splat(2));
    let handle = ImageHandle::new(TextureId(1), &image, registry);
    assert_eq!(shared.resident(), 1);
    image.texels_mut().fill(SrgbaU8::hex(0x4cd3ff));
    handle.update(&image);
    assert_eq!(handle.generation(), 1);
    let clone = handle.clone();
    drop(handle);
    assert_eq!(shared.resident(), 1, "a clone keeps the texture");
    drop(clone);
    assert_eq!(shared.resident(), 0);

    let survivor = ImageHandle::new(TextureId(2), &image, shared);
    survivor.update(&image);
    assert_eq!(survivor.generation(), 1);
    assert_eq!(gpu_store.upgrade().unwrap().borrow().resident(), 1);
    drop(survivor);
    assert!(
        gpu_store.upgrade().is_none(),
        "the last image handle releases a store whose other owners are gone",
    );
}
