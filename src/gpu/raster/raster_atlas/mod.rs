//! Rasterized-quad atlas for mask and color content, keyed by whatever its
//! tenant rasterizes from: glyphs (cosmic `CacheKey`) or icons
//! ([`IconRasterKey`](crate::icons::icon_raster_key::IconRasterKey)).
//!
//! The two instances share the policies (shelf packing, clock-sweep eviction,
//! grow-with-blit, batched staging uploads) and the layout from [`RasterProgram`].
//! Each keeps its own textures, bind group and eviction budget, so a color-icon-heavy
//! frame cannot take rectangles from adjacent glyphs. Packing lives on [`ShelfPacker`],
//! victim selection on [`ClockSweep`].

pub(crate) mod atlas_slot;
mod bound_sides;
mod clock_sweep;
pub(crate) mod counters;
mod free_slots;
pub(crate) mod packed_metadata;
pub(crate) mod raster_quad;
mod shelf_packer;
mod side;

use crate::common::expiry_wheel::ExpiryWheel;
use crate::common::span::Span;
use crate::gpu::device::gpu_ctx::{GpuCtx, Staged};
use crate::gpu::frame::binder::MainBinder;
use crate::gpu::frame::debug_marker;
use crate::gpu::raster::raster_atlas::atlas_slot::{AtlasSlot, SlotPlacement};
use crate::gpu::raster::raster_atlas::bound_sides::{BoundSides, SidesLayout};
use crate::gpu::raster::raster_atlas::clock_sweep::{ClockSweep, Evictable};
use crate::gpu::raster::raster_atlas::counters::AtlasCounters;
use crate::gpu::raster::raster_atlas::free_slots::FreeSlots;
use crate::gpu::raster::raster_atlas::packed_metadata::PackedMetadata;
use crate::gpu::raster::raster_atlas::raster_quad::RasterQuad;
use crate::gpu::raster::raster_atlas::shelf_packer::ShelfPacker;
use crate::gpu::raster::raster_atlas::side::Side;
use crate::gpu::raster::raster_program::RasterProgram;
use crate::gpu::resource::dynamic_buffer::DynamicBuffer;
use crate::primitives::paint::content_type::ContentType;
use glam::{U16Vec2, UVec2};
use rustc_hash::FxHashMap;
use std::collections::hash_map::Entry;
use std::fmt::Debug;
use std::hash::Hash;
use std::ops::Range;
use wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;

/// How one [`RasterAtlas`] differs from the other: GPU debug label and initial side sizes.
#[derive(Clone, Copy, Debug)]
pub(super) struct RasterAtlasConfig {
    pub(super) label: &'static str,
    pub(super) initial_mask_px: u32,
    pub(super) initial_color_px: u32,
    /// Hard ceiling on one side's backing texture. Per instance because the budget is
    /// in bytes (a mask side gets four times a color side's edge). When it binds,
    /// `Rasterized::AtlasFull` is returned and the entry re-encodes each frame.
    pub(super) max_bytes: u64,
    /// Byte budget below which [`RasterAtlas::allocate`] grows a side rather than
    /// evicting. Evicting first would pin an atlas at its initial size however badly it
    /// fits, and victim selection is O(live entries) per call.
    pub(super) eager_growth_bytes: u64,
}

/// A raster ready to file: its packed extents and bearing, which side it reads from, and its rows
/// of pixels with no slack.
#[derive(Clone, Copy, Debug)]
pub(super) struct PackedRaster<'a> {
    pub(super) metadata: PackedMetadata,
    pub(super) content: ContentType,
    pub(super) pixels: &'a [u8],
}

/// GPU debug labels, built once: a per-frame `format!` would allocate.
#[derive(Debug)]
struct AtlasLabels {
    grow_blit: String,
    batch_upload: String,
}

/// Frames a non-drawing entry (`alloc: None`) survives unused. Victim selection
/// skips them, so whitespace and rejected glyphs would otherwise pile up in the slab.
///
/// A per-entry deadline on [`RasterAtlas::unallocated_expiry`], not a periodic
/// `cache.retain`, which would make one frame in N pay for all. In the shared text
/// clock both instances age on; about 2 s at 60 Hz. A wrong value costs one
/// rasterizer call that yields no pixels. Its own constant, not
/// [`crate::text::RENDERED_RUN_KEEP_FRAMES`], so text tuning cannot move the icon
/// atlas.
const UNALLOCATED_KEEP_FRAMES: u64 = 120;

