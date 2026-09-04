//! Cross-frame registry of user images and their GPU textures — the
//! image counterpart of the renderer's gradient atlas.
//!
//! [`ImageRegistry::register`] takes an [`Image`], creates its GPU texture
//! on the spot, and returns an [`ImageHandle`] — an **RAII owner** of that
//! texture. Hold the handle (clone it where it needs to live) to keep the
//! texture resident; dropping the last clone frees it at once. There is no
//! `unregister` — the handle's lifetime *is* the texture's lifetime.
//!
//! Reference the handle from [`Shape::image`](crate::Shape::image) every
//! frame. The bytes reach the GPU inside `register`, and the registry keeps
//! none of them; only the GPU texture persists. A surface whose texels
//! change while it is on screen refills its own [`Image`] and hands it to
//! [`ImageHandle::update`]: same id, same texture, one `write_texture`, and
//! every shape drawing it repaints.
//!
//! Nothing here waits for a frame. The GPU side, [`ImageGpu`], is attached
//! by the one backend a host runs, before any `Ui` exists. A registry
//! without one — a CPU recorder, a test harness — mints ids and sizes and
//! discards the texels.
//!
//! The pure data types live elsewhere —
//! [`Image`] / [`ImageFit`](crate::primitives::image::ImageFit) in
//! `primitives`, [`TextureId`] in `primitives::texture_id` and its source
//! in `renderer::texture_id_source`, and the device ceiling a source is
//! measured against in
//! [`TextureLimit`](crate::renderer::texture_limit::TextureLimit) — so this
//! module owns only the lifecycle. Registration here is infallible:
//! `Ui::register_image` checks the ceiling before calling in, so a rejected
//! image never reaches this module at all.
//!
//! Single-threaded `Rc<RefCell<…>>`; cheap to clone, with shared inner state.

use crate::primitives::image::Image;
use crate::primitives::texture_id::TextureId;
use crate::renderer::backend::image_gpu::ImageGpu;
use crate::renderer::texture_id_source::TextureIdSource;
use glam::UVec2;
use std::cell::{Cell, Ref, RefCell};
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
#[derive(Clone)]
pub struct ImageHandle {
    inner: Rc<ImageToken>,
}

/// The reference-counted core of an [`ImageHandle`]. Its [`Drop`] is the
/// whole lifecycle: when the last `ImageHandle` clone goes away, the GPU
/// texture is freed.
#[derive(Debug)]
struct ImageToken {
    id: TextureId,
    size: UVec2,
    /// Counts the updates. Rides the image record, so the shape hash — and
    /// with it damage — moves with every rewrite of a texture whose id does
    /// not.
    generation: Cell<u32>,
    gpu: Rc<RefCell<Option<ImageGpu>>>,
}

impl Drop for ImageToken {
    fn drop(&mut self) {
        if let Some(gpu) = self.gpu.borrow_mut().as_mut() {
            gpu.free(self.id);
        }
    }
}

impl ImageHandle {
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

    /// How many times the texels were updated. What the image record
    /// carries so a rewrite under an unchanged id still moves the hash.
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
    /// staging before it returns. The registry keeps no copy, so a surface
    /// that refills one `Image` every frame allocates nothing.
    ///
    /// Update before recording the shape that draws the image, in the frame
    /// the change must show.
    ///
    /// # Panics
    ///
    /// Panics unless `image` is the registered size. The size is fixed at
    /// registration; a surface that must change size registers again and
    /// drops the old handle.
    pub fn update(&self, image: &Image) {
        assert_eq!(
            image.size, self.inner.size,
            "an image update must match the registered size",
        );
        self.inner
            .generation
            .set(self.inner.generation.get().wrapping_add(1));
        if let Some(gpu) = self.inner.gpu.borrow_mut().as_mut() {
            gpu.write(self.inner.id, &image.pixels);
        }
    }
}

impl std::fmt::Debug for ImageHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageHandle")
            .field("id", &self.inner.id)
            .field("size", &self.inner.size)
            .field("generation", &self.inner.generation.get())
            .field("owners", &Rc::strong_count(&self.inner))
            .finish()
    }
}

/// Shared image lifecycle: creates, rewrites and frees the GPU textures
/// the handles own. Clone is cheap — the inner state is `Rc`-shared.
/// `HostShared` retains it through `UiResources`; the backend attaches the
/// GPU side and binds through it when it draws.
#[derive(Clone, Debug)]
pub(crate) struct ImageRegistry {
    /// The GPU side, once a backend attached one. A deviceless recorder
    /// never does.
    gpu: Rc<RefCell<Option<ImageGpu>>>,
    /// Shared id source — also drawn from by each `GpuView` target so the two
    /// never mint colliding ids (see [`TextureIdSource`]).
    ids: TextureIdSource,
}

impl ImageRegistry {
    /// Build a registry minting from `ids`. Shares the same [`TextureIdSource`]
    /// with `GpuView` target minting (`Ui::gpu_view`) so their ids can't collide.
    pub(crate) fn new(ids: TextureIdSource) -> Self {
        Self {
            gpu: Rc::new(RefCell::new(None)),
            ids,
        }
    }

    /// Give the registry its GPU side. The one backend a host runs calls
    /// this at construction, before any `Ui` can register.
    pub(crate) fn attach(&self, gpu: ImageGpu) {
        let previous = self.gpu.borrow_mut().replace(gpu);
        assert!(previous.is_none(), "one backend attaches the GPU side once");
    }

    /// The GPU side, for the draw path.
    ///
    /// # Panics
    ///
    /// Panics without one attached: only a backend draws.
    pub(crate) fn gpu(&self) -> Ref<'_, ImageGpu> {
        Ref::map(self.gpu.borrow(), |gpu| {
            gpu.as_ref()
                .expect("a backend attached the GPU side at construction")
        })
    }

    /// Create `image`'s texture and return an owning [`ImageHandle`]. The
    /// texture lives until the returned handle (and every clone of it) is
    /// dropped. Each call is its own texture — share one image across call
    /// sites by cloning the handle, not by re-registering.
    ///
    /// Infallible: the device ceiling is
    /// [`TextureLimit`](crate::renderer::texture_limit::TextureLimit)'s to
    /// enforce, and `Ui::register_image` applies it before calling this —
    /// so the id is minted only once the image is known to be acceptable.
    pub(crate) fn register(&self, image: &Image) -> ImageHandle {
        let id = self.ids.reserve();
        if let Some(gpu) = self.gpu.borrow_mut().as_mut() {
            gpu.create(id, image.size, &image.pixels);
        }
        ImageHandle {
            inner: Rc::new(ImageToken {
                id,
                size: image.size,
                generation: Cell::new(0),
                gpu: Rc::clone(&self.gpu),
            }),
        }
    }
}

#[cfg(test)]
mod tests;
