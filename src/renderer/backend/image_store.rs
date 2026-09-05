//! The GPU side of registered images: their textures and bind groups,
//! created, rewritten and freed the moment the registry asks.

use crate::primitives::image::Image;
use crate::primitives::texture_id::TextureId;
use crate::renderer::backend::image_binding::ImageBinding;
use crate::renderer::backend::texture_region::TextureRegion;
use crate::renderer::image_registry::image_store::ImageStore;
use glam::UVec2;
use rustc_hash::FxHashMap;
use std::cell::{Ref, RefCell};

/// The [`ImageStore`] a host's registry writes through and its backend
/// draws from. The two hold one `Rc` between them, which is why the map
/// sits behind a `RefCell`: a handle creates, rewrites or frees through a
/// shared reference between frames, and the draw takes one [`Self::read`]
/// for a whole pass. Only a queue drained by the backend under `&mut self`
/// would remove the cell, and that is the staged-upload design this
/// immediate one replaced.
#[derive(Debug)]
pub(super) struct WgpuImageStore {
    device: wgpu::Device,
    queue: wgpu::Queue,
    /// The group-0 layout and sampler every image bind group is built
    /// against. The `GpuView` targets clone it, so a composite of a view
    /// binds exactly like an image, and each format's image pipeline
    /// composes over its layout.
    binding: ImageBinding,
    textures: RefCell<FxHashMap<TextureId, ImageTexture>>,
}

#[derive(Debug)]
pub(super) struct ImageTexture {
    texture: wgpu::Texture,
    pub(super) bind_group: wgpu::BindGroup,
}

impl WgpuImageStore {
    pub(super) fn new(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        Self {
            binding: ImageBinding::new(&device),
            device,
            queue,
            textures: RefCell::default(),
        }
    }

    pub(super) fn binding(&self) -> &ImageBinding {
        &self.binding
    }

    /// One borrow for a render traversal, so a draw pays neither a
    /// `RefCell` probe nor a handle clone per run.
    pub(super) fn read(&self) -> Ref<'_, FxHashMap<TextureId, ImageTexture>> {
        self.textures.borrow()
    }

    /// An empty texture at `size` and the bind group a draw samples it
    /// through. The texels follow in the write that asked for it.
    fn create(&self, id: TextureId, size: UVec2) -> ImageTexture {
        let raw_id = id.0;
        let texture_label = format!("palantir.image.tex.{raw_id:016x}");
        let bind_group_label = format!("palantir.image.tex.bg.{raw_id:016x}");
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(&texture_label),
            size: wgpu::Extent3d {
                width: size.x,
                height: size.y,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let bind_group = self
            .binding
            .bind_group(&self.device, &view, &bind_group_label);
        ImageTexture {
            texture,
            bind_group,
        }
    }
}

impl ImageStore for WgpuImageStore {
    fn write(&self, id: TextureId, image: &Image) {
        let mut textures = self.textures.borrow_mut();
        let entry = textures
            .entry(id)
            .or_insert_with(|| self.create(id, image.size));
        debug_assert_eq!(
            UVec2::new(entry.texture.width(), entry.texture.height()),
            image.size,
            "a write matches the texture it lands in",
        );
        TextureRegion {
            texture: &entry.texture,
            first_row: 0,
            size: image.size,
            bytes_per_row: image.size.x * 4,
        }
        .write(&self.queue, &image.pixels);
    }

    fn free(&self, id: TextureId) {
        self.textures.borrow_mut().remove(&id);
    }
}

#[cfg(any(test, feature = "internals"))]
pub(crate) mod test_support {
    use crate::renderer::backend::image_store::WgpuImageStore;

    impl WgpuImageStore {
        /// Registered images resident on the GPU. The surface-format-change
        /// tests assert this survives a pipeline rebuild.
        pub(crate) fn resident(&self) -> usize {
            self.textures.borrow().len()
        }
    }
}

#[cfg(all(test, feature = "internals"))]
mod tests {
    use crate::host::test_gpu;
    use crate::primitives::color::srgba_u8::SrgbaU8;
    use crate::primitives::image::Image;
    use crate::primitives::texture_id::TextureId;
    use crate::renderer::backend::image_store::WgpuImageStore;
    use crate::renderer::image_registry::ImageRegistry;
    use crate::renderer::image_registry::image_handle::ImageHandle;
    use glam::UVec2;
    use std::rc::Rc;

    #[test]
    fn a_gpu_texture_lives_exactly_as_long_as_its_handle() {
        let gpu = test_gpu::headless_test_gpu();
        let store = Rc::new(WgpuImageStore::new(gpu.device.clone(), gpu.queue.clone()));
        let weak = Rc::downgrade(&store);
        let registry = ImageRegistry::default();
        registry.attach(Rc::clone(&store));
        assert!(!store.read().contains_key(&TextureId(1)));

        let mut image = Image::blank(UVec2::splat(2));
        let handle = ImageHandle::new(TextureId(1), &image, registry.clone());
        assert_eq!(store.resident(), 1);
        let registered = store.read()[&TextureId(1)].bind_group.clone();
        assert!(!store.read().contains_key(&TextureId(2)));
        image.texels_mut().fill(SrgbaU8::hex(0x4cd3ff));
        handle.update(&image);
        assert_eq!(handle.generation(), 1);
        assert_eq!(
            store.read()[&TextureId(1)].bind_group,
            registered,
            "a write keeps the texture and its binding",
        );
        let clone = handle.clone();
        drop(handle);
        assert_eq!(store.resident(), 1, "a clone keeps the texture");
        drop(clone);
        assert_eq!(store.resident(), 0);
        assert!(!store.read().contains_key(&TextureId(1)));

        let survivor = ImageHandle::new(TextureId(2), &image, registry);
        drop(store);
        survivor.update(&image);
        assert_eq!(survivor.generation(), 1);
        assert_eq!(weak.upgrade().unwrap().resident(), 1);
        drop(survivor);
        assert!(
            weak.upgrade().is_none(),
            "the last image handle releases a store whose other owners are gone",
        );
    }
}