#[derive(Debug)]
pub(super) struct RasterAtlas<K> {
    /// Grows sides as inserts demand.
    device: wgpu::Device,
    sides: [Side; 2],
    labels: AtlasLabels,
    eager_growth_bytes: u64,
    /// Dense slot slab; `cache` maps each key to an index. Encoded-run caches record
    /// indices; safe because every recorded index carries the slot generation eviction
    /// advances before reuse.
    pub(super) slots: Vec<AtlasSlot>,
    /// Key held by each slab entry, parallel to [`Self::slots`], so eviction can drop
    /// the victim's map entry by slab position. A separate column because the slot is
    /// hot and the key cold. Only meaningful for an index [`Self::cache`] still maps.
    slot_keys: Vec<K>,
    pub(super) cache: FxHashMap<K, u32>,
    free: FreeSlots,
    /// Rotating eviction cursor over [`Self::slots`]; persists so the victim search resumes instead
    /// of rescanning.
    hand: u32,
    /// Latest reading of the shared text clock, mirrored by [`Self::advance_to`].
    /// Counting submits here would make `RENDERED_RUN_KEEP_FRAMES`'s two windows drift
    /// (a drawless recorded frame ages buffers only; a `PaintOnly` frame the atlas only).
    pub(super) cur_frame: u64,
    /// Deadlines for non-drawing entries (see [`ExpiryWheel`]); `touch` files nothing.
    unallocated_expiry: ExpiryWheel<K>,
    /// Everything group 0 needs to read [`Self::sides`]; see [`BoundSides`]. Rebinding
    /// inside [`Self::grow`] leaves no window where binding and textures disagree.
    bound: BoundSides,
    pub(super) counters: AtlasCounters,

    /// Raster pixels queued by `insert`, rows as packed. No row padding here:
    /// `copy_buffer_to_texture` wants `wgpu::COPY_BYTES_PER_ROW_ALIGNMENT = 256`, so it
    /// happens once into the belt's mapped staging; see
    /// [`Self::flush_pending_uploads`].
    pending_pixels: Vec<u8>,
    pending_copies: Vec<PendingCopy>,
    /// [`Self::evict_shelf`]'s tally, one per shelf, retained so a starving insert allocates
    /// nothing.
    shelf_uses: Vec<ShelfUse>,
}

/// What a shelf holds of one content type, as [`RasterAtlas::evict_shelf`] tallies it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShelfUse {
    Empty,
    /// Entries, none drawn this frame.
    Evictable,
    /// An entry drawn this frame, which the shelf must keep.
    Drawn,
}

#[derive(Clone, Copy, Debug)]
struct PendingCopy {
    content: ContentType,
    origin: UVec2,
    size: UVec2,
    pixels_start: usize,
}

impl PendingCopy {
    const fn bytes_per_row(self) -> u32 {
        self.size.x * self.content.bytes_per_pixel()
    }

    /// Pitch `copy_buffer_to_texture` reads at; each raster's region is a whole number of these,
    /// keeping buffer offsets 256-aligned.
    const fn padded_bytes_per_row(self) -> u32 {
        self.bytes_per_row()
            .next_multiple_of(COPY_BYTES_PER_ROW_ALIGNMENT)
    }

    const fn pixels(self) -> Range<usize> {
        let len = self.bytes_per_row() as usize * self.size.y as usize;
        self.pixels_start..self.pixels_start + len
    }
}

impl<K: Copy + Eq + Hash + Debug> RasterAtlas<K> {
    pub(super) fn new(
        device: &wgpu::Device,
        program: &RasterProgram,
        config: RasterAtlasConfig,
    ) -> Self {
        let sides = [
            Side::new(device, ContentType::Mask, config),
            Side::new(device, ContentType::Color, config),
        ];
        let labels = AtlasLabels {
            grow_blit: format!("{} atlas grow blit", config.label),
            batch_upload: format!("{} atlas batch upload", config.label),
        };

        let bound = BoundSides::new(device, SidesLayout::new(program, config.label), &sides);

        Self {
            device: device.clone(),
            sides,
            labels,
            eager_growth_bytes: config.eager_growth_bytes,
            slots: Vec::new(),
            slot_keys: Vec::new(),
            cache: FxHashMap::default(),
            free: FreeSlots::default(),
            hand: 0,
            cur_frame: 0,
            unallocated_expiry: ExpiryWheel::with_keep(UNALLOCATED_KEEP_FRAMES),
            bound,
            counters: AtlasCounters::default(),
            pending_pixels: Vec::new(),
            pending_copies: Vec::new(),
            shelf_uses: Vec::new(),
        }
    }

