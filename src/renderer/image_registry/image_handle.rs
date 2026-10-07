use crate::primitives::identity::texture_id::TextureId;
use crate::primitives::paint::image::Image;
use crate::renderer::image_registry::ImageRegistry;
use glam::UVec2;
use std::cell::Cell;
use std::rc::Rc;

/// RAII owner of a registered image's GPU texture, from [`Ui::load_image`](crate::Ui::load_image); the
/// texture lives until the last clone drops. Reference it from [`Shape::image`](crate::widget::Shape::image)
/// each frame; "no image" is `Option<ImageHandle>`. Not `Copy`: sharing is an explicit `clone`.
#[must_use = "hold the ImageHandle to keep its GPU texture alive — \
              discarding it (e.g. ignoring load_image's return) frees \
              the texture, so the image never renders"]
#[derive(Clone, Debug)]
pub struct ImageHandle {
    inner: Rc<ImageToken>,
}

#[derive(Debug)]
struct ImageToken {
    id: TextureId,
    size: UVec2,
    /// Counts updates, so the shape hash and damage move when a texture is rewritten under the same id.
    generation: Cell<u32>,
    registry: ImageRegistry,
}

impl Drop for ImageToken {
    fn drop(&mut self) {
        self.registry.free(self.id);
    }
}

impl ImageHandle {
    pub(crate) fn new(id: TextureId, image: &Image, registry: ImageRegistry) -> Self {
        registry.write(id, image);
        Self {
            inner: Rc::new(ImageToken {
                id,
                size: image.size,
                generation: Cell::new(0),
                registry,
            }),
        }
    }

    /// Stable per-registration id (never `TextureId(0)`), keying the GPU texture store and damage hash.
    #[inline]
    pub(crate) fn id(&self) -> TextureId {
        self.inner.id
    }

    #[inline]
    /// Pixel size.
    pub fn size(&self) -> UVec2 {
        self.inner.size
    }

    #[inline]
    pub(crate) fn generation(&self) -> u32 {
        self.inner.generation.get()
    }

    /// Overwrites the texture with `image`'s texels and repaints every shape drawing it, keeping the id;
    /// call before recording the shape, in the frame the change must show.
    ///
    /// # Panics
    ///
    /// Panics unless `image` is the registered size (a 2×3 and 3×2 image have equal byte counts, so wgpu would
    /// accept the write and scramble the rows).
    pub fn update(&self, image: &Image) {
        assert_eq!(
            image.size, self.inner.size,
            "an image update must match the registered size",
        );
        self.inner
            .generation
            .set(self.inner.generation.get().wrapping_add(1));
        self.inner.registry.write(self.inner.id, image);
    }
}

#[cfg(test)]
mod tests {
    use crate::primitives::identity::texture_id::TextureId;
    use crate::primitives::paint::image::Image;
    use crate::renderer::image_registry::ImageRegistry;
    use crate::renderer::image_registry::image_handle::ImageHandle;
    use glam::UVec2;

    #[test]
    fn every_update_bumps_the_generation_without_a_gpu() {
        let image = Image::blank(UVec2::ONE);
        let handle = ImageHandle::new(TextureId(1), &image, ImageRegistry::default());
        let clone = handle.clone();
        assert_eq!(handle.generation(), 0);
        handle.update(&image);
        assert_eq!(clone.generation(), 1);
        clone.update(&image);
        assert_eq!(handle.generation(), 2);
    }

    #[test]
    #[should_panic(expected = "an image update must match the registered size")]
    fn an_update_of_another_size_panics() {
        let handle = ImageHandle::new(
            TextureId(1),
            &Image::blank(UVec2::splat(2)),
            ImageRegistry::default(),
        );
        handle.update(&Image::blank(UVec2::new(2, 3)));
    }
}
