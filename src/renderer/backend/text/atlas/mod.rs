//! Glyph atlas: one struct for both mask + color content.

use crate::common::expiry_wheel::ExpiryWheel;
use crate::common::probe::BenchOnly;
use crate::renderer::backend::debug_marker;
use crate::text::render::{GlyphPlacement, GlyphRasterKey};
use etagere::{AllocId, BucketedAtlasAllocator, size2};
use rustc_hash::FxHashMap;
use wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;

use crate::renderer::backend::gpu_ctx::GpuCtx;
use crate::renderer::backend::text::ContentType;

/// Initial mask-atlas side length. Bumped from glyphon's 256 to skip
/// the 256→512→1024 grow chain on first frame with non-trivial text.
const INITIAL_MASK_ATLAS_SIZE: u32 = 1024;
/// Initial color-atlas side length. Color glyphs (emoji) are rare in
/// UI text: 256² RGBA is 256 KB and holds dozens at UI sizes, where
/// matching the mask side's 1024² would pin 4 MB of GPU memory most
/// sessions never touch. Grows on demand through the same blit path.
const INITIAL_COLOR_ATLAS_SIZE: u32 = 256;
const ATLAS_GROWTH_FACTOR: u32 = 2;

/// Hard ceiling on a side's backing texture, whatever the device allows.
///
/// [`GlyphAtlas::grow`] used to stop only at `max_texture_dimension_2d`,
/// which on desktop adapters is routinely 16384 — a 256 MB mask or a
/// 1 GB colour atlas, for text. Nothing observed reaches that, but the
/// failure mode if anything did is far worse than the alternative:
/// refusing to grow yields `Rasterized::AtlasFull`, whose only cost is
/// that the run re-encodes each frame instead of being cached.
///
/// 16 MiB is `2^24`, and both `bytes_per_pixel` values are powers of
/// two, so [`growth_ceiling`] divides and square-roots to an exact
/// power-of-two side on either: a 4096² mask or a 2048² colour atlas.
/// The measured `text_atlas/cache_churn` working set is 3700 glyphs in
/// a 2048² mask, so the mask ceiling is roughly 4x the largest set any
/// bench here produces.
const MAX_ATLAS_BYTE_BUDGET: u64 = 16 << 20;

/// Byte budget below which [`GlyphAtlas::allocate`] grows a side rather
/// than evicting from it.
///
/// `allocate` used to try eviction first unconditionally, so an atlas
/// never outgrew its initial size no matter how badly it fit: measured
/// on `text_atlas/cache_churn`, a 1024² mask holding ~1k live glyphs
/// performed 2668 evictions and *zero* growths, walking 4.06M cache
/// entries to pick victims — `evict_one` is O(live glyphs) and
/// `allocate` calls it in a loop, so the scan repeats for every glyph
/// the gesture brings in.
///
/// Sizing to the working set first turns that into one texture
/// allocation plus a preserved-rect blit: the same arm now performs a
/// single growth, zero evictions and zero scanning, and its frame drops
/// from 609 µs to 61 µs — from a 10x outlier among the `text_atlas`
/// arms to the same ~55-60 µs band as the rest. Its real working set is
/// 3700 glyphs, so a 1024² mask was recycling roughly seven of every
/// ten rasters it held.
///
/// The budget is what keeps that from being unbounded — without a
/// ceiling a thrashing atlas would run to the device maximum, which for
/// an R8 mask is 8192² = 64 MB. In bytes rather than pixels so both
/// sides get the same deal: 4 MiB is a 2048² mask or a 1024² colour
/// atlas, and the mask growing 1 MB -> 4 MB is what the measurement
/// above cost.
const EAGER_GROWTH_BYTE_BUDGET: u64 = 4 << 20;