    /// Bind this atlas and draw `span` of `vbuf`'s quad instances; text and icon share one
    /// bind-group shape (see [`RasterQuad`]).
    pub(super) fn draw_span<'a>(
        &'a self,
        binder: &mut MainBinder<'a>,
        pass: &mut wgpu::RenderPass<'a>,
        vbuf: &'a DynamicBuffer<RasterQuad>,
        span: Span,
    ) {
        if span.len == 0 {
            return;
        }
        binder.group(pass, 0, self.bound.bind_group());
        binder.vertex(pass, 0, &vbuf.buffer);
        pass.draw(0..4, span.start..span.start + span.len);
    }

    pub(super) const fn side_px(&self, content: ContentType) -> u32 {
        self.sides[content as usize].size
    }

    pub(super) fn touch(&mut self, key: &K) -> Option<u32> {
        let &idx = self.cache.get(key)?;
        self.slots[idx as usize].last_use = self.cur_frame;
        Some(idx)
    }

    /// Insert a freshly rasterized glyph, queueing its pixels for
    /// [`Self::flush_pending_uploads`]. Grows if full; `None` only at GPU-max when it
    /// still doesn't fit.
    pub(super) fn insert(&mut self, key: K, raster: PackedRaster<'_>) -> Option<u32> {
        let PackedRaster {
            metadata, content, ..
        } = raster;
        let origin = self.allocate(content, metadata.size)?;
        self.enqueue_upload(origin.as_uvec2(), raster);

        let slot = AtlasSlot {
            placement: Some(SlotPlacement {
                origin,
                size: metadata.size,
                bearing: metadata.bearing,
                content,
            }),
            generation: 0,
            last_use: self.cur_frame,
            free: false,
        };
        Some(self.store(key, slot))
    }

    fn store(&mut self, key: K, mut slot: AtlasSlot) -> u32 {
        let idx = if let Some(i) = self.free.claim() {
            slot.generation = self.slots[i as usize].generation;
            self.slots[i as usize] = slot;
            self.slot_keys[i as usize] = key;
            i
        } else {
            self.slots.push(slot);
            self.slot_keys.push(key);
            (self.slots.len() - 1) as u32
        };
        let prev = self.cache.insert(key, idx);
        // Callers insert only after a failed `touch`, so the key must be new.
        debug_assert!(prev.is_none(), "raster inserted over a live cache entry");
        idx
    }

    /// Queue `raster`'s pixels for the rectangle at `origin`.
    fn enqueue_upload(&mut self, origin: UVec2, raster: PackedRaster<'_>) {
        let copy = PendingCopy {
            content: raster.content,
            origin,
            size: raster.metadata.size.as_uvec2(),
            pixels_start: self.pending_pixels.len(),
        };
        debug_assert_eq!(
            raster.pixels.len(),
            copy.pixels().len(),
            "a raster's bytes must be its rows with no slack",
        );
        self.pending_pixels.extend_from_slice(raster.pixels);
        self.pending_copies.push(copy);
    }

    /// Drain this frame's queued rasters onto the GPU after any pending grow blit,
    /// once per frame before any pass draws. Row padding is applied here, directly into
    /// the belt's mapped staging, which the copies read: the bytes are staged once, and
    /// no buffer of this atlas's own holds the peak frame's worth.
    ///
    /// Not `queue.write_texture`: queue writes run before every command buffer in the
    /// submit, so they would land under the grow blit, and wgpu allocates per call.
    pub(super) fn flush_pending_uploads(&mut self, ctx: &mut GpuCtx<'_>) {
        // Grow blits first: the old-to-new copy must complete before new glyph writes.
        let mut any_grow = false;
        for side in &mut self.sides {
            if let Some(pg) = side.pending_grow.take() {
                if !any_grow {
                    debug_marker::push_encoder(ctx.encoder, &self.labels.grow_blit);
                    any_grow = true;
                }
                ctx.encoder.copy_texture_to_texture(
                    pg.old_texture.as_image_copy(),
                    side.texture.as_image_copy(),
                    wgpu::Extent3d {
                        width: pg.old_size,
                        height: pg.old_size,
                        depth_or_array_layers: 1,
                    },
                );
            }
        }
        if any_grow {
            debug_marker::pop_encoder(ctx.encoder);
        }

        if self.pending_copies.is_empty() {
            return;
        }
        let bytes: u64 = self
            .pending_copies
            .iter()
            .map(|c| u64::from(c.padded_bytes_per_row()) * u64::from(c.size.y))
            .sum();
        // Every raster covers at least one row, so the staging is never the empty one `stage`
        // declines. Rows start on 256-byte boundaries, as the copies read them.
        let Staged {
            mut view,
            buffer,
            offset,
            encoder,
        } = ctx
            .stage(bytes, u64::from(COPY_BYTES_PER_ROW_ALIGNMENT))
            .expect("a queued raster stages at least one padded row");

        // One walk, so a row's staging offset and its copy's read offset are one number. Pad bytes
        // are left as the belt chunk held them.
        debug_marker::push_encoder(encoder, &self.labels.batch_upload);
        let mut at = 0usize;
        for c in &self.pending_copies {
            let row_bytes = c.bytes_per_row() as usize;
            let padded = c.padded_bytes_per_row();
            encoder.copy_buffer_to_texture(
                wgpu::TexelCopyBufferInfo {
                    buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: offset + at as u64,
                        bytes_per_row: Some(padded),
                        rows_per_image: Some(c.size.y),
                    },
                },
                wgpu::TexelCopyTextureInfo {
                    texture: &self.sides[c.content as usize].texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: c.origin.x,
                        y: c.origin.y,
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width: c.size.x,
                    height: c.size.y,
                    depth_or_array_layers: 1,
                },
            );
            for row in self.pending_pixels[c.pixels()].chunks_exact(row_bytes) {
                view.slice(at..at + row_bytes).copy_from_slice(row);
                at += padded as usize;
            }
        }
        debug_marker::pop_encoder(encoder);

        self.pending_pixels.clear();
        self.pending_copies.clear();
    }

    /// Cache a non-drawing glyph: a slab index but no packer rectangle or upload.
    pub(super) fn insert_unallocated(&mut self, key: K) -> u32 {
        self.unallocated_expiry
            .schedule(key, unallocated_dies_at(self.cur_frame));
        let slot = AtlasSlot {
            placement: None,
            generation: 0,
            last_use: self.cur_frame,
            free: false,
        };
        self.store(key, slot)
    }

    /// Advance this atlas's view of the shared clock to `frame` and retire non-drawing entries now
    /// due. Not `end_frame`: this type owns no frame boundary.
    pub(super) fn advance_to(&mut self, frame: u64) {
        debug_assert!(
            frame >= self.cur_frame,
            "the atlas frame clock ran backwards",
        );
        self.cur_frame = frame;
        let mut slab = Slab {
            cache: &mut self.cache,
            slots: &mut self.slots,
            sides: &mut self.sides,
            free: &mut self.free,
        };
        // No stamp check: deadlines only move out, so every ticket that fires is the live one.
        self.unallocated_expiry
            .retire(frame, |key, _| slab.retire_unallocated(key, frame));
    }

    /// Allocate a rectangle in the right packer, evicting then growing as needed.
    ///
    /// Eviction is gated on the entry fitting the side's edge at all: freeing rectangles
    /// cannot widen a texture, so for a wider or taller entry every victim is wasted and
    /// the loop would empty the atlas, every frame it stays on screen (the run is refused
    /// as a template; see `EncodedCache::settle`).
    fn allocate(&mut self, content: ContentType, size: U16Vec2) -> Option<U16Vec2> {
        if !self.sides[content as usize].fits_ceiling(size) {
            self.counters.oversized.bump();
            return None;
        }
        let mut evictions = 0;
        let origin = loop {
            if let Some(origin) = self.sides[content as usize].packer.allocate(size) {
                break Some(origin);
            }
            // Under budget, one grow (texture plus blit) is cheaper than an O(live glyphs)
            // eviction scan per waiting glyph. Too wide for the current edge, growing is the
            // only thing that works.
            let must_grow = !self.sides[content as usize].fits_now(size);
            let grew = (must_grow || self.eager_growth(content)) && self.grow(content);
            if grew {
                continue;
            }
            let evicted = self.evict_for(content, size);
            evictions += evicted;
            if evicted == 0 && !self.grow(content) {
                break None;
            }
        };
        self.counters
            .worst_insert_evictions
            .edit(|worst| *worst = (*worst).max(evictions));
        origin
    }

    fn eager_growth(&self, content: ContentType) -> bool {
        let side = &self.sides[content as usize];
        let bytes =
            u64::from(side.size) * u64::from(side.size) * u64::from(content.bytes_per_pixel());
        bytes < self.eager_growth_bytes
    }

    /// Evict toward room for a `size` raster of `target` content, among entries not drawn this
    /// frame, chosen by a clock; returns how many went, `0` when none could.
    ///
    /// The packer reclaims per rectangle, so a victim on a shelf that takes the raster is room
    /// once enough of its neighbors go: the clock looks there first. With no such victim, every
    /// evictable entry on the first victim's shelf goes, so its rows can serve another height.
    ///
    /// # Why a clock, not exact LRU
    ///
    /// True LRU iterates the whole `cache` map per eviction, and [`Self::allocate`]
    /// calls this in a loop; during a sustained zoom that victim selection dominated the
    /// frame. In the thrash steady state nearly every slot is eligible, so the clock hand
    /// stops within a step or two. The trade is an approximately-oldest victim, fine
    /// since entries are regenerable. See [`ClockSweep`].
    ///
    /// # What the clock protects
    ///
    /// What has been drawn so far this frame, not what it will draw
    /// ([`Self::cur_frame`] moves in [`Self::advance_to`] at the end of a submit). An
    /// early batch's misses can therefore take rectangles a later batch was about to hit;
    /// that batch's `TextEncoder::try_emit_cached` sees the generation move and
    /// re-rasterizes. It costs work, never a wrong pixel.
    ///
    /// An intrusive MRU list is the wrong tool: this atlas refreshes a stamp with a
    /// single indexed store on the hottest text path (`TextEncoder::try_emit_cached`).
    fn evict_for(&mut self, target: ContentType, size: U16Vec2) -> u32 {
        if self.sides[target as usize].dry_frame == Some(self.cur_frame) {
            // A fourth write of `last_use` would break the invariant that doc rests on, silently
            // stopping reclaim; debug pays one rotation to check it.
            debug_assert!(
                self.sweep(target, |_| true).victim.is_none(),
                "{target:?} latched dry on frame {} still has an evictable slot",
                self.cur_frame,
            );
            return 0;
        }
        let packer = &self.sides[target as usize].packer;
        let sweep = self.sweep(target, |p| {
            ShelfPacker::takes(packer.shelf_height(p.origin.y), size.y)
        });
        let any = sweep.refused;
        if let Some(idx) = self.take_sweep(sweep) {
            self.evict(idx);
            return 1;
        }
        if !any {
            self.sides[target as usize].dry_frame = Some(self.cur_frame);
            return 0;
        }
        self.evict_shelf(target, size.y)
    }

    /// With no victim on a shelf that takes a raster of height `h`, a whole shelf of `target`
    /// entries none drawn this frame, chosen to make room: the shortest tall enough, emptied for
    /// the raster to reuse, else the bottom one, whose rows go back to the side. `0` when no
    /// shelf qualifies, as emptying one in the middle that is too short frees nothing usable.
    fn evict_shelf(&mut self, target: ContentType, h: u16) -> u32 {
        let packer = &self.sides[target as usize].packer;
        let shelves = packer.shelf_count();
        let uses = &mut self.shelf_uses;
        uses.clear();
        uses.resize(shelves, ShelfUse::Empty);
        for slot in &self.slots {
            let Some(placement) = slot.placement.filter(|p| p.content == target) else {
                continue;
            };
            let shelf_use = &mut uses[packer.shelf_at(placement.origin.y)];
            if slot.last_use >= self.cur_frame {
                *shelf_use = ShelfUse::Drawn;
            } else if *shelf_use == ShelfUse::Empty {
                *shelf_use = ShelfUse::Evictable;
            }
        }
        self.counters
            .evict_scans
            .edit(|n| *n += self.slots.len() as u64);
        let evictable = |i: &usize| uses[*i] == ShelfUse::Evictable;
        let Some(row) = (0..shelves)
            .filter(evictable)
            .filter(|&i| packer.row(i).height >= h)
            .min_by_key(|&i| packer.row(i).height)
            .or_else(|| shelves.checked_sub(1).filter(evictable))
            .map(|i| packer.row(i))
        else {
            return 0;
        };
        let mut evicted = 0;
        for idx in 0..self.slots.len() as u32 {
            let on_shelf = self.slots[idx as usize]
                .placement
                .is_some_and(|p| p.content == target && p.origin.y == row.y);
            if on_shelf {
                self.evict(idx);
                evicted += 1;
            }
        }
        evicted
    }

    /// One clock turn over `target` content's evictable slots that `takes` accepts.
    fn sweep(&self, target: ContentType, takes: impl Fn(&SlotPlacement) -> bool) -> ClockSweep {
        ClockSweep::over(
            &self.slots,
            self.hand,
            Evictable {
                content: target,
                cur_frame: self.cur_frame,
                takes,
            },
        )
    }

    /// Move the hand past `sweep` and bill its walk; the victim it found, if any.
    fn take_sweep(&mut self, sweep: ClockSweep) -> Option<u32> {
        self.hand = sweep.hand;
        self.counters
            .evict_scans
            .edit(|n| *n += u64::from(sweep.examined));
        sweep.victim
    }

    /// Drop the entry at slab index `idx`, returning its rectangle to its packer.
    fn evict(&mut self, idx: u32) {
        self.counters.evictions.bump();
        let key = self.slot_keys[idx as usize];
        let removed = self.cache.remove(&key);
        debug_assert_eq!(
            removed,
            Some(idx),
            "slot_keys disagreed with cache about slab index {idx}",
        );
        self.free.release(&mut self.slots, &mut self.sides, idx);
    }

    /// Drop every entry whose key `keep` rejects, for retiring a family of keys at once
    /// (the icon backend, on unloading sets), which would otherwise hold rectangles
    /// until pressure swept them. Walks [`Self::cache`], not the slab; `keep` must
    /// answer for a set of doomed keys in one pass.
    pub(super) fn forget(&mut self, keep: impl Fn(&K) -> bool) {
        let Self {
            cache,
            slots,
            sides,
            free,
            ..
        } = self;
        cache.retain(|key, &mut idx| {
            if keep(key) {
                return true;
            }
            free.release(slots, sides, idx);
            false
        });
    }

    fn grow(&mut self, content: ContentType) -> bool {
        if !self.sides[content as usize].grow(&self.device) {
            return false;
        }
        self.counters.grows.bump();
        // Rebind now, not behind a dirty flag: the old texture is gone, so any frame between would
        // sample a destroyed view.
        self.bound.rebind(&self.device, &self.sides);
        true
    }
}

