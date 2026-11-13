//! A raster tenant's side of the atlas pool, keyed by whatever it rasterizes from: glyphs (cosmic
//! `CacheKey`) or icons ([`IconRasterKey`](crate::icons::icon_raster_key::IconRasterKey)).
//!
//! The pool places, grows and evicts across both tenants (`gpu::atlas`,
//! `src/gpu/AGENTS.md`, Atlases); a tenant keeps the keys it rasterized, a slab of the rasters
//! each names with their bearings, and the entries that draw nothing. An eviction of either
//! tenant's raster reaches its owner through the pool's queue, drained at the end of each submit
//! and after a purge; a lookup that finds its raster gone before then drops the key itself.

pub(crate) mod atlas_slot;
pub(crate) mod counters;
pub(crate) mod packed_metadata;

use crate::common::expiry_wheel::ExpiryWheel;
use crate::common::expiry_window::EXPIRY_FRAMES;
use crate::gpu::atlas::AtlasPool;
use crate::gpu::atlas::atlas_format::AtlasFormat;
use crate::gpu::atlas::atlas_handle::{Allocation, AtlasHandle};
use crate::gpu::atlas::page_list::PageListConfig;
use crate::gpu::atlas::tenant::Tenant;
use crate::gpu::pipeline::raster_pipeline::raster_quad::U_BITS;
use crate::gpu::raster::raster_atlas::atlas_slot::{AtlasSlot, PlacedRaster, SlotPlacement};
use crate::gpu::raster::raster_atlas::counters::AtlasCounters;
use crate::primitives::paint::content_type::ContentType;
use glam::I16Vec2;
use rustc_hash::FxHashMap;
use std::fmt::Debug;
use std::hash::Hash;

// A quad packs a raster's origin in `U_BITS` a coordinate, so no page of a raster list passes it.
const _: () = {
    let edge = 1 << U_BITS;
    assert!(PageListConfig::of(AtlasFormat::R8).page_edge <= edge);
    assert!(PageListConfig::of(AtlasFormat::Rgba8).page_edge <= edge);
};

/// The pool's list a raster of `content` lives in. Here rather than on [`ContentType`]
/// (primitives-layer): naming the pool's formats is the graphics layer's job.
pub(super) const fn format_of(content: ContentType) -> AtlasFormat {
    match content {
        ContentType::Mask => AtlasFormat::R8,
        ContentType::Color => AtlasFormat::Rgba8,
    }
}

/// What a raster in `format`'s list holds, [`format_of`] turned back.
///
/// # Panics
///
/// For the half-float list, which holds no raster.
pub(crate) const fn content_of(format: AtlasFormat) -> ContentType {
    match format {
        AtlasFormat::R8 => ContentType::Mask,
        AtlasFormat::Rgba8 => ContentType::Color,
        AtlasFormat::Rgba16F => panic!("no raster lives in the half-float list"),
    }
}

/// What a key's entry gives a draw: the raster's placement, or nothing to draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Found {
    Raster(SlotPlacement),
    /// Whitespace, a raster with no pixels, past a page or past what a slot carries: cached, so
    /// its rasterizer runs once.
    Nothing,
}

#[derive(Debug)]
pub(super) struct RasterAtlas<K> {
    tenant: Tenant,
    /// Dense slot slab; `cache` maps each key to an index.
    slots: Vec<AtlasSlot>,
    /// Key held by each slab entry, parallel to [`Self::slots`], so an eviction can drop
    /// the victim's map entry by slab position. A separate column because the slot is
    /// hot and the key cold. Only meaningful for an index [`Self::cache`] still maps.
    slot_keys: Vec<K>,
    cache: FxHashMap<K, u32>,
    /// The slab index each pool handle sits at, so an eviction the pool queues finds its key.
    by_handle: FxHashMap<AtlasHandle, u32>,
    /// Slab indices no key names, the next insert's.
    free: Vec<u32>,
    /// Deadlines for non-drawing entries (see [`ExpiryWheel`]), on the pool's display frames, as
    /// no eviction reaches them, so whitespace and rejected glyphs would otherwise pile up in the
    /// slab; [`Self::find`] files nothing.
    unallocated_expiry: ExpiryWheel<K>,
    pub(super) counters: AtlasCounters,
}

