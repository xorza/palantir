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

/// 64-bit hash of the user-supplied key. Stable across frames and
/// across `Ui::frame` boundaries. [`ImageHandle::NONE`] (value `0`) is
/// the "no image" sentinel — never produced by
/// [`ImageRegistry::register`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ImageHandle(pub(crate) u64);

impl ImageHandle {
    pub const NONE: ImageHandle = ImageHandle(0);

    #[inline]
    pub fn is_none(self) -> bool {
        self.0 == 0
    }
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
    by_id: FxHashMap<ImageHandle, Rc<Image>>,
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
        let handle = hash_key(&key);
        let mut inner = self.inner.borrow_mut();
        if let std::collections::hash_map::Entry::Vacant(slot) = inner.by_id.entry(handle) {
            slot.insert(Rc::new(image));
            inner.pending.push(handle);
        }
        handle
    }

    /// Free this entry's bytes. Future draws using `handle` paint
    /// nothing (the backend sees a missing entry and skips). Idempotent.
    pub fn unregister(&self, handle: ImageHandle) {
        let mut inner = self.inner.borrow_mut();
        inner.by_id.remove(&handle);
        inner.pending.retain(|h| *h != handle);
    }

    /// Look up the bytes for a handle. `None` if the handle was never
    /// registered or has been unregistered.
    #[allow(dead_code)] // wired by backend image pipeline (slice 1 Phase B)
    pub(crate) fn get(&self, handle: ImageHandle) -> Option<Rc<Image>> {
        self.inner.borrow().by_id.get(&handle).cloned()
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
            .filter_map(|h| by_id.get(&h).map(|img| (h, img.clone())))
            .collect()
    }

    /// Backend-side: flag `handle` for re-upload on next draw. Called
    /// when the GPU-side LRU evicts a texture so the registry's
    /// `Rc<Image>` is re-handed-out next `drain_pending`.
    #[allow(dead_code)] // GPU-side LRU lands in slice 2
    pub(crate) fn mark_pending(&self, handle: ImageHandle) {
        let mut inner = self.inner.borrow_mut();
        if inner.by_id.contains_key(&handle) && !inner.pending.contains(&handle) {
            inner.pending.push(handle);
        }
    }
}

/// Hash an arbitrary `Hash` key to an [`ImageHandle`]. `0` is reserved
/// for [`ImageHandle::NONE`]; collisions there are bumped to `1`.
fn hash_key<K: Hash>(key: &K) -> ImageHandle {
    let mut h = Hasher::new();
    key.hash(&mut h);
    let v = h.finish();
    ImageHandle(if v == 0 { 1 } else { v })
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
        reg.mark_pending(ImageHandle(0xdead_beef));
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
