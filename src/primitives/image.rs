//! User-supplied raster images.
//!
//! Authoring side registers an [`Image`] once into an [`ImageRegistry`]
//! under a stable key, gets back an [`ImageHandle`], and references the
//! handle in [`Shape::Image`](crate::shape::Shape::Image)s every frame
//! — the bytes don't have to travel through user code again. The wgpu
//! backend drains [`ImageRegistry::drain_pending`] each frame and
//! uploads new entries to GPU; on eviction it calls
//! [`ImageRegistry::mark_pending`] to flag the handle for re-upload.
//!
//! Single-threaded `Rc<RefCell<…>>` (same pattern as
//! [`FrameArenaHandle`](crate::common::frame_arena::FrameArenaHandle)).
//! Cheap to clone; the inner map is shared.

use crate::common::hash::Hasher;
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::hash::{Hash, Hasher as _};
use std::rc::Rc;

/// Handle into the [`ImageRegistry`]. `id` is a 64-bit hash of the
/// user-supplied key (stable across frames); `size` is the image's
/// intrinsic pixel dimensions, baked in at registration so downstream
/// code (encoder, layout) never has to consult the registry to read
/// them. `u16` axes cap each side at 65 535 px — enough for 8K
/// (7 680). `Copy`, `repr` unconstrained, 16 B.
///
/// `Hash` / `Eq` key on `id` only; `size` is a fixed property of the
/// content keyed by `id`, so two handles with the same `id` and
/// different `size`s can't legitimately coexist (re-registering the
/// same key drops the new bytes per [`ImageRegistry::register`]'s
/// idempotent contract).
///
/// [`ImageHandle::NONE`] (`id == 0`) is the "no image" sentinel —
/// never produced by [`ImageRegistry::register`].
#[derive(Clone, Copy, Debug)]
pub struct ImageHandle {
    pub(crate) id: u64,
    pub(crate) size: glam::U16Vec2,
}

impl ImageHandle {
    pub const NONE: ImageHandle = ImageHandle {
        id: 0,
        size: glam::U16Vec2::ZERO,
    };

    #[inline]
    pub fn is_none(self) -> bool {
        self.id == 0
    }

    /// Intrinsic pixel dimensions. `(0, 0)` for [`Self::NONE`].
    #[inline]
    pub fn size(self) -> glam::UVec2 {
        self.size.as_uvec2()
    }
}

impl PartialEq for ImageHandle {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for ImageHandle {}

impl std::hash::Hash for ImageHandle {
    #[inline]
    fn hash<H: std::hash::Hasher>(&self, h: &mut H) {
        self.id.hash(h);
    }
}

/// How an image's intrinsic size maps onto its paint rect. Same
/// semantics as CSS `object-fit`. `Fill` (the default) stretches the
/// image to exactly fill the rect — fastest, no UV crop needed.
/// `Contain` / `None` produce a smaller paint rect inside the owner;
/// `Cover` produces a UV crop so the full rect is painted with the
/// image's centered portion.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ImageFit {
    /// Stretch the image to fill the rect exactly. Aspect ratio not
    /// preserved. Default — matches the legacy "no fit" behaviour.
    #[default]
    Fill,
    /// Preserve aspect ratio; fit the image entirely inside the rect.
    /// Letterboxes (transparent margins) if aspect ratios differ.
    Contain,
    /// Preserve aspect ratio; fill the rect entirely. Crops the
    /// image's longer axis (centered).
    Cover,
    /// Paint at the image's intrinsic pixel size, centered in the rect.
    /// Larger-than-rect images overflow the rect (currently uncropped —
    /// future slice can add per-image scissor).
    None,
}

impl Default for ImageHandle {
    #[inline]
    fn default() -> Self {
        Self::NONE
    }
}

