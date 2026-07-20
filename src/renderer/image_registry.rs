//! Cross-frame registry of user images and their GPU textures — the
//! image counterpart of the renderer's gradient atlas.
//!
//! [`ImageRegistry::register`] takes an [`Image`], queues it for GPU
//! upload, and returns an [`ImageHandle`] — an **RAII owner** of the
//! resulting GPU texture. Hold the handle (clone it where it needs to
//! live) to keep the texture resident; dropping the last clone frees the
//! texture. There is no `unregister` — the handle's lifetime *is* the
//! texture's lifetime.
//!
//! Reference the handle from [`Shape::Image`](crate::shape::Shape::Image)
//! every frame. The CPU bytes travel to the GPU exactly once — on the
//! first drain after registration — and are dropped immediately after
//! upload; only the GPU texture persists. The pure data types live
//! elsewhere — [`Image`] / [`ImageFit`](crate::primitives::image::ImageFit)
//! in `primitives`, [`TextureId`](crate::renderer::texture_id::TextureId) +
//! its source in `renderer::texture_id` — so this module owns only the
//! stateful lifecycle.
//!
//! Single-threaded `Rc<RefCell<…>>`; cheap to clone, with shared inner state.

use crate::primitives::image::Image;
use crate::renderer::texture_id::{TextureId, TextureIdSource};
use std::cell::RefCell;
use std::rc::Rc;

/// RAII owner of a registered image's GPU texture, returned by
/// [`Ui::register_image`](crate::Ui::register_image). The texture lives exactly
/// as long as an `ImageHandle` (or any clone of one) is held; dropping the last
/// clone queues the texture for release. `Clone` shares ownership
/// (reference-counted). Reference it from [`Shape::Image`](crate::Shape::Image)
/// each frame; "no image" is
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
/// whole lifecycle: when the last `ImageHandle` clone goes away, push the
/// id onto the shared drop queue so the backend frees the GPU texture on
/// its next drain.
#[derive(Debug)]
struct ImageToken {
    id: TextureId,
    size: glam::U16Vec2,
    shared: Rc<RefCell<Inner>>,
}

impl Drop for ImageToken {
    fn drop(&mut self) {
        self.shared.borrow_mut().dropped.push(self.id);
    }
}

impl ImageHandle {
    /// Stable per-registration id (never `TextureId(0)` — that's the render
    /// path's "no texture" value). Keys the GPU texture cache and the
    /// per-shape damage hash.
    #[inline]
    pub(crate) fn id(&self) -> TextureId {
        self.inner.id
    }

    /// Intrinsic pixel dimensions, baked in at registration so
    /// downstream code never consults the registry to read them.
    #[inline]
    pub fn size(&self) -> glam::UVec2 {
        self.inner.size.as_uvec2()
    }

    /// `u16`-packed intrinsic size as stored on the shape record.
    #[inline]
    pub(crate) fn size_u16(&self) -> glam::U16Vec2 {
        self.inner.size
    }
}

impl std::fmt::Debug for ImageHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageHandle")
            .field("id", &self.inner.id)
            .field("size", &self.inner.size)
            .field("owners", &Rc::strong_count(&self.inner))
            .finish()
    }
}

/// Shared image lifecycle: hands the backend the bytes of newly
/// registered images (once) and the ids of dropped handles (to free
/// their GPU textures). Clone is cheap — the inner state is `Rc`-shared.
/// `HostShared` retains it through `UiResources`; the host derives capability
/// clones for recorder registration and backend upload/release.
#[derive(Clone, Debug)]
pub(crate) struct ImageRegistry {
    inner: Rc<RefCell<Inner>>,
    /// Shared id source — also drawn from by each `GpuView` target so the two
    /// never mint colliding ids (see [`TextureIdSource`]).
    ids: TextureIdSource,
}

#[derive(Debug, Default)]
struct Inner {
    /// Newly registered images awaiting their one GPU upload. Owns the
    /// bytes until the backend drains them; the `Image` is dropped right
    /// after upload, freeing the CPU copy.
    pending: Vec<(TextureId, Image)>,
    /// Ids whose last [`ImageHandle`] clone dropped since the last
    /// drain. The backend frees the matching GPU texture. One entry per
    /// dropped owner (ids are unique per registration, so each appears
    /// at most once).
    dropped: Vec<TextureId>,
}

impl ImageRegistry {
    /// Build a registry minting from `ids`. Shares the same [`TextureIdSource`]
    /// with `GpuView` target minting (`Ui::gpu_view`) so their ids can't collide.
    pub(crate) fn new(ids: TextureIdSource) -> Self {
        Self {
            inner: Rc::new(RefCell::new(Inner::default())),
            ids,
        }
    }

