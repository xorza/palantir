//! A raster tenant's side of the atlas pool, keyed by whatever it rasterizes from: glyphs (cosmic
//! `CacheKey`) or icons ([`IconRasterKey`](crate::icons::icon_raster_key::IconRasterKey)).
//!
//! The pool places, grows and evicts across both tenants (`gpu::atlas`,
//! `src/gpu/AGENTS.md`, Atlases); a tenant keeps the keys it rasterized, a slab of the rasters
//! each names with their bearings, the entries that draw nothing, and the uploads it owes. An
//! eviction of either tenant's raster reaches its owner through the pool's queue, drained at the
//! end of each submit; a lookup that finds its raster gone before then drops the key itself.

pub(crate) mod atlas_slot;
pub(crate) mod counters;
pub(crate) mod packed_metadata;

use crate::common::expiry_wheel::ExpiryWheel;
use crate::gpu::atlas::atlas_format::AtlasFormat;
use crate::gpu::atlas::atlas_handle::AtlasHandle;
use crate::gpu::atlas::atlas_textures::AtlasTextures;
use crate::gpu::atlas::error::AtlasError;
use crate::gpu::atlas::tenant::Tenant;
use crate::gpu::atlas::{AtlasPool, LIST_CONFIGS};
use crate::gpu::debug_marker;
use crate::gpu::frame::gpu_ctx::{GpuCtx, RowCopy};
use crate::gpu::pipeline::raster_pipeline::raster_quad::U_BITS;
use crate::gpu::raster::raster_atlas::atlas_slot::{
    AtlasSlot, PlacedRaster, SlotPlacement, SlotState,
};
use crate::gpu::raster::raster_atlas::counters::AtlasCounters;
use crate::gpu::raster::raster_atlas::packed_metadata::PackedMetadata;
use crate::primitives::paint::content_type::ContentType;
use glam::{U16Vec2, UVec2};
use rustc_hash::FxHashMap;
use std::fmt::Debug;
use std::hash::Hash;
use std::ops::Range;

/// A raster ready to file: its packed extents and bearing, which list it reads from, and its rows
/// of pixels with no slack.
#[derive(Clone, Copy, Debug)]
pub(super) struct PackedRaster<'a> {
    pub(super) metadata: PackedMetadata,
    pub(super) content: ContentType,
    pub(super) pixels: &'a [u8],
}

/// Frames a non-drawing entry survives unused. No eviction reaches them, as the pool holds none,
/// so whitespace and rejected glyphs would otherwise pile up in the slab.
///
/// A per-entry deadline on [`RasterAtlas::unallocated_expiry`], not a periodic
/// `cache.retain`, which would make one frame in N pay for all. In the shared text
/// clock both instances age on; about 2 s at 60 Hz. A wrong value costs one
/// rasterizer call that yields no pixels. Its own constant, not
/// [`crate::text::RENDERED_RUN_KEEP_FRAMES`], so text tuning cannot move the icon
/// atlas.
const UNALLOCATED_KEEP_FRAMES: u64 = 120;

// A quad packs a raster's origin in `U_BITS` a coordinate, so no page of a raster list passes it.
const _: () = {
    let edge = 1 << U_BITS;
    assert!(LIST_CONFIGS[AtlasFormat::R8 as usize].page_edge <= edge);
    assert!(LIST_CONFIGS[AtlasFormat::Rgba8 as usize].page_edge <= edge);
};

/// The pool's list a raster of `content` lives in. Here rather than on [`ContentType`]
/// (primitives-layer): naming the pool's formats is the graphics layer's job.
const fn format_of(content: ContentType) -> AtlasFormat {
    match content {
        ContentType::Mask => AtlasFormat::R8,
        ContentType::Color => AtlasFormat::Rgba8,
    }
}

