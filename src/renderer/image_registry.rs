//! Cross-frame registry of user images and their GPU textures — the
//! image counterpart of [`GradientAtlas`](crate::renderer::gradient_atlas::GradientAtlas),
//! bundled alongside it in [`RenderCaches`](crate::renderer::caches::RenderCaches).
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
//! upload; only the GPU texture persists. The pure data types
//! ([`Image`], [`ImageId`], [`ImageFit`](crate::primitives::image::ImageFit))
//! stay in `primitives`; this module owns only the stateful lifecycle.
//!
//! Single-threaded `Rc<RefCell<…>>` (same pattern as
//! [`FrameArena`](crate::forest::frame_arena::FrameArena)). Cheap to
//! clone; the inner state is shared.

use crate::primitives::image::Image;
use std::cell::RefCell;
use std::rc::Rc;

/// A registered image's identity: a process-unique id assigned at
/// [`ImageRegistry::register`]. Keys the GPU texture cache and threads
/// through the shape record + draw payload, so a bare `u64` can't be
/// confused with any other. `ImageId(0)` is the render path's "no
/// texture" value (the `Zeroable` default of a draw payload) and is never
/// handed out — ids start at `1`. `Pod` so it can live inline on the
/// `bytemuck`-cast draw payload.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct ImageId(pub(crate) u64);

/// RAII owner of a registered image's GPU texture, returned by
/// [`ImageRegistry::register`]. The texture lives exactly as long as an
/// `ImageHandle` (or any clone of one) is held; dropping the last clone
/// queues the texture for release. `Clone` shares ownership
/// (reference-counted). Reference it from
/// [`Shape::Image`](crate::shape::Shape::Image) each frame; "no image" is
/// expressed as `Option<ImageHandle>` at the call site, not a sentinel.
///
/// Not `Copy`: the lifetime is load-bearing, so sharing must be an
/// explicit `clone`. The render path keys on the cheap [`ImageId`] behind
/// it ([`Self::id`]), so per-frame draw data never carries the `Rc`.
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
struct ImageToken {
    id: ImageId,
    size: glam::U16Vec2,
    shared: Rc<RefCell<Inner>>,
}

impl Drop for ImageToken {
    fn drop(&mut self) {
        self.shared.borrow_mut().dropped.push(self.id);
    }
}

impl ImageHandle {
    /// Stable per-registration id (never `ImageId(0)` — that's the render
    /// path's "no texture" value). Keys the GPU texture cache and the
    /// per-shape damage hash.
    #[inline]
    pub(crate) fn id(&self) -> ImageId {
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
/// `Host` constructs one and hands clones to `Ui` (for registration) and
/// the wgpu backend (for upload + release).
#[derive(Clone, Default)]
pub struct ImageRegistry {
    inner: Rc<RefCell<Inner>>,
}

#[derive(Default)]
struct Inner {
    /// Monotonic id source. `0` is never handed out — it's the render
    /// path's "no texture" value.
    next_id: u64,
    /// Newly registered images awaiting their one GPU upload. Owns the
    /// bytes until the backend drains them; the `Image` is dropped right
    /// after upload, freeing the CPU copy.
    pending: Vec<(ImageId, Image)>,
    /// Ids whose last [`ImageHandle`] clone dropped since the last
    /// drain. The backend frees the matching GPU texture. One entry per
    /// dropped owner (ids are unique per registration, so each appears
    /// at most once).
    dropped: Vec<ImageId>,
}

impl ImageRegistry {
    /// Upload `image` and return an owning [`ImageHandle`]. The texture
    /// lives until the returned handle (and every clone of it) is
    /// dropped. Each call uploads independently — share one image across
    /// call sites by cloning the handle, not by re-registering.
    pub fn register(&self, image: Image) -> ImageHandle {
        let size = u16_size(&image);
        let id = {
            let mut inner = self.inner.borrow_mut();
            inner.next_id += 1;
            let id = ImageId(inner.next_id);
            inner.pending.push((id, image));
            id
        };
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
    pub(crate) fn drain_pending(&self, mut upload: impl FnMut(ImageId, Image)) {
        let mut inner = self.inner.borrow_mut();
        for (id, image) in inner.pending.drain(..) {
            upload(id, image);
        }
    }

    /// Drain the ids whose owning handles all dropped, calling `free` for
    /// each (the backend drops the matching GPU texture). Drains in place
    /// (retains capacity); same no-re-entry rule as [`Self::drain_pending`].
    pub(crate) fn drain_dropped(&self, mut free: impl FnMut(ImageId)) {
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
        image.width.min(u16::MAX as u32) as u16,
        image.height.min(u16::MAX as u32) as u16,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(w: u32, h: u32) -> Image {
        Image::from_rgba8(w, h, vec![0u8; (w * h * 4) as usize])
    }

    #[test]
    fn register_queues_one_upload_and_unique_ids() {
        let reg = ImageRegistry::default();
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

    #[test]
    fn dropping_last_handle_queues_release() {
        let reg = ImageRegistry::default();
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