    /// Upload `image` and return an owning [`ImageHandle`]. The texture
    /// lives until the returned handle (and every clone of it) is
    /// dropped. Each call uploads independently — share one image across
    /// call sites by cloning the handle, not by re-registering.
    pub(crate) fn register(&self, image: Image) -> ImageHandle {
        let size = u16_size(&image);
        let id = self.ids.reserve();
        self.inner.borrow_mut().pending.push((id, image));
        ImageHandle {
            inner: Rc::new(ImageToken {
                id,
                size,
                shared: Rc::clone(&self.inner),
            }),
        }
    }

    /// Drain images needing GPU upload, calling `upload` for each. The
    /// backend calls this once per frame and uploads inside the closure;
    /// the moved-in `Image` is **dropped right after** — the CPU bytes
    /// don't outlive the upload. Drains in place, so `pending` keeps its
    /// capacity (no realloc across registration bursts).
    ///
    /// The registry borrow is held across `upload`, so the closure must
    /// not re-enter the registry (register / drop a handle). The upload
    /// path is GPU-only and doesn't, so this is safe.
    pub(crate) fn drain_pending(&self, mut upload: impl FnMut(TextureId, Image)) {
        let mut inner = self.inner.borrow_mut();
        for (id, image) in inner.pending.drain(..) {
            upload(id, image);
        }
    }

    /// Drain the ids whose owning handles all dropped, calling `free` for
    /// each (the backend drops the matching GPU texture). Drains in place
    /// (retains capacity); same no-re-entry rule as [`Self::drain_pending`].
    pub(crate) fn drain_dropped(&self, mut free: impl FnMut(TextureId)) {
        let mut inner = self.inner.borrow_mut();
        for id in inner.dropped.drain(..) {
            free(id);
        }
    }
}

/// Pack an image's dimensions into `u16` axes (caps each side at
/// 65 535 px — past 8K). Saturates rather than wrapping.
fn u16_size(image: &Image) -> glam::U16Vec2 {
    glam::U16Vec2::new(
        image.size.x.min(u16::MAX as u32) as u16,
        image.size.y.min(u16::MAX as u32) as u16,
    )
}

#[cfg(test)]
mod tests {
    use crate::renderer::image_registry::*;
    use crate::renderer::texture_id::TextureIdSource;

    fn reg() -> ImageRegistry {
        ImageRegistry::new(TextureIdSource::default())
    }

    fn img(w: u32, h: u32) -> Image {
        Image::from_rgba8(w, h, vec![0u8; (w * h * 4) as usize])
    }

    #[test]
    fn register_queues_one_upload_and_unique_ids() {
        let reg = reg();
        let a = reg.register(img(2, 3));
        let b = reg.register(img(4, 5));
        // Distinct registrations get distinct ids, both nonzero.
        assert_ne!(a.id(), b.id());
        assert_ne!(a.id().0, 0);
        assert_eq!(a.size(), glam::UVec2::new(2, 3));
        // Both uploads are pending; draining hands the bytes over once.
        let mut uploaded = 0;
        reg.drain_pending(|_, _| uploaded += 1);
        assert_eq!(uploaded, 2);
        reg.drain_pending(|_, _| uploaded += 1);
        assert_eq!(uploaded, 2, "drain consumes pending");
    }

    /// A 0×0 image is a logic error caught at construction — before it
    /// can reach `register` and blow up a frame later in the GPU upload.
    #[test]
    #[should_panic(expected = "RGBA8 dimensions must be non-zero")]
    fn zero_sized_image_panics_at_construction() {
        let _ = img(0, 0);
    }

    #[test]
    fn dropping_last_handle_queues_release() {
        let reg = reg();
        let h = reg.register(img(1, 1));
        let id = h.id();
        reg.drain_pending(|_, _| {});
        // A live clone keeps it alive: no release queued yet.
        let clone = h.clone();
        drop(h);
        let mut freed = Vec::new();
        reg.drain_dropped(|id| freed.push(id));
        assert!(freed.is_empty(), "clone still holds it");
        // Last clone gone → id queued for GPU release exactly once.
        drop(clone);
        reg.drain_dropped(|id| freed.push(id));
        assert_eq!(freed, vec![id]);
        reg.drain_dropped(|id| freed.push(id));
        assert_eq!(freed, vec![id], "drain consumes dropped");
    }
}