impl<K: Copy + Eq + Hash + Debug> RasterAtlas<K> {
    pub(super) fn new(tenant: Tenant) -> Self {
        Self {
            tenant,
            slots: Vec::new(),
            slot_keys: Vec::new(),
            cache: FxHashMap::default(),
            by_handle: FxHashMap::default(),
            free: Vec::new(),
            unallocated_expiry: ExpiryWheel::with_keep(EXPIRY_FRAMES),
            counters: AtlasCounters::default(),
        }
    }

    /// What `key`'s entry draws, its raster stamped as drawn this frame, `None` on a miss. A
    /// raster another tenant's insert evicted this frame, before this tenant drained its queue, is
    /// a miss too, its key dropped here.
    pub(super) fn find(&mut self, pool: &mut AtlasPool, key: &K) -> Option<Found> {
        let &idx = self.cache.get(key)?;
        match self.slots[idx as usize] {
            AtlasSlot::Placed(raster) => {
                let Some(placement) = pool.stamp(raster.handle) else {
                    self.by_handle.remove(&raster.handle);
                    self.drop_slot(idx);
                    self.counters.evictions.bump();
                    return None;
                };
                Some(Found::Raster(SlotPlacement::of(placement, raster)))
            }
            AtlasSlot::Unplaced { ref mut last_use } => {
                *last_use = pool.frame();
                Some(Found::Nothing)
            }
            AtlasSlot::Free => unreachable!("a cached key names a live slot"),
        }
    }

    /// File `allocation`, a raster of `bearing`, under `key`, which [`Self::find`] missed, and say
    /// where it draws.
    pub(super) fn insert_placed(
        &mut self,
        key: K,
        allocation: Allocation,
        bearing: I16Vec2,
    ) -> SlotPlacement {
        let raster = PlacedRaster {
            handle: allocation.handle,
            bearing,
        };
        let idx = self.store(key, AtlasSlot::Placed(raster));
        self.by_handle.insert(raster.handle, idx);
        SlotPlacement::of(allocation.placement, raster)
    }

    /// File `key`, which [`Self::find`] missed, as drawing nothing, looked up at display frame
    /// `frame`.
    pub(super) fn insert_unplaced(&mut self, key: K, frame: u64) {
        self.unallocated_expiry
            .schedule(key, unallocated_dies_at(frame));
        self.store(key, AtlasSlot::Unplaced { last_use: frame });
    }

    fn store(&mut self, key: K, slot: AtlasSlot) -> u32 {
        let idx = if let Some(i) = self.free.pop() {
            self.slots[i as usize] = slot;
            self.slot_keys[i as usize] = key;
            i
        } else {
            self.slots.push(slot);
            self.slot_keys.push(key);
            u32::try_from(self.slots.len() - 1).expect("a slab counts its slots in u32")
        };
        let prev = self.cache.insert(key, idx);
        // Callers insert only after a failed `find`, so the key must be new.
        debug_assert!(prev.is_none(), "raster inserted over a live cache entry");
        idx
    }

    /// Whose rasters the atlas files.
    pub(super) const fn tenant(&self) -> Tenant {
        self.tenant
    }

    /// The keys the atlas holds a slot for.
    pub(super) fn cached_len(&self) -> usize {
        self.cache.len()
    }

    /// Drop the keys of the rasters the pool evicted since the last drain: at the end of each
    /// submit, and after a purge.
    pub(super) fn drop_evicted(&mut self, pool: &mut AtlasPool) {
        let mut evicted = 0;
        for handle in pool.drain_evicted(self.tenant) {
            // A lookup that found the raster gone dropped its slot already.
            if let Some(idx) = self.by_handle.remove(&handle) {
                let key = self.slot_keys[idx as usize];
                self.cache.remove(&key);
                free_slot(&mut self.slots, &mut self.free, idx);
                evicted += 1;
            }
        }
        self.counters.evictions.edit(|n| *n += evicted);
    }

    /// Retire the non-drawing entries due at display frame `frame`, at the end of each submit.
    pub(super) fn expire(&mut self, frame: u64) {
        let mut slab = Slab {
            cache: &mut self.cache,
            slots: &mut self.slots,
            free: &mut self.free,
        };
        // No stamp check: deadlines only move out, so every ticket that fires is the live one.
        self.unallocated_expiry
            .retire(frame, |key, _| slab.retire_unallocated(key, frame));
    }