/// Frames a non-drawing entry (`alloc: None`) survives unused.
/// `evict_one` skips them — there is no rectangle to deallocate — so
/// every whitespace or rejected glyph at every scale rung would
/// otherwise accumulate forever and bloat its linear scan. 512 ≈ 8 s at
/// 60 fps, far outside any flicker.
///
/// A per-entry deadline on [`GlyphAtlas::unallocated_expiry`], not a
/// cadence. It used to be a periodic `cache.retain` over the whole glyph
/// map, which is the shape this crate avoids everywhere else: one frame
/// in 512 paying for all of them. It also had to be spelled as a
/// threshold rather than `frame % INTERVAL == 0`, because the shared
/// clock can advance by more than one and a modulo gate steps over its
/// own trigger. A wheel has neither problem, and retires each entry on
/// its own last use instead of rounding every entry to a shared tick.
const UNALLOCATED_SWEEP_INTERVAL: u64 = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PackedGlyphMetadata {
    width: u16,
    height: u16,
    left: i16,
    top: i16,
}

impl PackedGlyphMetadata {
    pub(super) const EMPTY: Self = Self {
        width: 0,
        height: 0,
        left: 0,
        top: 0,
    };

    pub(super) fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }
}

impl TryFrom<&GlyphPlacement> for PackedGlyphMetadata {
    type Error = std::num::TryFromIntError;