/// Decoded pixel buffer. Straight (non-premultiplied) sRGB RGBA8 — the
/// backend uses a `Rgba8UnormSrgb` texture so the sampler decodes to
/// linear on read, and the shader premultiplies.
#[derive(Debug)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Image {
    /// Build from raw RGBA8 bytes. Hard-asserts
    /// `pixels.len() == width * height * 4`.
    pub fn from_rgba8(width: u32, height: u32, pixels: Vec<u8>) -> Self {
        let expected = (width as usize) * (height as usize) * 4;
        assert_eq!(
            pixels.len(),
            expected,
            "Image::from_rgba8: pixels.len() = {} != width*height*4 = {}",
            pixels.len(),
            expected,
        );
        Self {
            width,
            height,
            pixels,
        }
    }
}

/// Shared cross-frame image cache. Clone is cheap — the inner state is
/// `Rc`-shared. `Host` constructs one and hands clones to `Ui` (for
/// registration) and the wgpu backend (for upload / eviction signalling).
#[derive(Clone, Default)]
pub struct ImageRegistry {
    inner: Rc<RefCell<Inner>>,
}

#[derive(Default)]
struct Inner {
    /// Keyed on `id` (not `ImageHandle`) because the handle carries
    /// `size` too — and we want re-registering the same id to find
    /// the prior entry regardless of size lanes.
    by_id: FxHashMap<u64, Rc<Image>>,
    /// Handles needing GPU upload — newly registered, or evicted by
    /// the backend and flagged via `mark_pending`. Drained each frame
    /// by the backend; dedup is by linear scan because the set is
    /// typically tiny (most frames: empty; first frame: ~5 entries).
    pending: Vec<ImageHandle>,
}

impl ImageRegistry {
    /// Register an [`Image`] under a stable user-supplied key. Returns
    /// a handle reusable across frames — pass it to
    /// [`Shape::Image`](crate::shape::Shape::Image) without re-passing
    /// the bytes.
    ///
    /// Re-registering the same key drops the new `Image` (idempotent).
    /// To update bytes under the same logical name, version the key
    /// (`("logo", 2)`).
    ///
    /// The registry holds an `Rc<Image>` so the bytes survive across
    /// frames, allowing the backend to re-upload after GPU eviction
    /// without involving the user.
    pub fn register<K: Hash>(&self, key: K, image: Image) -> ImageHandle {
        use std::collections::hash_map::Entry;
        let id = hash_key(&key);
        let mut inner = self.inner.borrow_mut();
        let stored = match inner.by_id.entry(id) {
            Entry::Vacant(slot) => {
                let rc = Rc::new(image);
                let inserted = slot.insert(rc).clone();
                let h = ImageHandle {
                    id,
                    size: u16_size(&inserted),
                };
                inner.pending.push(h);
                inserted
            }
            // Re-register under the same key drops the new `image`;
            // returned handle carries the *original* image's size.
            // Versioned keys are the user-facing escape hatch.
            Entry::Occupied(slot) => slot.get().clone(),
        };
        ImageHandle {
            id,
            size: u16_size(&stored),
        }
    }

    /// Free this entry's bytes. Future draws using `handle` paint
    /// nothing (the backend sees a missing entry and skips). Idempotent.
    pub fn unregister(&self, handle: ImageHandle) {
        let mut inner = self.inner.borrow_mut();
        inner.by_id.remove(&handle.id);
        inner.pending.retain(|h| h.id != handle.id);
    }

    /// Look up the bytes for a handle. `None` if the handle was never
    /// registered or has been unregistered.
    #[allow(dead_code)] // wired by backend image pipeline (slice 1 Phase B)
    pub(crate) fn get(&self, handle: ImageHandle) -> Option<Rc<Image>> {
        self.inner.borrow().by_id.get(&handle.id).cloned()
    }