    /// Drop the entry at slab index `idx` and its key, its raster already gone from the pool.
    fn drop_slot(&mut self, idx: u32) {
        let key = self.slot_keys[idx as usize];
        let removed = self.cache.remove(&key);
        debug_assert_eq!(
            removed,
            Some(idx),
            "slot_keys disagreed with cache about slab index {idx}",
        );
        free_slot(&mut self.slots, &mut self.free, idx);
    }

    /// Drop every entry whose key `keep` rejects, giving their rasters back to `pool`, for
    /// retiring a family of keys at once (the icon backend, on unloading sets), which would
    /// otherwise hold rectangles until pressure swept them. Walks [`Self::cache`], not the slab;
    /// `keep` must answer for a set of doomed keys in one pass.
    pub(super) fn forget(&mut self, pool: &mut AtlasPool, keep: impl Fn(&K) -> bool) {
        let Self {
            cache,
            slots,
            free,
            by_handle,
            ..
        } = self;
        cache.retain(|key, &mut idx| {
            if keep(key) {
                return true;
            }
            if let Some(raster) = slots[idx as usize].raster() {
                by_handle.remove(&raster.handle);
                // Evicted already, its handle waiting in the queue, where the drain passes it, a
                // release does nothing.
                pool.release(raster.handle);
            }
            free_slot(slots, free, idx);
            false
        });
    }
}

/// Mark slab index `idx` free and file it for the next insert.
fn free_slot(slots: &mut [AtlasSlot], free: &mut Vec<u32>, idx: u32) {
    let slot = &mut slots[idx as usize];
    debug_assert!(
        *slot != AtlasSlot::Free,
        "slab index {idx} released twice; two keys would share one slot",
    );
    *slot = AtlasSlot::Free;
    free.push(idx);
}

/// The slab and what a released slot returns to, borrowed apart from
/// [`RasterAtlas::unallocated_expiry`] so its drain can re-file into it.
#[derive(Debug)]
struct Slab<'a, K> {
    cache: &'a mut FxHashMap<K, u32>,
    slots: &'a mut [AtlasSlot],
    free: &'a mut Vec<u32>,
}

impl<K: Copy + Eq + Hash + Debug> Slab<'_, K> {
    /// Settle one drained non-drawing ticket: `Some(due)` to re-file, `None` once
    /// reclaimed or no longer this wheel's business.
    fn retire_unallocated(&mut self, key: K, frame: u64) -> Option<u64> {
        // `get`, not `entry`, which reserves on a vacant key and could rehash the map on a frame that
        // rasterizes nothing.
        let idx = *self.cache.get(&key)?;
        // A key forgotten and filed again since as a raster is the pool's to reclaim.
        let AtlasSlot::Unplaced { last_use } = self.slots[idx as usize] else {
            return None;
        };
        let dies_at = unallocated_dies_at(last_use);
        if dies_at > frame {
            return Some(dies_at);
        }
        self.cache.remove(&key);
        free_slot(self.slots, self.free, idx);
        None
    }
}

/// The display frame a non-drawing entry last used on `last_use` is first dead; one expression for
/// both the filing and the re-file.
const fn unallocated_dies_at(last_use: u64) -> u64 {
    last_use + EXPIRY_FRAMES + 1
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::gpu::raster::raster_atlas::RasterAtlas;
    use crate::gpu::raster::raster_atlas::atlas_slot::AtlasSlot;
    use rustc_hash::FxHashMap;
    use std::fmt::Debug;
    use std::hash::Hash;

    pub(crate) const fn unallocated_dies_at(last_use: u64) -> u64 {
        super::unallocated_dies_at(last_use)
    }

    impl<K: Copy + Eq + Hash + Debug> RasterAtlas<K> {
        pub(crate) const fn cache(&self) -> &FxHashMap<K, u32> {
            &self.cache
        }

        /// The keys held as drawing nothing.
        pub(crate) fn unplaced_len(&self) -> usize {
            self.cache
                .values()
                .filter(|&&idx| matches!(self.slots[idx as usize], AtlasSlot::Unplaced { .. }))
                .count()
        }
    }
}

#[cfg(test)]
mod tests;
