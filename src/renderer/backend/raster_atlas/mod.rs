//! Rasterized-quad atlas: one struct for both mask + colour content, keyed by
//! whatever its tenant rasterizes from.
//!
//! Two instances exist. The text backend keys one on a glyph's cosmic
//! `CacheKey`; the icon backend keys another on
//! [`IconRasterKey`](crate::icons::icon_raster_key::IconRasterKey). They share
//! every policy below — bucketed packing, clock-sweep eviction, grow-with-blit,
//! and batched staging uploads — and share nothing else: separate textures,
//! separate bind groups, separate eviction budgets.

use crate::common::counters::BenchOnly;
use crate::common::expiry_wheel::ExpiryWheel;
pub(crate) mod quad;

use crate::renderer::backend::debug_marker;
use etagere::{AllocId, BucketedAtlasAllocator, size2};
use rustc_hash::FxHashMap;
use std::fmt::Debug;
use std::hash::Hash;
use wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;

use crate::renderer::backend::gpu_ctx::GpuCtx;

/// Which of an atlas's two sides content lives on. `Mask` is one coverage
/// byte per texel and takes the draw's colour; `Color` is straight sRGB RGBA
/// and supplies its own.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum ContentType {
    Mask = 0,
    Color = 1,
}

impl ContentType {
    pub(crate) fn format(self) -> wgpu::TextureFormat {
        match self {
            Self::Mask => wgpu::TextureFormat::R8Unorm,
            Self::Color => wgpu::TextureFormat::Rgba8UnormSrgb,
        }
    }

    pub(crate) fn bytes_per_pixel(self) -> u32 {
        match self {
            Self::Mask => 1,
            Self::Color => 4,
        }
    }

    fn side_name(self) -> &'static str {
        match self {
            Self::Mask => "mask",
            Self::Color => "color",
        }
    }
}

/// How one [`RasterAtlas`] differs from the other: what it calls itself in GPU
/// debug labels, and how big each side starts.
///
/// Initial sizes are a tenant's judgement about its own content, not a shared
/// default — a session full of text and no emoji wants the opposite split from
/// one full of colour icons.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RasterAtlasConfig {
    /// Label stem, e.g. `"palantir.text"`. Every texture, marker, and buffer
    /// this atlas creates is named from it.
    pub(crate) label: &'static str,
    pub(crate) initial_mask_px: u32,
    pub(crate) initial_color_px: u32,
    /// Hard ceiling on one side's backing texture, whatever the device allows.
    ///
    /// Per instance rather than shared, because the two tenants store
    /// different things: the budget is in *bytes*, so it buys a mask side four
    /// times the side length it buys a colour side, and a number tuned against
    /// 1-byte glyph coverage is not automatically the right number for 4-byte
    /// colour rasters.
    ///
    /// Stopping growth at `max_texture_dimension_2d` alone admits 16384 on a
    /// routine desktop adapter — a 256 MB mask or a 1 GB colour atlas. The
    /// failure mode when this binds is mild: refusing to grow yields
    /// `Rasterized::AtlasFull`, whose only cost is that the entry re-encodes
    /// each frame instead of being cached.
    pub(crate) max_bytes: u64,
    /// Byte budget below which [`RasterAtlas::allocate`] grows a side rather
    /// than evicting from it.
    ///
    /// Trying eviction first unconditionally would pin an atlas at its initial
    /// size no matter how badly it fits: measured that way on
    /// `text_atlas/cache_churn`, a 1024² mask holding ~1k live glyphs performs
    /// 2668 evictions and *zero* growths, walking 4.06M cache entries to pick
    /// victims — `evict_one` is O(live entries) and `allocate` calls it in a
    /// loop. Sizing to the working set first turns that into one texture
    /// allocation plus a preserved-rect blit: one growth, zero evictions, and
    /// 61 µs a frame rather than 609 µs.
    pub(crate) eager_growth_bytes: u64,
}

/// GPU debug labels, built once at construction. Held as owned strings rather
/// than formatted per flush because two of them are pushed on the encoder
/// every frame that uploads, and a per-frame `format!` is exactly the
/// allocation this crate does not do.
#[derive(Debug)]
struct AtlasLabels {
    /// The configured stem, kept so `grow` can re-derive a texture name.
    stem: &'static str,
    grow_blit: String,
    batch_upload: String,
    staging: String,
}

