use crate::primitives::image::Image;
use crate::primitives::texture_id::TextureId;
use crate::renderer::image_registry::ImageRegistry;
use glam::UVec2;
use std::cell::Cell;
use std::rc::Rc;

/// RAII owner of a registered image's GPU texture, returned by
/// [`Ui::register_image`](crate::Ui::register_image). The texture lives exactly
/// as long as an `ImageHandle` (or any clone of one) is held; dropping the last
/// clone frees it. `Clone` shares ownership (reference-counted). Reference it
/// from [`Shape::image`](crate::Shape::image) each frame; "no image" is
/// expressed as `Option<ImageHandle>` at the call site, not a sentinel.
///
/// Not `Copy`: the lifetime is load-bearing, so sharing must be an
/// explicit `clone`. The render path keys on a cheap internal texture id, so
/// per-frame draw data never carries the `Rc`.
#[must_use = "hold the ImageHandle to keep its GPU texture alive — \
              discarding it (e.g. ignoring register_image's return) frees \
              the texture, so the image never renders"]
#[derive(Clone, Debug)]
pub struct ImageHandle {
    inner: Rc<ImageToken>,
}

#[derive(Debug)]
struct ImageToken {
    id: TextureId,
    size: UVec2,
    /// Counts the updates. Rides the image record, so the shape hash — and
    /// with it damage — moves with every rewrite of a texture whose id does
    /// not.
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
        registry.create(id, image);
        Self {
            inner: Rc::new(ImageToken {
                id,
                size: image.size,
                generation: Cell::new(0),
                registry,
            }),
        }
    }

    /// Stable per-registration id (never `TextureId(0)` — that's the render
    /// path's "no texture" value). Keys the GPU texture store and the
    /// per-shape damage hash.
    #[inline]
    pub(crate) fn id(&self) -> TextureId {
        self.inner.id
    }

    /// Intrinsic pixel dimensions, baked in at registration so
    /// downstream code never consults the registry to read them.
    #[inline]
    pub fn size(&self) -> UVec2 {
        self.inner.size
    }

    #[inline]
    pub(crate) fn generation(&self) -> u32 {
        self.inner.generation.get()
    }

    /// Overwrite the texture with `image`'s texels at once, and repaint
    /// every shape drawing it.
    ///
    /// The door for a surface whose pixels change while it is on screen — a
    /// colour-picker field following a hue drag, a decoded video frame, a
    /// CPU preview. Registering again would mint a new id, build a second
    /// texture and free the first; this keeps the texture and its binding
    /// and issues one `write_texture`, which copies the bytes into wgpu's
    /// staging before it returns. The caller can reuse the CPU pixel buffer;
    /// wgpu allocates staging memory for each upload.
    ///
    /// Update before recording the shape that draws the image, in the frame
    /// the change must show.
    ///
    /// # Panics
    ///
    /// Panics unless `image` is the registered size. The size is fixed at
    /// registration; a surface that must change size registers again and
    /// drops the old handle. A release assert, not a debug one: a 2×3 and a
    /// 3×2 image have the same byte count, so wgpu would accept the write
    /// and draw the rows scrambled.
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
    use crate::primitives::image::Image;
    use crate::primitives::texture_id::TextureId;
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
