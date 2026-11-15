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
use crate::common::gen_slab::{GenSlab, SlabKey};
use crate::gpu::atlas::AtlasPool;
use crate::gpu::atlas::atlas_format::AtlasFormat;
use crate::gpu::atlas::atlas_handle::Evicted;
use crate::gpu::atlas::error::AtlasError;
use crate::gpu::atlas::page_list::PageListConfig;
use crate::gpu::atlas::tenant::{ItemOwner, Tenant};
use crate::gpu::pipeline::raster_pipeline::raster_quad::U_BITS;
use crate::gpu::raster::raster_atlas::atlas_slot::{AtlasSlot, PlacedRaster, SlotPlacement};
use crate::gpu::raster::raster_atlas::counters::AtlasCounters;
use crate::gpu::raster::raster_atlas::packed_metadata::PackedMetadata;
use crate::primitives::paint::content_type::ContentType;
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

/// What a raster asks of the pool: the list its content lives in, and its extents and bearing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RasterRequest {
    pub(super) format: AtlasFormat,
    pub(super) metadata: PackedMetadata,
}

/// One key's entry of the slab: what it draws, and the key, so an eviction the pool queues under
/// the entry's tag drops the key.
#[derive(Clone, Copy, Debug)]
pub(super) struct RasterEntry<K> {
    slot: AtlasSlot,
    key: K,
}

#[derive(Debug)]
pub(super) struct RasterAtlas<K> {
    tenant: Tenant,
    /// Each key's entry; a raster's pool item carries its entry's slot as its tag.
    entries: GenSlab<RasterEntry<K>>,
    cache: FxHashMap<K, SlabKey>,
    /// Deadlines for non-drawing entries (see [`ExpiryWheel`]), on the pool's display frames, as
    /// no eviction reaches them, so whitespace and rejected glyphs would otherwise pile up in the
    /// slab; [`Self::find`] files nothing.
    unplaced_expiry: ExpiryWheel<K>,
    pub(super) counters: AtlasCounters,
}

impl<K: Copy + Eq + Hash + Debug> RasterAtlas<K> {
    pub(super) fn new(tenant: Tenant) -> Self {
        Self {
            tenant,
            entries: GenSlab::new(),
            cache: FxHashMap::default(),
            unplaced_expiry: ExpiryWheel::with_keep(EXPIRY_FRAMES),
            counters: AtlasCounters::default(),
        }
    }

    /// What `key`'s entry draws, its raster stamped as drawn this frame, `None` on a miss. A
    /// raster another tenant's insert evicted this frame, before this tenant drained its queue, is
    /// a miss too, its key dropped here.
    pub(super) fn find(&mut self, pool: &mut AtlasPool, key: &K) -> Option<Found> {
        let &slab_key = self.cache.get(key)?;
        let entry = self
            .entries
            .get_mut(slab_key)
            .expect("a cached key names a live entry");
        match entry.slot {
            AtlasSlot::Placed(raster) => {
                let Some(placement) = pool.stamp(raster.handle) else {
                    self.drop_entry(slab_key);
                    self.counters.evictions.bump();
                    return None;
                };
                Some(Found::Raster(SlotPlacement { raster, placement }))
            }
            AtlasSlot::Unplaced { ref mut last_use } => {
                *last_use = pool.frame();
                Some(Found::Nothing)
            }
        }
    }

    /// Room in `pool` for `key`'s raster as `request` asks, which [`Self::find`] missed, filed
    /// under `key`, and where it draws; nothing filed where the pool refuses.
    pub(super) fn place(
        &mut self,
        pool: &mut AtlasPool,
        key: K,
        request: RasterRequest,
    ) -> Result<SlotPlacement, AtlasError> {
        let RasterRequest { format, metadata } = request;
        let tenant = self.tenant;
        let mut placed = None;
        let slab_key = self.entries.try_insert_with_key(|slab_key| {
            let owner = ItemOwner {
                tenant,
                tag: slab_key.idx,
            };
            let allocation = pool.allocate(format, owner, metadata.size.as_uvec2())?;
            let found = PlacedRaster {
                handle: allocation.handle,
                bearing: metadata.bearing,
            };
            placed = Some(SlotPlacement {
                raster: found,
                placement: allocation.placement,
            });
            Ok(RasterEntry {
                slot: AtlasSlot::Placed(found),
                key,
            })
        })?;
        self.file(key, slab_key);
        Ok(placed.expect("a filed raster was placed"))
    }

    /// File `key`, which [`Self::find`] missed, as drawing nothing, looked up at display frame
    /// `frame`.
    pub(super) fn insert_unplaced(&mut self, key: K, frame: u64) {
        self.unplaced_expiry.schedule(key, unplaced_dies_at(frame));
        let slab_key = self.entries.insert(RasterEntry {
            slot: AtlasSlot::Unplaced { last_use: frame },
            key,
        });
        self.file(key, slab_key);
    }

    /// Name `slab_key`'s entry by `key`, which a failed [`Self::find`] left unnamed.
    fn file(&mut self, key: K, slab_key: SlabKey) {
        let prev = self.cache.insert(key, slab_key);
        // Callers insert only after a failed `find`, so the key must be new.
        debug_assert!(prev.is_none(), "raster inserted over a live cache entry");
    }