    fn try_from(placement: &GlyphPlacement) -> Result<Self, Self::Error> {
        Ok(Self {
            width: placement.width.try_into()?,
            height: placement.height.try_into()?,
            left: placement.left.try_into()?,
            top: placement.top.try_into()?,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct GlyphSlot {
    pub(super) x: u16,
    pub(super) y: u16,
    pub(super) width: u16,
    pub(super) height: u16,
    pub(super) left: i16,
    pub(super) top: i16,
    pub(super) content: ContentType,
    pub(super) alloc: Option<AllocId>,
    pub(super) generation: u32,
    pub(super) last_use: u64,
}

/// One per-content-type backing store. Indexed by `ContentType as usize`.
struct Side {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    size: u32,
    packer: BucketedAtlasAllocator,
    /// On grow, the previous-frame texture is moved here so the
    /// shared-encoder flush can record the copy alongside pending
    /// glyph writes. `None` whenever there's no pending grow blit
    /// for this side.
    pending_grow: Option<PendingGrow>,
}

/// Old texture + its size (= square edge length, == old.width ==
/// old.height) preserved across the grow point. Consumed by
/// `flush_pending_uploads`.
#[derive(Debug)]
struct PendingGrow {
    old_texture: wgpu::Texture,
    old_size: u32,
}

#[derive(Debug)]
pub(super) struct AtlasBindings<'a> {
    pub(super) mask_view: &'a wgpu::TextureView,
    pub(super) color_view: &'a wgpu::TextureView,
    pub(super) atlas_px: [u32; 2],
}

#[derive(Debug)]
pub(super) struct GlyphAtlas {
    sides: [Side; 2],
    /// Dense slot slab; `cache` maps each key to an index into it.
    /// Encoded-run caches record these indices so their hot-path LRU
    /// refresh is an indexed store instead of a map probe per glyph —
    /// safe because every recorded index carries the slot generation
    /// that `evict_one` advances before making the index reusable.
    pub(super) slots: Vec<GlyphSlot>,
    /// Key held by each slab entry, parallel to [`Self::slots`]. The
    /// reverse of [`Self::cache`], and the only reason eviction can pick
    /// a victim by slab position at all — it has to drop the outgoing
    /// glyph's map entry, and the map alone only answers key → index.
    ///
    /// A parallel column rather than a field on `GlyphSlot`: the slot is
    /// hot (copied whole by `encode_run`, and read per glyph by
    /// `try_emit_cached`) while the key is touched only when a slot is
    /// stored or evicted, so folding a ~24-byte key in would cost the
    /// hot path density for a cold path's convenience.
    slot_keys: Vec<GlyphRasterKey>,
    pub(super) cache: FxHashMap<GlyphRasterKey, u32>,
    /// Slab indices freed by `evict_one` / the empty sweep, reused by
    /// the next `store`.
    free: Vec<u32>,
    /// Rotating eviction cursor over [`Self::slots`] — see
    /// [`Self::evict_one`]. Persists across calls, which is the whole
    /// point: it is what turns the victim search from a scan of the
    /// whole slab per eviction into a walk that resumes where the last
    /// one stopped.
    hand: u32,
    /// Latest value of the shaper's shared frame clock, mirrored here by
    /// [`Self::end_frame`] — not a count of this atlas's own frames.
    ///
    /// The atlas used to increment per submit while the shaped-buffer
    /// cache incremented per recorded frame, so the two retention
    /// windows `RENDERED_RUN_KEEP_FRAMES` derives were denominated in
    /// different units and drifted apart in both directions (a recorded
    /// frame that drew no text aged buffers only; a `PaintOnly` frame
    /// aged the atlas only). Reading one clock is what makes the shared
    /// constant mean what it says.
    pub(super) current_frame: u64,
    /// Deadlines for non-drawing entries, which `evict_one` cannot
    /// reclaim. Same file-once/re-file-on-fire protocol as the two
    /// caches above this one — see [`ExpiryWheel`] — so `touch` stays a
    /// single indexed store on the hot path and files nothing.
    unallocated_expiry: ExpiryWheel<GlyphRasterKey>,
    max_texture_dimension_2d: u32,
    /// Set on grow; the renderer rebuilds its bind group and clears it.
    pub(super) bind_group_dirty: bool,
    /// Evictions performed, growths performed, and — the number audit
    /// F2 turns on — cache entries *examined* choosing a victim.
    ///
    /// `evict_one` is O(live glyphs) and `allocate` calls it in a loop,
    /// so the eviction count alone understates the cost by whatever the
    /// cache happens to hold. `scanned` is the product that actually
    /// bills, and the only one that says whether this atlas reaches the
    /// region where F2's curve matters.
    pub(super) probe: AtlasProbe,

    /// Glyph pixel data queued by `insert`, packed with per-row padding
    /// so each glyph's copy can satisfy
    /// `wgpu::COPY_BYTES_PER_ROW_ALIGNMENT = 256`. Drained by
    /// [`Self::flush_pending_uploads`] into one staging buffer + one
    /// encoder with N `copy_buffer_to_texture` commands.
    pending_staging: Vec<u8>,
    pending_staging_used: usize,
    pending_copies: Vec<PendingCopy>,
    /// Retained staging buffer; grown on demand, reused across frames.
    staging_buf: Option<wgpu::Buffer>,
}

#[derive(Clone, Copy, Debug)]
struct PendingCopy {
    side: u8,
    origin_x: u32,
    origin_y: u32,
    width: u32,
    height: u32,
    bytes_per_row: u32,
    staging_offset: u64,
}

impl GlyphAtlas {
    pub(super) fn new(device: &wgpu::Device) -> Self {
        let max = device.limits().max_texture_dimension_2d;

        // Order matches `ContentType as usize`: [Mask, Color].
        let sides = [
            Side::new(device, ContentType::Mask, INITIAL_MASK_ATLAS_SIZE.min(max)),
            Side::new(
                device,
                ContentType::Color,
                INITIAL_COLOR_ATLAS_SIZE.min(max),
            ),
        ];

        Self {
            sides,
            slots: Vec::new(),
            slot_keys: Vec::new(),
            cache: FxHashMap::default(),
            free: Vec::new(),
            hand: 0,
            current_frame: 0,
            unallocated_expiry: ExpiryWheel::with_horizon(UNALLOCATED_SWEEP_INTERVAL + 2),
            max_texture_dimension_2d: max,
            bind_group_dirty: false,
            probe: AtlasProbe::default(),
            pending_staging: Vec::new(),
            pending_staging_used: 0,
            pending_copies: Vec::new(),
            staging_buf: None,
        }
    }

    pub(super) fn bindings(&self) -> AtlasBindings<'_> {
        let mask = &self.sides[ContentType::Mask as usize];
        let color = &self.sides[ContentType::Color as usize];
        AtlasBindings {
            mask_view: &mask.view,
            color_view: &color.view,
            atlas_px: [color.size, mask.size],
        }
    }

    /// Cache-hit fast path: bump the slot's LRU stamp and return its
    /// slab index (read the slot itself via `self.slots[idx]`).
    pub(super) fn touch(&mut self, key: &GlyphRasterKey) -> Option<u32> {
        let &idx = self.cache.get(key)?;
        self.slots[idx as usize].last_use = self.current_frame;
        Some(idx)
    }

    /// Insert a freshly-rasterized glyph. Queues the pixel data into
    /// a per-frame staging buffer (drained by
    /// [`Self::flush_pending_uploads`] before the text pass) so all
    /// glyph uploads land in one encoder/submit instead of N separate
    /// `queue.write_texture` calls. Grows if full; returns `None`
    /// only at GPU-max and still doesn't fit. On success returns the
    /// new slot's slab index.
    pub(super) fn insert(
        &mut self,
        device: &wgpu::Device,
        key: GlyphRasterKey,
        content: ContentType,
        metadata: PackedGlyphMetadata,
        pixels: &[u8],
    ) -> Option<u32> {
        let alloc = self.allocate(device, content, metadata.width, metadata.height)?;
        self.enqueue_upload(
            content,
            alloc.rectangle.min.x as u32,
            alloc.rectangle.min.y as u32,
            metadata.width as u32,
            metadata.height as u32,
            pixels,
        );

        let slot = GlyphSlot {
            x: alloc.rectangle.min.x as u16,
            y: alloc.rectangle.min.y as u16,
            width: metadata.width,
            height: metadata.height,
            left: metadata.left,
            top: metadata.top,
            content,
            alloc: Some(alloc.id),
            generation: 0,
            last_use: self.current_frame,
        };
        Some(self.store(key, slot))
    }

    /// Park `slot` in the slab (reusing a freed index when available)
    /// and map `key` to it.
    fn store(&mut self, key: GlyphRasterKey, mut slot: GlyphSlot) -> u32 {
        let idx = match self.free.pop() {
            Some(i) => {
                slot.generation = self.slots[i as usize].generation;
                self.slots[i as usize] = slot;
                self.slot_keys[i as usize] = key;
                i
            }
            None => {
                self.slots.push(slot);
                self.slot_keys.push(key);
                (self.slots.len() - 1) as u32
            }
        };
        let prev = self.cache.insert(key, idx);
        // A double-insert would leak the previous slab slot; callers
        // only insert after a failed `touch`, so the key must be new.
        assert!(prev.is_none(), "glyph inserted over a live cache entry");
        idx
    }

    /// Append one glyph's pixel data to the pending-upload staging
    /// vec, padding each row out to `COPY_BYTES_PER_ROW_ALIGNMENT` so
    /// `copy_buffer_to_texture` can consume it. The per-glyph
    /// staging-buffer offset is 256-aligned by construction (rows
    /// pad to 256), satisfying both the row-pitch and buffer-offset
    /// alignment requirements.
    fn enqueue_upload(
        &mut self,
        content: ContentType,
        origin_x: u32,
        origin_y: u32,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) {
        let bpp = content.bytes_per_pixel();
        let unpadded = width * bpp;
        let bytes_per_row = unpadded.next_multiple_of(COPY_BYTES_PER_ROW_ALIGNMENT);
        // Every queued region is `bytes_per_row × height` with
        // `bytes_per_row` a multiple of 256, so the append offset is
        // 256-aligned by construction — the buffer-offset and row-pitch
        // alignment requirements hold for every PendingCopy.
        let region_start = self.pending_staging_used;
        assert!(region_start.is_multiple_of(COPY_BYTES_PER_ROW_ALIGNMENT as usize));
        let region_bytes = bytes_per_row as usize * height as usize;
        self.pending_staging_used += region_bytes;
        if self.pending_staging.len() < self.pending_staging_used {
            self.pending_staging.resize(self.pending_staging_used, 0);
        }
        for row in 0..height as usize {
            let src = &pixels[row * unpadded as usize..(row + 1) * unpadded as usize];
            let dst_off = region_start + row * bytes_per_row as usize;
            self.pending_staging[dst_off..dst_off + unpadded as usize].copy_from_slice(src);
        }
        self.pending_copies.push(PendingCopy {
            side: content as u8,
            origin_x,
            origin_y,
            width,
            height,
            bytes_per_row,
            staging_offset: region_start as u64,
        });
    }

    /// Drain queued uploads through `ctx`: the per-glyph bytes are
    /// staged through the renderer's shared staging belt (one
    /// `copy_buffer_to_buffer` into our retained staging buffer), plus
    /// N `copy_buffer_to_texture` commands recorded on `ctx.encoder`.
    /// The renderer owns the submit; this method adds no extra one.
    pub(super) fn flush_pending_uploads(&mut self, ctx: &mut GpuCtx<'_>) {
        // Grow blits first: old→new copy must complete before any new
        // glyph writes hit the new texture. wgpu serialises commands
        // within an encoder, so recording in this order is enough.
        let mut any_grow = false;
        for side in &mut self.sides {
            if let Some(pg) = side.pending_grow.take() {
                if !any_grow {
                    debug_marker::push_encoder(ctx.encoder, "palantir text atlas grow blit");
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
        let bytes = self.pending_staging_used as u64;
        let current_cap = self.staging_buf.as_ref().map_or(0, wgpu::Buffer::size);
        if bytes > current_cap {
            let new_cap = bytes.next_power_of_two().max(current_cap * 2).max(4096);
            self.staging_buf = Some(ctx.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("palantir text atlas staging"),
                size: new_cap,
                usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let buf = self.staging_buf.as_ref().unwrap();
        ctx.write(buf, 0, &self.pending_staging[..self.pending_staging_used]);

        debug_marker::push_encoder(ctx.encoder, "palantir text atlas batch upload");
        for c in &self.pending_copies {
            let side = &self.sides[c.side as usize];
            ctx.encoder.copy_buffer_to_texture(
                wgpu::TexelCopyBufferInfo {
                    buffer: buf,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: c.staging_offset,
                        bytes_per_row: Some(c.bytes_per_row),
                        rows_per_image: Some(c.height),
                    },
                },
                wgpu::TexelCopyTextureInfo {
                    texture: &side.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: c.origin_x,
                        y: c.origin_y,
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::Extent3d {
                    width: c.width,
                    height: c.height,
                    depth_or_array_layers: 1,
                },
            );
        }
        debug_marker::pop_encoder(ctx.encoder);

        self.pending_staging_used = 0;
        self.pending_copies.clear();
    }

    /// Cache a non-drawing glyph (no atlas slot or upload). Subsequent
    /// lookups still hit the cache and skip swash.
    pub(super) fn insert_unallocated(
        &mut self,
        key: GlyphRasterKey,
        content: ContentType,
        metadata: PackedGlyphMetadata,
    ) -> u32 {
        debug_assert!(metadata.is_empty());
        self.unallocated_expiry
            .schedule(key, self.current_frame + UNALLOCATED_SWEEP_INTERVAL + 1);
        let slot = GlyphSlot {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
            left: metadata.left,
            top: metadata.top,
            content,
            alloc: None,
            generation: 0,
            last_use: self.current_frame,
        };
        self.store(key, slot)
    }

    /// Frame teardown: take the shaper's `frame` clock and retire the
    /// non-drawing entries whose deadline came due on it.
    pub(super) fn end_frame(&mut self, frame: u64) {
        debug_assert!(
            frame >= self.current_frame,
            "the shared frame clock ran backwards",
        );
        self.current_frame = frame;
        let cache = &mut self.cache;
        let slots = &self.slots;
        let free = &mut self.free;
        self.unallocated_expiry.retire(frame, |key| {
            retire_unallocated(cache, slots, free, key, frame)
        });
    }

    /// Allocate a slot in the right packer, evicting then growing as
    /// needed.
    fn allocate(
        &mut self,
        device: &wgpu::Device,
        content: ContentType,
        width: u16,
        height: u16,
    ) -> Option<etagere::Allocation> {
        let need = size2(width as i32, height as i32);
        loop {
            if let Some(a) = self.sides[content as usize].packer.allocate(need) {
                return Some(a);
            }
            // Under the budget, buy space before paying for a victim:
            // one grow is a texture plus a rect-preserving blit, while
            // eviction is an O(live glyphs) scan that this loop repeats
            // for every glyph still waiting.
            let grew = self.eager_growth(content) && self.grow(device, content);
            // Past the budget — or already at the device maximum, where
            // the grow above returned false — the atlas holds its size
            // by recycling rectangles instead.
            if !grew && !self.evict_one(content) && !self.grow(device, content) {
                return None;
            }
        }
    }

    /// Whether `content`'s side is still small enough to grow in
    /// preference to evicting — see [`EAGER_GROWTH_BYTE_BUDGET`].
    fn eager_growth(&self, content: ContentType) -> bool {
        let side = &self.sides[content as usize];
        let bytes =
            u64::from(side.size) * u64::from(side.size) * u64::from(content.bytes_per_pixel());
        bytes < EAGER_GROWTH_BYTE_BUDGET
    }

    /// Evict one glyph of `target` content that was not drawn this
    /// frame, chosen by a **clock** — a persistent hand that walks
    /// [`Self::slots`] and takes the first entry it finds eligible,
    /// resuming next call where this one stopped.
    ///
    /// # Why not exact LRU
    ///
    /// This used to pick the true least-recently-used entry, which meant
    /// iterating the whole `cache` map and indexing `slots` per entry —
    /// O(live glyphs) *per eviction*, and [`Self::allocate`] calls this
    /// in a loop, so one insert could pay the walk several times over.
    /// Measured at 6.0 ns per live glyph, dead linear from 1k to 32k
    /// entries. Driving a real zoom at the production
    /// `TEXT_SCALE_STEP` through this atlas, the mask side fills at
    /// roughly 400 rungs — about two seconds of a continuous zoom — and
    /// then thrash-evicts for the rest of the gesture, at up to 76
    /// evictions a frame over ~9k live glyphs: **3.3 ms per frame of
    /// pure victim selection**, sustained, on top of the rasterization
    /// the zoom already owes.
    ///
    /// A clock makes that O(1) in the state that matters. In the thrash
    /// steady state nearly every slot is eligible — only the handful
    /// touched this frame are not — so the hand stops within a step or
    /// two, and the whole-slab walk happens at most once per full
    /// rotation instead of once per eviction.
    ///
    /// The trade is that the victim is approximately, not exactly, the
    /// oldest. That is the standard bargain for an atlas: entries are
    /// rasterizations, regenerable from the font at any time, and the
    /// working set is protected regardless because anything drawn this
    /// frame is skipped outright.
    ///
    /// **An intrusive MRU list is the wrong tool here**, though the
    /// gradient atlas has one. Its `touch` is move-to-head, ~six link
    /// writes; this atlas refreshes a slot's stamp with a *single
    /// indexed store* per glyph on the encoded-cache hit path
    /// (`TextEncoder::try_emit_cached`), which is the hottest path in
    /// text rendering. Move-to-head would tax that to speed this up.
    fn evict_one(&mut self, target: ContentType) -> bool {
        let sweep = clock_victim(&self.slots, self.hand, target, self.current_frame);
        self.hand = sweep.hand;
        self.probe.evict_scans.edit(|n| *n += sweep.examined as u64);
        let Some(idx) = sweep.victim else {
            return false;
        };
        self.probe.evictions.bump();
        let key = self.slot_keys[idx as usize];
        let removed = self.cache.remove(&key);
        debug_assert_eq!(
            removed,
            Some(idx),
            "slot_keys disagreed with cache about slab index {idx}",
        );
        let slot = &mut self.slots[idx as usize];
        let id = slot.alloc.take().unwrap();
        slot.generation = slot
            .generation
            .checked_add(1)
            .expect("glyph slot generation overflowed");
        self.sides[target as usize].packer.deallocate(id);
        self.free.push(idx);
        true
    }

    /// Double the atlas of `content`. Returns `false` at GPU-max. On
    /// success, stashes the old texture into `Side::pending_grow` so
    /// `flush_pending_uploads` can record the old→new blit on the
    /// shared encoder. etagere preserves rects on `packer.grow`, so
    /// the cache stays valid — no re-rasterization.
    fn grow(&mut self, device: &wgpu::Device, content: ContentType) -> bool {
        let ceiling = growth_ceiling(self.max_texture_dimension_2d, content);
        let side = &mut self.sides[content as usize];
        if side.size >= ceiling {
            return false;
        }
        self.probe.grows.bump();
        let new_size = (side.size * ATLAS_GROWTH_FACTOR).min(ceiling);
        let new_texture = make_texture(device, content.format(), new_size, content.label());
        let old_size = side.size;
        let old_texture = std::mem::replace(&mut side.texture, new_texture);

        // If a previous grow this frame hasn't flushed yet, keep the
        // oldest texture — that's the one holding live pixel data
        // (the intermediate-size texture was never written into).
        if side.pending_grow.is_none() {
            side.pending_grow = Some(PendingGrow {
                old_texture,
                old_size,
            });
        }

        side.view = side
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        side.size = new_size;
        side.packer.grow(size2(new_size as i32, new_size as i32));
        self.bind_group_dirty = true;
        true
    }
}

// Manual: etagere's `BucketedAtlasAllocator` isn't `Debug`.
impl std::fmt::Debug for Side {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Side")
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}

impl Side {
    fn new(device: &wgpu::Device, content: ContentType, size: u32) -> Self {
        let texture = make_texture(device, content.format(), size, content.label());
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            texture,
            view,
            size,
            packer: BucketedAtlasAllocator::new(size2(size as i32, size as i32)),
            pending_grow: None,
        }
    }
}

/// Largest side length a `content` atlas will grow to: whichever of the
/// device maximum and [`MAX_ATLAS_BYTE_BUDGET`] binds first.
///
/// A free function taking the device limit rather than a method, so the
/// arithmetic is testable without a `wgpu::Device`.
fn growth_ceiling(max_texture_dimension_2d: u32, content: ContentType) -> u32 {
    let by_bytes = (MAX_ATLAS_BYTE_BUDGET / u64::from(content.bytes_per_pixel())).isqrt() as u32;
    max_texture_dimension_2d.min(by_bytes)
}

/// Settle one drained non-drawing ticket: `Some(due)` to re-file it,
/// `None` once it has been reclaimed or is no longer this wheel's
/// business.
///
/// A reclaimed entry re-inserts through `insert_unallocated` on next
/// use, with a fresh ticket. Unallocated slots carry no uv coords and
/// encoded-run caches never record them, so returning one to `free` does
/// not advance its generation.
///
/// A free function, and one that borrows the three fields rather than
/// `&mut self`, so the caller can hold `unallocated_expiry` borrowed
/// across the call to re-file into it.
fn retire_unallocated(
    cache: &mut FxHashMap<GlyphRasterKey, u32>,
    slots: &[GlyphSlot],
    free: &mut Vec<u32>,
    key: GlyphRasterKey,
    frame: u64,
) -> Option<u64> {
    // Gone: reclaimed by an earlier ticket, or its key removed by
    // `evict_one`.
    let &idx = cache.get(&key)?;
    let slot = &slots[idx as usize];
    // Allocated entries are `evict_one`'s to reclaim, and it advances
    // their generation when it does. Defensive rather than reachable —
    // every path that allocates over a slab index removes the old key
    // from `cache` first, so the lookup above would have missed.
    if slot.alloc.is_some() {
        return None;
    }
    // `touch` refreshes `last_use` without filing anything, so the real
    // deadline is re-read here.
    let dies_at = slot.last_use + UNALLOCATED_SWEEP_INTERVAL + 1;
    if dies_at > frame {
        return Some(dies_at);
    }
    cache.remove(&key);
    free.push(idx);
    None
}

/// One turn of [`GlyphAtlas::evict_one`]'s clock: where the hand ended
/// up, what it found, and how far it walked.
///
/// A named result rather than a tuple, and a free function rather than a
/// method, so the policy is testable against a hand-built slab with no
/// `wgpu::Device` in sight — the hand's persistence across calls is the
/// property most worth pinning and the least visible from outside.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ClockSweep {
    victim: Option<u32>,
    /// Where the next sweep resumes. Past the victim, not on it: the
    /// slot just evicted is about to be refilled, and starting there
    /// would make the next eviction reconsider it first.
    hand: u32,
    /// Slots examined. What [`AtlasProbe::evict_scans`] bills, and the
    /// number that says whether the clock is behaving — a healthy thrash
    /// state stops after one or two.
    examined: u32,
}

/// Advance `hand` over `slots` until it meets an entry eligible for
/// eviction: allocated, of `target` content, and not drawn on
/// `current_frame`. Gives up after one full rotation.
fn clock_victim(
    slots: &[GlyphSlot],
    hand: u32,
    target: ContentType,
    current_frame: u64,
) -> ClockSweep {
    let n = slots.len();
    if n == 0 {
        return ClockSweep {
            victim: None,
            hand: 0,
            examined: 0,
        };
    }
    // `slots` only ever grows, but a hand parked at the old length is
    // still possible after a `store` that pushed — wrap it in.
    let mut at = hand as usize % n;
    for examined in 1..=n {
        let idx = at;
        at = if at + 1 == n { 0 } else { at + 1 };
        let slot = &slots[idx];
        if slot.content == target && slot.alloc.is_some() && slot.last_use < current_frame {
            return ClockSweep {
                victim: Some(idx as u32),
                hand: at as u32,
                examined: examined as u32,
            };
        }
    }
    ClockSweep {
        victim: None,
        hand: at as u32,
        examined: n as u32,
    }
}

fn make_texture(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    size: u32,
    label: &str,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

/// What the glyph atlas paid to keep itself packed. Zero-sized outside
/// benchmark and test builds.
///
/// `BenchOnly` rather than `TestOnly`: the question these answer — does a
/// real workload drive the atlas into the regime where `evict_one`'s
/// linear scan bills — is only reachable from `text_atlas`, which the
/// `internals` feature gates.
#[derive(Debug, Default)]
pub(super) struct AtlasProbe {
    pub(super) evictions: BenchOnly<u32>,
    pub(super) grows: BenchOnly<u32>,
    /// Cache entries walked by `eviction_candidate`, summed. The product
    /// F2 is about.
    pub(super) evict_scans: BenchOnly<u64>,
}

/// Reads are gated with their sole consumer: the `text_atlas`
/// benchmark, which `internals` gates too. A plain `cargo test` build
/// has no caller.
#[cfg(feature = "internals")]
impl AtlasProbe {
    pub(super) fn counts(&self) -> AtlasCounts {
        AtlasCounts {
            evictions: self.evictions.count(),
            grows: self.grows.count(),
            evict_scans: *self.evict_scans.get(),
        }
    }
}

/// One reading of an [`AtlasProbe`].
#[cfg(feature = "internals")]
#[derive(Clone, Copy, Debug)]
pub(super) struct AtlasCounts {
    pub(super) evictions: u32,
    pub(super) grows: u32,
    pub(super) evict_scans: u64,
}

#[cfg(test)]
mod tests;
