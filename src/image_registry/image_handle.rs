use crate::image_registry::ImageRegistry;
use crate::image_registry::error::ImageSizeError;
use crate::primitives::identity::texture_id::TextureId;
use crate::primitives::paint::image::Image;
use glam::UVec2;
use std::cell::Cell;
use std::rc::Rc;

/// RAII owner of a registered image's GPU texture, from [`Ui::load_image`](crate::Ui::load_image); the
/// texture lives until the last clone drops. Reference it from [`ImagePaint`](crate::ImagePaint)
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
    /// The registration of `image`, which the caller measured against the device's limit.
    pub(crate) fn new(id: TextureId, image: &Image, registry: ImageRegistry) -> Self {
        registry
            .write(id, image)
            .expect("registration measured the image against this device's limit");
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

    /// Pixel size.
    #[inline]
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
    /// # Errors
    ///
    /// A device attached after a loss is smaller than the one the image registered on, and `image`
    /// exceeds its 2D texture limit: the image paints nothing and stays not
    /// [resident](Self::is_resident).
    ///
    /// # Panics
    ///
    /// Panics unless `image` is the registered size (a 2×3 and 3×2 image have equal byte counts, so wgpu would
    /// accept the write and scramble the rows).
    pub fn update(&self, image: &Image) -> Result<(), ImageSizeError> {
        assert_eq!(
            image.size, self.inner.size,
            "an image update must match the registered size",
        );
        self.inner.registry.write(self.inner.id, image)?;
        self.inner
            .generation
            .set(self.inner.generation.get().wrapping_add(1));
        Ok(())
    }

    /// Whether the device attached now holds the image's texture. A device lost takes every
    /// texture with it, and the image paints nothing until [`Self::update`] fills it again, so an
    /// app that keeps the texels of an image it shows calls `update` when this is `false`.
    /// Always `true` for a recorder with no device.
    pub fn is_resident(&self) -> bool {
        self.inner.registry.holds(self.inner.id)
    }
}

#[cfg(test)]
mod tests {
    use crate::image_registry::ImageRegistry;
    use crate::image_registry::image_handle::ImageHandle;
    use crate::primitives::identity::texture_id::TextureId;
    use crate::primitives::paint::image::Image;
    use glam::UVec2;

    #[test]
    fn every_update_bumps_the_generation_without_a_gpu() {
        let image = Image::blank(UVec2::ONE);
        let handle = ImageHandle::new(TextureId(1), &image, ImageRegistry::default());
        let clone = handle.clone();
        assert_eq!(handle.generation(), 0);
        handle.update(&image).unwrap();
        assert_eq!(clone.generation(), 1);
        clone.update(&image).unwrap();
        assert_eq!(handle.generation(), 2);
        assert!(
            handle.is_resident(),
            "a recorder with no device holds every image"
        );
    }

    #[test]
    #[should_panic(expected = "an image update must match the registered size")]
    fn an_update_of_another_size_panics() {
        let handle = ImageHandle::new(
            TextureId(1),
            &Image::blank(UVec2::splat(2)),
            ImageRegistry::default(),
        );
        handle.update(&Image::blank(UVec2::new(2, 3))).ok();
    }
}