#[derive(Debug)]
pub(super) struct RasterAtlas<K> {
    tenant: Tenant,
    /// GPU debug label of the upload, built once: a per-frame `format!` would allocate.
    upload_label: String,
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
    /// Latest reading of the shared text clock, mirrored by [`Self::end_frame`].
    /// Counting submits here would make `RENDERED_RUN_KEEP_FRAMES`'s two windows drift
    /// (a drawless recorded frame ages buffers only; a `PaintOnly` frame the atlas only).
    cur_frame: u64,
    /// Deadlines for non-drawing entries (see [`ExpiryWheel`]); `touch` files nothing.
    unallocated_expiry: ExpiryWheel<K>,
    pub(super) counters: AtlasCounters,
    /// Raster pixels queued by `insert`, rows as packed: [`GpuCtx::copy_rows`] pads them to the
    /// copy pitch once, in the belt's mapped staging.
    pending_pixels: Vec<u8>,
    pending_copies: Vec<PendingCopy>,
}

#[derive(Clone, Debug)]
struct PendingCopy {
    format: AtlasFormat,
    page: u8,
    origin: UVec2,
    size: UVec2,
    pixels: Range<usize>,
}

impl<K: Copy + Eq + Hash + Debug> RasterAtlas<K> {
    pub(super) fn new(tenant: Tenant, label: &str) -> Self {
        Self {
            tenant,
            upload_label: format!("{label} atlas batch upload"),
            slots: Vec::new(),
            slot_keys: Vec::new(),
            cache: FxHashMap::default(),
            by_handle: FxHashMap::default(),
            free: Vec::new(),
            cur_frame: 0,
            unallocated_expiry: ExpiryWheel::with_keep(UNALLOCATED_KEEP_FRAMES),
            counters: AtlasCounters::default(),
            pending_pixels: Vec::new(),
            pending_copies: Vec::new(),
        }
    }

    /// The slab index of `key`, its raster stamped as drawn, `None` on a miss. A raster another tenant's
    /// insert evicted this frame, before this tenant drained its queue, is a miss too, its key
    /// dropped here.
    pub(super) fn touch(&mut self, pool: &mut AtlasPool, key: &K) -> Option<u32> {
        let &idx = self.cache.get(key)?;
        if let Some(raster) = self.slots[idx as usize].raster()
            && pool.stamp(raster.handle).is_none()
        {
            self.by_handle.remove(&raster.handle);
            self.drop_slot(idx);
            self.counters.evictions.bump();
            return None;
        }
        self.slots[idx as usize].last_use = self.cur_frame;
        Some(idx)
    }

    /// Where slot `idx`, touched or filed this frame, draws from now, `None` for one that draws
    /// nothing.
    pub(super) fn placement(&self, pool: &AtlasPool, idx: u32) -> Option<SlotPlacement> {
        let raster = self.slots[idx as usize].raster()?;
        let placement = pool
            .placement(raster.handle)
            .expect("the pool evicts no raster drawn this frame");
        Some(SlotPlacement::of(placement, raster))
    }

    /// The keys the atlas holds a slot for.
    pub(super) fn cached_len(&self) -> usize {
        self.cache.len()
    }

    /// The shared text clock's reading this atlas ages against.
    pub(super) const fn frame(&self) -> u64 {
        self.cur_frame
    }

    /// File a freshly rasterized raster under `key` in the pool, queueing its pixels for
    /// [`Self::flush_pending_uploads`]. `None` where the pool refuses it: past a page, which
    /// [`AtlasCounters::oversized`] counts, or full this frame.
    pub(super) fn insert(
        &mut self,
        pool: &mut AtlasPool,
        key: K,
        raster: PackedRaster<'_>,
    ) -> Option<u32> {
        let PackedRaster {
            metadata, content, ..
        } = raster;
        let format = format_of(content);
        let handle = match pool.allocate(format, self.tenant, metadata.size) {
            Ok(handle) => handle,
            Err(AtlasError::Oversized { .. }) => {
                self.counters.oversized.bump();
                return None;
            }
            Err(AtlasError::Full) => return None,
        };
        let placement = pool.placement(handle).expect("a new raster is live");
        self.enqueue_upload(format, placement.page, placement.origin, raster);
        let idx = self.store(
            key,
            SlotState::Placed(PlacedRaster {
                handle,
                bearing: metadata.bearing,
                content,
            }),
        );
        self.by_handle.insert(handle, idx);
        Some(idx)
    }

