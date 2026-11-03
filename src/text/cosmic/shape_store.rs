//! [`ShapeStore`]: where a miss finds a shape of its text at other metrics, resident or kept past
//! its buffer's entry.

use crate::common::expiry_wheel::ExpiryWheel;
use crate::text::RENDERED_RUN_KEEP_FRAMES;
use crate::text::cosmic::cache_entry::lines_hold;
use crate::text::cosmic::shaped_buffer_cache::Recycled;
use crate::text::key::{ShapeIdentity, TextShapeKey};
use cosmic_text::BufferLine;
use rustc_hash::FxHashMap;
use std::collections::hash_map::Entry;

/// Which key holds each shape identity's last whole-text buffer, and the buffers kept for their
/// shaped lines after their entries expired. Its own wheel ages the kept buffers, and keeping one
/// runs out of line, so the cache's frame tick stays small where nothing expires.
#[derive(Debug)]
pub(super) struct ShapeStore {
    /// For each shape identity, the last key filed with a buffer of its whole text.
    donors: FxHashMap<ShapeIdentity, TextShapeKey>,
    kept: FxHashMap<ShapeIdentity, KeptShape>,
    /// Which kept buffers come due on which frame; their deadlines only move out.
    expiry: ExpiryWheel<ShapeIdentity>,
}

/// An expired buffer kept for its shaped lines, out of the pool until it goes, so the pool's
/// buffers keep their allocations for a reshape.
#[derive(Debug)]
struct KeptShape {
    /// The key the buffer was filed under.
    key: TextShapeKey,
    recycled: Recycled,
    dies_at: u64,
}

impl Default for ShapeStore {
    fn default() -> Self {
        Self {
            donors: FxHashMap::default(),
            kept: FxHashMap::default(),
            expiry: ExpiryWheel::with_keep(RENDERED_RUN_KEEP_FRAMES),
        }
    }
}

impl ShapeStore {
    /// `key` holds its identity's whole text from now on.
    pub(super) fn filed(&mut self, key: TextShapeKey) {
        self.donors.insert(key.shape_identity(), key);
    }

    /// The key that last filed a whole-text buffer of `key`'s identity.
    pub(super) fn donor(&self, key: TextShapeKey) -> Option<TextShapeKey> {
        self.donors.get(&key.shape_identity()).copied()
    }

    /// `key`'s whole-text entry left the cache; another key may hold its identity since.
    pub(super) fn left(&mut self, key: TextShapeKey) {
        let identity = key.shape_identity();
        if self.donors.get(&identity) == Some(&key) {
            self.donors.remove(&identity);
        }
    }

    /// The lines of the buffer kept for `text` under `key`'s identity, at other metrics or under
    /// `key` itself, kept a rendered run's window longer from `frame`.
    pub(super) fn kept_lines(
        &mut self,
        key: TextShapeKey,
        text: &str,
        frame: u64,
    ) -> Option<&[BufferLine]> {
        let kept = self.kept.get_mut(&key.shape_identity())?;
        let lines = kept.recycled.buffer.lines.as_slice();
        if (kept.key != key && kept.key.shares_metrics(key)) || !lines_hold(lines, text) {
            return None;
        }
        kept.dies_at = frame + RENDERED_RUN_KEEP_FRAMES + 1;
        Some(lines)
    }

    /// Keep the buffer of `key`'s expired whole-text entry, unless one of its identity is kept:
    /// then it comes back, for the pool.
    #[cold]
    pub(super) fn keep(
        &mut self,
        key: TextShapeKey,
        recycled: Recycled,
        frame: u64,
    ) -> Option<Recycled> {
        let identity = key.shape_identity();
        let Entry::Vacant(slot) = self.kept.entry(identity) else {
            return Some(recycled);
        };
        let dies_at = frame + RENDERED_RUN_KEEP_FRAMES + 1;
        slot.insert(KeptShape {
            key,
            recycled,
            dies_at,
        });
        self.expiry.schedule(identity, dies_at);
        None
    }

    /// How many buffers are kept.
    pub(super) fn kept_len(&self) -> usize {
        self.kept.len()
    }

    /// Hand `release` every kept buffer whose window passed by `frame`.
    pub(super) fn tick(&mut self, frame: u64, mut release: impl FnMut(Recycled)) {
        let kept = &mut self.kept;
        self.expiry.retire(frame, |identity, _| {
            let Entry::Occupied(shape) = kept.entry(identity) else {
                return None;
            };
            if shape.get().dies_at > frame {
                return Some(shape.get().dies_at);
            }
            release(shape.remove().recycled);
            None
        });
    }

    /// Forget every donor and hand `release` every kept buffer.
    pub(super) fn drain(&mut self, mut release: impl FnMut(Recycled)) {
        self.donors.clear();
        #[expect(
            clippy::iter_over_hash_type,
            reason = "each buffer returns to the pool alone, so the order changes nothing"
        )]
        for (_, kept) in self.kept.drain() {
            release(kept.recycled);
        }
        self.expiry.clear();
    }
}