    /// Drain the set of handles needing GPU upload. The backend calls
    /// this once per frame; for each returned `(handle, Rc<Image>)` it
    /// uploads to GPU and stores the `GpuTexture` in its own cache.
    /// Bytes stay in the registry — the `Rc` keeps the CPU copy alive
    /// for future re-uploads after eviction.
    #[allow(dead_code)] // wired by backend image pipeline (slice 1 Phase B)
    pub(crate) fn drain_pending(&self) -> Vec<(ImageHandle, Rc<Image>)> {
        let mut inner = self.inner.borrow_mut();
        let Inner { by_id, pending } = &mut *inner;
        pending
            .drain(..)
            .filter_map(|h| by_id.get(&h.id).map(|img| (h, img.clone())))
            .collect()
    }

    /// Backend-side: flag `handle` for re-upload on next draw. Called
    /// when the GPU-side LRU evicts a texture so the registry's
    /// `Rc<Image>` is re-handed-out next `drain_pending`.
    #[allow(dead_code)] // GPU-side LRU lands in slice 2
    pub(crate) fn mark_pending(&self, handle: ImageHandle) {
        let mut inner = self.inner.borrow_mut();
        if inner.by_id.contains_key(&handle.id) && !inner.pending.iter().any(|h| h.id == handle.id)
        {
            inner.pending.push(handle);
        }
    }
}

/// Hash an arbitrary `Hash` key. `0` is reserved for
/// [`ImageHandle::NONE`]; collisions there are bumped to `1`.
fn hash_key<K: Hash>(key: &K) -> u64 {
    let mut h = Hasher::new();
    key.hash(&mut h);
    let v = h.finish();
    if v == 0 { 1 } else { v }
}

/// Saturating u32→u16 conversion for the handle's size lanes.
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
        Image::from_rgba8(w, h, vec![0; (w * h * 4) as usize])
    }

    #[test]
    fn same_key_same_handle() {
        let reg = ImageRegistry::default();
        let a = reg.register("logo", img(1, 1));
        let b = reg.register("logo", img(2, 2));
        assert_eq!(a, b);
        assert!(!a.is_none());
        // First image wins; re-register dropped the second.
        let stored = reg.get(a).unwrap();
        assert_eq!(stored.width, 1);
    }

    #[test]
    fn different_keys_different_handles() {
        let reg = ImageRegistry::default();
        let a = reg.register("logo", img(1, 1));
        let b = reg.register("avatar", img(1, 1));
        assert_ne!(a, b);
    }

    #[test]
    fn versioned_key_yields_new_handle() {
        let reg = ImageRegistry::default();
        let a = reg.register(("logo", 1u32), img(1, 1));
        let b = reg.register(("logo", 2u32), img(2, 2));
        assert_ne!(a, b);
    }

    #[test]
    fn drain_pending_drains_once() {
        let reg = ImageRegistry::default();
        let h = reg.register("k", img(1, 1));
        let first = reg.drain_pending();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].0, h);
        let second = reg.drain_pending();
        assert!(second.is_empty());
    }

    #[test]
    fn mark_pending_requeues_for_drain() {
        let reg = ImageRegistry::default();
        let h = reg.register("k", img(1, 1));
        let _ = reg.drain_pending();
        reg.mark_pending(h);
        let again = reg.drain_pending();
        assert_eq!(again.len(), 1);
        assert_eq!(again[0].0, h);
    }

    #[test]
    fn mark_pending_unknown_handle_noop() {
        let reg = ImageRegistry::default();
        reg.mark_pending(ImageHandle {
            id: 0xdead_beef,
            size: glam::U16Vec2::ZERO,
        });
        assert!(reg.drain_pending().is_empty());
    }

    #[test]
    fn unregister_removes_and_dequeues() {
        let reg = ImageRegistry::default();
        let h = reg.register("k", img(1, 1));
        reg.unregister(h);
        assert!(reg.get(h).is_none());
        assert!(reg.drain_pending().is_empty());
    }

    #[test]
    #[should_panic(expected = "Image::from_rgba8")]
    fn wrong_pixel_count_panics() {
        let _ = Image::from_rgba8(2, 2, vec![0; 3]);
    }
}