    /// Cache a non-drawing glyph: a slab index but no rectangle or upload.
    pub(super) fn insert_unallocated(&mut self, key: K) -> u32 {
        self.unallocated_expiry
            .schedule(key, unallocated_dies_at(self.cur_frame));
        self.store(key, SlotState::Unplaced)
    }

    fn store(&mut self, key: K, state: SlotState) -> u32 {
        let slot = AtlasSlot {
            state,
            last_use: self.cur_frame,
        };
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
        // Callers insert only after a failed `touch`, so the key must be new.
        debug_assert!(prev.is_none(), "raster inserted over a live cache entry");
        idx
    }

    /// Queue `raster`'s pixels for the rectangle at `origin` of `format`'s page `page`.
    fn enqueue_upload(
        &mut self,
        format: AtlasFormat,
        page: u8,
        origin: U16Vec2,
        raster: PackedRaster<'_>,
    ) {
        let size = raster.metadata.size.as_uvec2();
        let start = self.pending_pixels.len();
        let len = (size.x * raster.content.bytes_per_pixel()) as usize * size.y as usize;
        debug_assert_eq!(
            raster.pixels.len(),
            len,
            "a raster's bytes must be its rows with no slack",
        );
        self.pending_pixels.extend_from_slice(raster.pixels);
        self.pending_copies.push(PendingCopy {
            format,
            page,
            origin: origin.as_uvec2(),
            size,
            pixels: start..start + len,
        });
    }

    /// Drain this frame's queued rasters into `textures`' pages, once per frame after the pool's
    /// commit and before any pass draws, through [`GpuCtx::copy_rows`]: the bytes stage once, and
    /// no buffer of this atlas's own holds the peak frame's worth.
    pub(super) fn flush_pending_uploads(&mut self, ctx: &mut GpuCtx<'_>, textures: &AtlasTextures) {
        if self.pending_copies.is_empty() {
            return;
        }
        let Self {
            pending_pixels,
            pending_copies,
            upload_label,
            ..
        } = self;
        let pixels = &*pending_pixels;
        debug_marker::push(ctx.encoder, upload_label);
        ctx.copy_rows(pending_copies.iter().map(|copy| {
            RowCopy {
                texture: textures
                    .page_texture(copy.format, usize::from(copy.page))
                    .expect("a raster's page was committed before its upload"),
                origin: copy.origin,
                size: copy.size,
                rows: &pixels[copy.pixels.clone()],
            }
        }));
        debug_marker::pop(ctx.encoder);
        pending_pixels.clear();
        pending_copies.clear();
    }

    /// Drop the keys of the rasters the pool evicted since the last drain, and move this atlas's
    /// view of the shared clock to `frame`, retiring the non-drawing entries now due. At the end of
    /// each submit, which owns the frame boundary.
    pub(super) fn end_frame(&mut self, pool: &mut AtlasPool, frame: u64) {
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
        debug_assert!(
            frame >= self.cur_frame,
            "the atlas frame clock ran backwards",
        );
        self.cur_frame = frame;
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
                // Evicted already, its handle waiting in the queue, where the drain passes it.
                if pool.placement(raster.handle).is_some() {
                    pool.release(raster.handle);
                }
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
        slot.state != SlotState::Free,
        "slab index {idx} released twice; two keys would share one slot",
    );
    slot.state = SlotState::Free;
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
        let slot = &self.slots[idx as usize];
        // Placed entries are the pool's to reclaim. Defensive: every path filing a raster over a
        // slab index removes the old key first.
        if slot.state != SlotState::Unplaced {
            return None;
        }
        let dies_at = unallocated_dies_at(slot.last_use);
        if dies_at > frame {
            return Some(dies_at);
        }
        self.cache.remove(&key);
        free_slot(self.slots, self.free, idx);
        None
    }
}

/// The frame a non-drawing entry last used on `last_use` is first dead; one expression for both the
/// filing and the re-file.
const fn unallocated_dies_at(last_use: u64) -> u64 {
    last_use + UNALLOCATED_KEEP_FRAMES + 1
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
        pub(crate) fn slots(&self) -> &[AtlasSlot] {
            &self.slots
        }

        pub(crate) const fn cache(&self) -> &FxHashMap<K, u32> {
            &self.cache
        }
    }
}

#[cfg(test)]
mod tests;