const ATLAS_GROWTH_FACTOR: u32 = 2;

/// Frames a non-drawing entry (`alloc: None`) survives unused.
/// `evict_one` skips them — there is no rectangle to deallocate — so
/// every whitespace or rejected glyph at every scale rung would
/// otherwise accumulate forever, lengthening the slab the clock hand
/// walks without ever offering it a victim. 512 ≈ 8 s at 60 fps, far
/// outside any flicker.
///
/// A per-entry deadline on [`RasterAtlas::unallocated_expiry`], not a
/// cadence. A periodic `cache.retain` over the whole glyph map would be
/// the shape this crate avoids everywhere else — one frame in 512 paying
/// for all of them — and it would have to be spelled as a threshold
/// rather than `frame % INTERVAL == 0`, because the shared clock can
/// advance by more than one and a modulo gate steps over its own
/// trigger. A wheel has neither problem, and retires each entry on its
/// own last use instead of rounding every entry to a shared tick.
const UNALLOCATED_SWEEP_INTERVAL: u64 = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PackedMetadata {
    width: u16,
    height: u16,
    left: i16,
    top: i16,
}

impl PackedMetadata {
    pub(crate) const EMPTY: Self = Self {
        width: 0,
        height: 0,
        left: 0,
        top: 0,
    };

    pub(crate) fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }
}