/// The slab and what a released slot returns to, borrowed apart from
/// [`RasterAtlas::unallocated_expiry`] so its drain can re-file into it.
#[derive(Debug)]
struct Slab<'a, K> {
    cache: &'a mut FxHashMap<K, u32>,
    slots: &'a mut [AtlasSlot],
    sides: &'a mut [Side],
    free: &'a mut FreeSlots,
}

impl<K: Copy + Eq + Hash + Debug> Slab<'_, K> {
    /// Settle one drained non-drawing ticket: `Some(due)` to re-file, `None` once
    /// reclaimed or no longer this wheel's business.
    fn retire_unallocated(&mut self, key: K, frame: u64) -> Option<u64> {
        let Entry::Occupied(entry) = self.cache.entry(key) else {
            return None;
        };
        let idx = *entry.get();
        let slot = &self.slots[idx as usize];
        // Allocated entries are the clock's to reclaim. Defensive: every path allocating over a slab
        // index removes the old key first.
        if slot.placement.is_some() {
            return None;
        }
        let dies_at = unallocated_dies_at(slot.last_use);
        if dies_at > frame {
            return Some(dies_at);
        }
        entry.remove();
        self.free.release(self.slots, self.sides, idx);
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
    pub(crate) const fn unallocated_dies_at(last_use: u64) -> u64 {
        super::unallocated_dies_at(last_use)
    }
}

#[cfg(test)]
mod tests;