    /// The keys the atlas holds a slot for.
    pub(super) fn cached_len(&self) -> usize {
        self.cache.len()
    }

    /// Drop the keys of the rasters the pool evicted since the last drain: at the end of each
    /// submit, and after a purge. An eviction's tag names its entry's slot, whose raster is still
    /// the evicted one unless a lookup that found it gone dropped the entry, and the slot took
    /// another since.
    pub(super) fn drop_evicted(&mut self, pool: &mut AtlasPool) {
        let mut evicted = 0;
        for Evicted { handle, tag } in pool.drain_evicted(self.tenant) {
            let Some(slab_key) = self.entries.key_at(tag as usize) else {
                continue;
            };
            let entry = self.entries.get(slab_key).expect("a slot's key names it");
            if entry
                .slot
                .raster()
                .is_some_and(|raster| raster.handle == handle)
            {
                self.drop_entry(slab_key);
                evicted += 1;
            }
        }
        self.counters.evictions.edit(|n| *n += evicted);
    }

    /// Retire the non-drawing entries due at display frame `frame`, at the end of each submit.
    pub(super) fn expire(&mut self, frame: u64) {
        let mut slab = Slab {
            cache: &mut self.cache,
            entries: &mut self.entries,
        };
        // No stamp check: deadlines only move out, so every ticket that fires is the live one.
        self.unplaced_expiry
            .retire(frame, |key, _| slab.retire_unplaced(key, frame));
    }

    /// Drop the entry `slab_key` names and its key, its raster already gone from the pool.
    fn drop_entry(&mut self, slab_key: SlabKey) {
        let entry = self
            .entries
            .remove(slab_key)
            .expect("a dropped entry is live");
        let removed = self.cache.remove(&entry.key);
        debug_assert_eq!(
            removed,
            Some(slab_key),
            "an entry's key named another entry",
        );
    }

    /// Drop every entry whose key `keep` rejects, giving their rasters back to `pool`, for
    /// retiring a family of keys at once (the icon backend, on unloading sets), which would
    /// otherwise hold rectangles until pressure swept them. Walks [`Self::cache`], not the slab;
    /// `keep` must answer for a set of doomed keys in one pass.
    pub(super) fn forget(&mut self, pool: &mut AtlasPool, keep: impl Fn(&K) -> bool) {
        let Self { cache, entries, .. } = self;
        cache.retain(|key, &mut slab_key| {
            if keep(key) {
                return true;
            }
            let entry = entries
                .remove(slab_key)
                .expect("a cached key names a live entry");
            if let Some(raster) = entry.slot.raster() {
                // Evicted already, its handle waiting in the queue, where the drain finds its slot
                // vacant or another's, a release does nothing.
                pool.release(raster.handle);
            }
            false
        });
    }
}

/// The slab and its names, borrowed apart from [`RasterAtlas::unplaced_expiry`] so its drain can
/// re-file into it.
#[derive(Debug)]
struct Slab<'a, K> {
    cache: &'a mut FxHashMap<K, SlabKey>,
    entries: &'a mut GenSlab<RasterEntry<K>>,
}

impl<K: Copy + Eq + Hash + Debug> Slab<'_, K> {
    /// Settle one drained non-drawing ticket: `Some(due)` to re-file, `None` once
    /// reclaimed or no longer this wheel's business.
    fn retire_unplaced(&mut self, key: K, frame: u64) -> Option<u64> {
        // `get`, not `entry`, which reserves on a vacant key and could rehash the map on a frame that
        // rasterizes nothing.
        let slab_key = *self.cache.get(&key)?;
        let entry = self
            .entries
            .get(slab_key)
            .expect("a cached key names a live entry");
        // A key forgotten and filed again since as a raster is the pool's to reclaim.
        let AtlasSlot::Unplaced { last_use } = entry.slot else {
            return None;
        };
        let dies_at = unplaced_dies_at(last_use);
        if dies_at > frame {
            return Some(dies_at);
        }
        self.cache.remove(&key);
        self.entries.remove(slab_key);
        None
    }
}

/// The display frame a non-drawing entry last used on `last_use` is first dead; one expression for
/// both the filing and the re-file.
const fn unplaced_dies_at(last_use: u64) -> u64 {
    last_use + EXPIRY_FRAMES + 1
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::common::gen_slab::SlabKey;
    use crate::gpu::raster::raster_atlas::RasterAtlas;
    use crate::gpu::raster::raster_atlas::atlas_slot::AtlasSlot;
    use rustc_hash::FxHashMap;
    use std::fmt::Debug;
    use std::hash::Hash;

    pub(crate) const fn unplaced_dies_at(last_use: u64) -> u64 {
        super::unplaced_dies_at(last_use)
    }

    impl<K: Copy + Eq + Hash + Debug> RasterAtlas<K> {
        pub(crate) const fn cache(&self) -> &FxHashMap<K, SlabKey> {
            &self.cache
        }

        /// The keys held as drawing nothing.
        pub(crate) fn unplaced_len(&self) -> usize {
            self.entries
                .values()
                .filter(|entry| matches!(entry.slot, AtlasSlot::Unplaced { .. }))
                .count()
        }
    }
}

#[cfg(test)]
mod tests;