impl PackedMetadata {
    /// Narrow a rasterizer's extents and bearing into the atlas's packed
    /// form. `None` when any of them is out of range, which the caller treats
    /// as "too big to cache" rather than an error — an atlas side tops out far
    /// below `u16::MAX` anyway.
    pub(crate) fn new(width: u32, height: u32, left: i32, top: i32) -> Option<Self> {
        Some(Self {
            width: width.try_into().ok()?,
            height: height.try_into().ok()?,
            left: left.try_into().ok()?,
            top: top.try_into().ok()?,
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct AtlasSlot {
    pub(crate) x: u16,
    pub(crate) y: u16,
    pub(crate) width: u16,
    pub(crate) height: u16,
    pub(crate) left: i16,
    pub(crate) top: i16,
    pub(crate) content: ContentType,
    pub(crate) alloc: Option<AllocId>,
    pub(crate) generation: u32,
    pub(crate) last_use: u64,
}

/// One per-content-type backing store. Indexed by `ContentType as usize`.
struct Side {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    size: u32,
    /// Largest edge this side will ever reach — see [`growth_ceiling`].
    ///
    /// Resolved once at construction because its three inputs never
    /// change, and [`RasterAtlas::allocate`] reads it per entry it is
    /// asked to place: recomputing meant a `u64` divide and an `isqrt`
    /// on every glyph and icon that missed the cache.
    ceiling: u32,
    /// The frame a full clock rotation over this side last came up
    /// empty on, or `None` until one has.
    ///
    /// A rotation is O(slab), and [`RasterAtlas::allocate`] calls
    /// [`RasterAtlas::evict_one`] once for every entry it cannot place —
    /// so a frame asking for more than the ceiling holds pays that walk
    /// per starving entry, which is quadratic in the slab. Every one of
    /// those walks is provably wasted: a slot is eligible only while
    /// `last_use < current_frame`, and `last_use` never moves *down*
    /// within a frame (`touch` and `store` both stamp it with
    /// `current_frame`), so once a side has been walked dry nothing can
    /// become evictable until the clock advances. Remembering which
    /// frame that happened on turns the second and every later miss into
    /// one comparison.
    dry_frame: Option<u64>,
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
pub(crate) struct AtlasBindings<'a> {
    pub(crate) mask_view: &'a wgpu::TextureView,
    pub(crate) color_view: &'a wgpu::TextureView,
    pub(crate) atlas_px: [u32; 2],
}

#[derive(Debug)]
pub(crate) struct RasterAtlas<K> {
    sides: [Side; 2],
    labels: AtlasLabels,
    eager_growth_bytes: u64,
    /// Dense slot slab; `cache` maps each key to an index into it.
    /// Encoded-run caches record these indices so their hot-path LRU
    /// refresh is an indexed store instead of a map probe per glyph —
    /// safe because every recorded index carries the slot generation
    /// that `evict_one` advances before making the index reusable.
    pub(crate) slots: Vec<AtlasSlot>,
    /// Key held by each slab entry, parallel to [`Self::slots`]. The
    /// reverse of [`Self::cache`], and the only reason eviction can pick
    /// a victim by slab position at all — it has to drop the outgoing
    /// glyph's map entry, and the map alone only answers key → index.
    ///
    /// A parallel column rather than a field on `AtlasSlot`: the slot is
    /// hot (copied whole by `encode_run`, and read per glyph by
    /// `try_emit_cached`) while the key is touched only when a slot is
    /// stored or evicted, so folding a ~24-byte key in would cost the
    /// hot path density for a cold path's convenience.
    slot_keys: Vec<K>,
    pub(crate) cache: FxHashMap<K, u32>,
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
    /// Incrementing per submit here while the shaped-buffer cache
    /// increments per recorded frame would denominate the two retention
    /// windows `RENDERED_RUN_KEEP_FRAMES` derives in different units, and
    /// they would drift in both directions (a recorded frame that drew no
    /// text ages buffers only; a `PaintOnly` frame ages the atlas only).
    /// Reading one clock is what makes the shared constant mean what it
    /// says.
    pub(crate) current_frame: u64,
    /// Deadlines for non-drawing entries, which `evict_one` cannot
    /// reclaim. Same file-once/re-file-on-fire protocol as the two
    /// caches above this one — see [`ExpiryWheel`] — so `touch` stays a
    /// single indexed store on the hot path and files nothing.
    unallocated_expiry: ExpiryWheel<K>,
    /// Set on grow; the renderer rebuilds its bind group and clears it.
    pub(crate) bind_group_dirty: bool,
    /// Evictions performed, growths performed, and slots *examined*
    /// choosing a victim.
    ///
    /// `allocate` calls `evict_one` in a loop, so the eviction count
    /// alone says nothing about what victim selection cost. `examined`
    /// is the product that actually bills, and the only one that says
    /// whether the clock is behaving — it should track evictions almost
    /// one-for-one. It is also the number that says whether this atlas
    /// reaches the
    /// region where F2's curve matters.
    pub(crate) counters: AtlasCounters,

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

impl<K: Copy + Eq + Hash + Debug> RasterAtlas<K> {
    pub(crate) fn new(device: &wgpu::Device, config: RasterAtlasConfig) -> Self {
        let max = device.limits().max_texture_dimension_2d;

        // Order matches `ContentType as usize`: [Mask, Color].
        let sides = [
            Side::new(
                device,
                ContentType::Mask,
                config.initial_mask_px.min(max),
                growth_ceiling(max, ContentType::Mask, config.max_bytes),
                config.label,
            ),
            Side::new(
                device,
                ContentType::Color,
                config.initial_color_px.min(max),
                growth_ceiling(max, ContentType::Color, config.max_bytes),
                config.label,
            ),
        ];
        let labels = AtlasLabels {
            stem: config.label,
            grow_blit: format!("{} atlas grow blit", config.label),
            batch_upload: format!("{} atlas batch upload", config.label),
            staging: format!("{} atlas staging", config.label),
        };

        Self {
            sides,
            labels,
            eager_growth_bytes: config.eager_growth_bytes,
            slots: Vec::new(),
            slot_keys: Vec::new(),
            cache: FxHashMap::default(),
            free: Vec::new(),
            hand: 0,
            current_frame: 0,
            unallocated_expiry: ExpiryWheel::with_horizon(UNALLOCATED_SWEEP_INTERVAL + 2),
            bind_group_dirty: false,
            counters: AtlasCounters::default(),
            pending_staging: Vec::new(),
            pending_staging_used: 0,
            pending_copies: Vec::new(),
            staging_buf: None,
        }
    }

    pub(crate) fn bindings(&self) -> AtlasBindings<'_> {
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
    pub(crate) fn touch(&mut self, key: &K) -> Option<u32> {
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
    pub(crate) fn insert(
        &mut self,
        device: &wgpu::Device,
        key: K,
        content: ContentType,
        metadata: PackedMetadata,
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

        let slot = AtlasSlot {
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
    fn store(&mut self, key: K, mut slot: AtlasSlot) -> u32 {
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
        assert!(prev.is_none(), "raster inserted over a live cache entry");
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
    pub(crate) fn flush_pending_uploads(&mut self, ctx: &mut GpuCtx<'_>) {
        // Grow blits first: old→new copy must complete before any new
        // glyph writes hit the new texture. wgpu serialises commands
        // within an encoder, so recording in this order is enough.
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
        let bytes = self.pending_staging_used as u64;
        let current_cap = self.staging_buf.as_ref().map_or(0, wgpu::Buffer::size);
        if bytes > current_cap {
            let new_cap = bytes.next_power_of_two().max(current_cap * 2).max(4096);
            self.staging_buf = Some(ctx.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(&self.labels.staging),
                size: new_cap,
                usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let buf = self.staging_buf.as_ref().unwrap();
        ctx.write(buf, 0, &self.pending_staging[..self.pending_staging_used]);

        debug_marker::push_encoder(ctx.encoder, &self.labels.batch_upload);
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
    pub(crate) fn insert_unallocated(
        &mut self,
        key: K,
        content: ContentType,
        metadata: PackedMetadata,
    ) -> u32 {
        debug_assert!(metadata.is_empty());
        self.unallocated_expiry
            .schedule(key, self.current_frame + UNALLOCATED_SWEEP_INTERVAL + 1);
        let slot = AtlasSlot {
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
    pub(crate) fn end_frame(&mut self, frame: u64) {
        debug_assert!(
            frame >= self.current_frame,
            "the shared frame clock ran backwards",
        );
        self.current_frame = frame;
        let cache = &mut self.cache;
        let slots = &self.slots;
        let free = &mut self.free;
        // No stamp to check: this wheel's deadlines only ever move out,
        // so a ticket is never supplanted and every one that fires is
        // the live one.
        self.unallocated_expiry.retire(frame, |key, _| {
            retire_unallocated(cache, slots, free, key, frame)
        });
    }

    /// Allocate a slot in the right packer, evicting then growing as
    /// needed.
    ///
    /// # Why eviction is gated on the entry fitting at all
    ///
    /// Freeing rectangles cannot widen a texture, so for an entry that
    /// does not fit the side's *edge* every victim the loop takes is
    /// spent for nothing — and the loop takes them until the side runs
    /// dry, which empties the whole atlas. Past the ceiling that is the
    /// worst state this type can reach: the entry never fits however
    /// much is freed, the run it belongs to is refused as a template
    /// (see [`EncodedCache::settle`]), so it is asked for again on the
    /// next frame and the side is wiped again, for as long as it stays
    /// on screen. Both gates below are the same predicate — a rect
    /// taller or wider than an edge cannot be placed inside it — applied
    /// once to the ceiling and once to the current size.
    ///
    /// [`EncodedCache::settle`]:
    ///     crate::renderer::backend::text::encode::cache::EncodedCache
    fn allocate(
        &mut self,
        device: &wgpu::Device,
        content: ContentType,
        width: u16,
        height: u16,
    ) -> Option<etagere::Allocation> {
        if !fits_edge(width, height, self.sides[content as usize].ceiling) {
            self.counters.oversized.bump();
            return None;
        }
        let need = size2(width as i32, height as i32);
        loop {
            if let Some(a) = self.sides[content as usize].packer.allocate(need) {
                return Some(a);
            }
            // Two reasons to buy space before paying for a victim. Under
            // the budget it is simply cheaper: one grow is a texture plus
            // a rect-preserving blit, while eviction is an O(live glyphs)
            // scan that this loop repeats for every glyph still waiting.
            // Too wide for the current edge it is the *only* thing that
            // can work, budget or not — and the check above already
            // proved a large enough edge is reachable.
            let must_grow = !fits_edge(width, height, self.sides[content as usize].size);
            let grew = (must_grow || self.eager_growth(content)) && self.grow(device, content);
            // Past the budget — or already at the device maximum, where
            // the grow above returned false — the atlas holds its size
            // by recycling rectangles instead.
            if !grew && !self.evict_one(content) && !self.grow(device, content) {
                return None;
            }
        }
    }

    /// Whether `content`'s side is still small enough to grow in
    /// preference to evicting — see [`RasterAtlasConfig::eager_growth_bytes`].
    fn eager_growth(&self, content: ContentType) -> bool {
        let side = &self.sides[content as usize];
        let bytes =
            u64::from(side.size) * u64::from(side.size) * u64::from(content.bytes_per_pixel());
        bytes < self.eager_growth_bytes
    }

    /// Evict one glyph of `target` content that was not drawn this
    /// frame, chosen by a **clock** — a persistent hand that walks
    /// [`Self::slots`] and takes the first entry it finds eligible,
    /// resuming next call where this one stopped.
    ///
    /// # Why not exact LRU
    ///
    /// Picking the true least-recently-used entry means iterating the
    /// whole `cache` map and indexing `slots` per entry —
    /// O(live glyphs) *per eviction*, and [`Self::allocate`] calls this
    /// in a loop, so one insert can pay the walk several times over.
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
        // Already walked dry on this frame — see [`Side::dry_frame`].
        if self.sides[target as usize].dry_frame == Some(self.current_frame) {
            return false;
        }
        let sweep = clock_victim(&self.slots, self.hand, target, self.current_frame);
        self.hand = sweep.hand;
        self.counters
            .evict_scans
            .edit(|n| *n += sweep.examined as u64);
        let Some(idx) = sweep.victim else {
            self.sides[target as usize].dry_frame = Some(self.current_frame);
            return false;
        };
        self.counters.evictions.bump();
        self.reclaim(idx);
        true
    }

    /// Reclaim allocated slab index `idx`: drop its cache entry, hand its
    /// rectangle back to its side's packer, and advance the generation so
    /// an encoded run still holding the index reads it as stale.
    ///
    /// Takes the side from the slot rather than from the caller, because
    /// [`Self::forget`] reclaims across both at once.
    fn reclaim(&mut self, idx: u32) {
        let key = self.slot_keys[idx as usize];
        let removed = self.cache.remove(&key);
        debug_assert_eq!(
            removed,
            Some(idx),
            "slot_keys disagreed with cache about slab index {idx}",
        );
        let slot = &mut self.slots[idx as usize];
        let content = slot.content;
        let id = slot.alloc.take().unwrap();
        slot.generation = slot
            .generation
            .checked_add(1)
            .expect("glyph slot generation overflowed");
        self.sides[content as usize].packer.deallocate(id);
        self.free.push(idx);
    }

    /// Drop every entry whose key `keep` rejects.
    ///
    /// O(slab), and for the one thing the clock cannot do on its own:
    /// retire a whole *family* of keys at once because what they name is
    /// gone. The icon backend calls it when a set is unloaded — those
    /// rasters can never be asked for again, and left in place they would
    /// hold their rectangles until ordinary pressure happened to sweep
    /// them, which on an atlas sized for the working set may be never.
    ///
    /// Everything else lets the clock reclaim on its own schedule; an
    /// entry that is merely cold is not the same as one that is dead.
    pub(crate) fn forget(&mut self, keep: impl Fn(&K) -> bool) {
        for idx in 0..self.slots.len() as u32 {
            let key = self.slot_keys[idx as usize];
            // A slab index already on the free list still holds its old
            // key, so the cache — not the key column — is what says
            // whether this entry is live.
            if self.cache.get(&key) != Some(&idx) || keep(&key) {
                continue;
            }
            if self.slots[idx as usize].alloc.is_some() {
                self.reclaim(idx);
            } else {
                // A non-drawing entry owns no rectangle, so only its
                // expiry ticket would ever have retired it. Drop it here
                // and let that ticket fire on nothing — the same
                // already-reclaimed case `retire_unallocated` handles,
                // and the reason neither advances the generation.
                self.cache.remove(&key);
                self.free.push(idx);
            }
        }
    }

    /// Double the atlas of `content`. Returns `false` at GPU-max. On
    /// success, stashes the old texture into `Side::pending_grow` so
    /// `flush_pending_uploads` can record the old→new blit on the
    /// shared encoder. etagere preserves rects on `packer.grow`, so
    /// the cache stays valid — no re-rasterization.
    fn grow(&mut self, device: &wgpu::Device, content: ContentType) -> bool {
        // Read before the side is borrowed mutably below.
        let label = format!("{} {} atlas", self.labels.stem, content.side_name());
        let side = &mut self.sides[content as usize];
        if side.size >= side.ceiling {
            return false;
        }
        self.counters.grows.bump();
        let new_size = (side.size * ATLAS_GROWTH_FACTOR).min(side.ceiling);
        let new_texture = make_texture(device, content.format(), new_size, &label);
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
    fn new(
        device: &wgpu::Device,
        content: ContentType,
        size: u32,
        ceiling: u32,
        label: &str,
    ) -> Self {
        let texture = make_texture(
            device,
            content.format(),
            size,
            &format!("{label} {} atlas", content.side_name()),
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            texture,
            view,
            size,
            ceiling,
            dry_frame: None,
            packer: BucketedAtlasAllocator::new(size2(size as i32, size as i32)),
            pending_grow: None,
        }
    }
}

/// Largest side length a `content` atlas will grow to: whichever of the
/// device maximum and the instance's byte budget binds first.
///
/// A free function taking the device limit rather than a method, so the
/// arithmetic is testable without a `wgpu::Device`.
fn growth_ceiling(max_texture_dimension_2d: u32, content: ContentType, max_bytes: u64) -> u32 {
    let by_bytes = (max_bytes / u64::from(content.bytes_per_pixel())).isqrt() as u32;
    max_texture_dimension_2d.min(by_bytes)
}

/// Whether a `width × height` rect can be placed inside a square side of
/// `edge` texels — the one question [`RasterAtlas::allocate`] has to
/// answer before it is allowed to evict anything.
///
/// Exact rather than conservative, and that is what makes it usable as a
/// gate: the packer is configured with one column and unit alignment, so
/// its own reject is `w > edge || h > edge` and this agrees with it
/// texel for texel. A stricter test here would refuse entries the packer
/// would have taken.
const fn fits_edge(width: u16, height: u16, edge: u32) -> bool {
    width as u32 <= edge && height as u32 <= edge
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
fn retire_unallocated<K: Copy + Eq + Hash + Debug>(
    cache: &mut FxHashMap<K, u32>,
    slots: &[AtlasSlot],
    free: &mut Vec<u32>,
    key: K,
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

/// One turn of [`RasterAtlas::evict_one`]'s clock: where the hand ended
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
    /// Slots examined. What [`AtlasCounters::evict_scans`] bills, and the
    /// number that says whether the clock is behaving — a healthy thrash
    /// state stops after one or two.
    examined: u32,
}

/// Advance `hand` over `slots` until it meets an entry eligible for
/// eviction: allocated, of `target` content, and not drawn on
/// `current_frame`. Gives up after one full rotation.
fn clock_victim(
    slots: &[AtlasSlot],
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

/// What a raster atlas paid to keep itself packed. Zero-sized outside
/// benchmark and test builds.
///
/// `BenchOnly` rather than `TestOnly`: the question these answer — does a
/// real workload drive the atlas into the regime where eviction bills at
/// all — is only reachable from `text_atlas`, which the `internals`
/// feature gates.
#[derive(Debug, Default)]
pub(crate) struct AtlasCounters {
    pub(crate) evictions: BenchOnly<u32>,
    pub(crate) grows: BenchOnly<u32>,
    /// Slots the clock hand walked past, summed over every call. Divided
    /// by [`Self::evictions`] this is the hand's average stride, which is
    /// the whole health check on the policy: a healthy thrash state
    /// stops on the first or second slot, and a number that climbs
    /// toward the slab length means the skip conditions are rejecting
    /// nearly everything.
    pub(crate) evict_scans: BenchOnly<u64>,
    /// Entries refused because they exceed the side's growth ceiling.
    ///
    /// Distinct from a plain full atlas, and the distinction is the
    /// point: a full atlas recovers on its own once the frame's pressure
    /// clears, while this one never does — the same entry is refused on
    /// every frame it is drawn. A non-zero reading means content is
    /// asking for rasters the configured budget cannot hold, so the
    /// answer is a bigger `max_bytes` or a size ladder that clamps
    /// earlier, not anything the atlas can do at runtime.
    pub(crate) oversized: BenchOnly<u32>,
}

/// Reads are gated with their sole consumer: the `text_atlas`
/// benchmark, which `bench` gates too. A plain `cargo test` build
/// has no caller.
#[cfg(feature = "bench")]
impl AtlasCounters {
    pub(crate) fn counts(&self) -> AtlasCounts {
        AtlasCounts {
            evictions: self.evictions.count(),
            grows: self.grows.count(),
            evict_scans: *self.evict_scans.get(),
            oversized: self.oversized.count(),
        }
    }
}

/// One reading of an [`AtlasCounters`].
#[cfg(feature = "bench")]
#[derive(Clone, Copy, Debug)]
pub(crate) struct AtlasCounts {
    pub(crate) evictions: u32,
    pub(crate) grows: u32,
    pub(crate) evict_scans: u64,
    pub(crate) oversized: u32,
}

#[cfg(test)]
mod tests;
