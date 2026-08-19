//! Rasterized-quad atlas: one struct for both mask + colour content, keyed by
//! whatever its tenant rasterizes from.
//!
//! Two instances exist. The text backend keys one on a glyph's cosmic
//! `CacheKey`; the icon backend keys another on
//! [`IconRasterKey`](crate::icons::icon_raster_key::IconRasterKey). They share
//! every policy below — bucketed packing, clock-sweep eviction, grow-with-blit,
//! and batched staging uploads — and share nothing else: separate textures,
//! separate bind groups, separate eviction budgets.
//!
//! The packing and growth of one side live on [`Side`]; victim selection
//! lives on [`ClockSweep`]. What stays here is the slab, the key index,
//! and the upload queue that turns both into GPU traffic.

pub(crate) mod atlas_slot;
mod clock_sweep;
pub(crate) mod content_type;
pub(crate) mod counters;
pub(crate) mod packed_metadata;
pub(crate) mod quad;
mod side;

use crate::common::expiry_wheel::ExpiryWheel;
use crate::primitives::span::Span;
use crate::renderer::backend::debug_marker;
use crate::renderer::backend::dynamic_buffer::DynamicBuffer;
use crate::renderer::backend::gpu_ctx::GpuCtx;
use crate::renderer::backend::pipeline_utils::StencilVariant;
use crate::renderer::backend::raster_atlas::atlas_slot::AtlasSlot;
use crate::renderer::backend::raster_atlas::clock_sweep::ClockSweep;
use crate::renderer::backend::raster_atlas::content_type::ContentType;
use crate::renderer::backend::raster_atlas::counters::AtlasCounters;
use crate::renderer::backend::raster_atlas::packed_metadata::PackedMetadata;
use crate::renderer::backend::raster_atlas::quad::{PARAMS_OFFSET, RasterQuad};
use crate::renderer::backend::raster_atlas::side::Side;
use crate::renderer::backend::viewport::ViewportPush;
use etagere::size2;
use rustc_hash::FxHashMap;
use std::fmt::Debug;
use std::hash::Hash;
use wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;

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
    /// victims — victim selection is O(live entries) and `allocate` asks for
    /// one in a loop. Sizing to the working set first turns that into one
    /// texture allocation plus a preserved-rect blit: one growth, zero
    /// evictions, and 61 µs a frame rather than 609 µs.
    pub(crate) eager_growth_bytes: u64,
}

/// GPU debug labels, built once at construction. Held as owned strings rather
/// than formatted per flush because two of them are pushed on the encoder
/// every frame that uploads, and a per-frame `format!` is exactly the
/// allocation this crate does not do.
#[derive(Debug)]
struct AtlasLabels {
    /// The configured stem, kept so a grow can re-derive a texture name.
    stem: &'static str,
    grow_blit: String,
    batch_upload: String,
    staging: String,
    bind_group: String,
}

/// Frames a non-drawing entry (`alloc: None`) survives unused.
///
/// Victim selection skips them — there is no rectangle to deallocate — so
/// every whitespace or rejected glyph at every scale rung would otherwise
/// accumulate forever, lengthening the slab the clock hand walks without
/// ever offering it a victim.
///
/// A per-entry deadline on [`RasterAtlas::unallocated_expiry`], not a
/// cadence. A periodic `cache.retain` over the whole glyph map would be
/// the shape this crate avoids everywhere else — one frame in N paying
/// for all of them — and it would have to be spelled as a threshold
/// rather than `frame % INTERVAL == 0`, because the shared clock can
/// advance by more than one and a modulo gate steps over its own
/// trigger. A wheel has neither problem, and retires each entry on its
/// own last use instead of rounding every entry to a shared tick.
///
/// **Denominated in the instance's own clock**, which is not the same
/// clock for both: the text atlas mirrors the shaper's, which advances
/// once per recorded frame per window, while the icon atlas counts its
/// own submits. Both land near 2 s of a 60 Hz session, which is what the
/// number has to be — far outside any flicker a visibility toggle or a
/// hover paint produces, and short enough that the deadline ring stays
/// 128 buckets rather than the 1024 an 8 s window cost. Getting it wrong
/// costs one swash call that yields no pixels.
///
/// Its own constant rather than [`crate::text::RENDERED_RUN_KEEP_FRAMES`],
/// which it happens to equal. That one answers how long a *shaped buffer*
/// survives untouched, and one of this type's two instances holds no text
/// at all — deriving from it would let a text-side tuning silently move
/// the icon atlas.
const UNALLOCATED_KEEP_FRAMES: u64 = 120;

#[derive(Debug)]
pub(crate) struct RasterAtlas<K> {
    sides: [Side; 2],
    labels: AtlasLabels,
    eager_growth_bytes: u64,
    /// Dense slot slab; `cache` maps each key to an index into it.
    /// Encoded-run caches record these indices so their hot-path LRU
    /// refresh is an indexed store instead of a map probe per glyph —
    /// safe because every recorded index carries the slot generation
    /// that eviction advances before making the index reusable.
    pub(crate) slots: Vec<AtlasSlot>,
    /// Key held by each slab entry, parallel to [`Self::slots`]. The
    /// reverse of [`Self::cache`], and the only reason eviction can pick
    /// a victim by slab position at all — it has to drop the outgoing
    /// glyph's map entry, and the map alone only answers key → index.
    ///
    /// A parallel column rather than a field on [`AtlasSlot`]: the slot
    /// is hot (copied whole by `encode_run`, and read per glyph by
    /// `try_emit_cached`) while the key is touched only when a slot is
    /// stored or evicted, so folding a ~24-byte key in would cost the
    /// hot path density for a cold path's convenience.
    ///
    /// **Only meaningful for an index [`Self::cache`] still maps.** A
    /// freed index keeps its old key here until something overwrites it,
    /// so the map is the single authority on which indices are live.
    slot_keys: Vec<K>,
    pub(crate) cache: FxHashMap<K, u32>,
    /// Slab indices freed by eviction, expiry, or [`Self::forget`],
    /// reused by the next `store`.
    free: Vec<u32>,
    /// Rotating eviction cursor over [`Self::slots`] — see
    /// [`Self::evict_one`]. Persists across calls, which is the whole
    /// point: it is what turns the victim search from a scan of the
    /// whole slab per eviction into a walk that resumes where the last
    /// one stopped.
    hand: u32,
    /// Latest value of this atlas's frame clock, mirrored here by
    /// [`Self::end_frame`] — the shaper's for the text instance, the
    /// icon backend's submit count for the other.
    ///
    /// Incrementing per submit on the text side while the shaped-buffer
    /// cache increments per recorded frame would denominate the two
    /// retention windows `RENDERED_RUN_KEEP_FRAMES` derives in different
    /// units, and they would drift in both directions (a recorded frame
    /// that drew no text ages buffers only; a `PaintOnly` frame ages the
    /// atlas only). Reading one clock is what makes the shared constant
    /// mean what it says.
    pub(crate) current_frame: u64,
    /// Deadlines for non-drawing entries, which the clock cannot
    /// reclaim. Same file-once/re-file-on-fire protocol as the two
    /// caches above this one — see [`ExpiryWheel`] — so `touch` stays a
    /// single indexed store on the hot path and files nothing.
    unallocated_expiry: ExpiryWheel<K>,
    /// Group-0 layout, sampler, bind group and the `[color, mask]`
    /// side extents the shader reads as params.
    ///
    /// Owned here rather than by each tenant because they are a function
    /// of the atlas's own textures, and those move under a grow. A
    /// dirty flag would put the rebuild on every tenant — a dozen
    /// identical lines apiece, and a protocol whose failure mode is
    /// sampling a destroyed texture view. Rebuilding inside
    /// [`Self::grow`] means there is no
    /// window in which the two disagree and nothing for a third tenant
    /// to forget.
    bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    bind_group: wgpu::BindGroup,
    atlas_px: [u32; 2],
    /// Evictions, growths, refusals, and slots *examined* choosing a
    /// victim — see [`AtlasCounters`].
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

/// The group-0 bind group over `sides`, paired with the `[color, mask]`
/// extents the shader reads as params.
#[derive(Debug)]
struct BoundSides {
    bind_group: wgpu::BindGroup,
    atlas_px: [u32; 2],
}

/// Bind `sides` for sampling. One definition rather than one per call
/// site: [`RasterAtlas::new`] and `RasterAtlas::grow` both need the
/// pair, and building it two ways is how the extents and the views they
/// describe drift apart.
fn bind_sides(
    device: &wgpu::Device,
    sides: &[Side; 2],
    bgl: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    label: &str,
) -> BoundSides {
    let mask = &sides[ContentType::Mask as usize];
    let color = &sides[ContentType::Color as usize];
    BoundSides {
        bind_group: quad::bind_group(device, bgl, &mask.view, &color.view, sampler, label),
        atlas_px: [color.size, mask.size],
    }
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
                Side::growth_ceiling(max, ContentType::Mask, config.max_bytes),
                config.label,
            ),
            Side::new(
                device,
                ContentType::Color,
                config.initial_color_px.min(max),
                Side::growth_ceiling(max, ContentType::Color, config.max_bytes),
                config.label,
            ),
        ];
        let labels = AtlasLabels {
            stem: config.label,
            grow_blit: format!("{} atlas grow blit", config.label),
            batch_upload: format!("{} atlas batch upload", config.label),
            staging: format!("{} atlas staging", config.label),
            bind_group: format!("{} atlas bg", config.label),
        };

        // Built here rather than by each tenant: the pair is a function
        // of `sides`, so the atlas is the only thing that can keep them
        // in step across a grow.
        let bgl = quad::bind_group_layout(device, &format!("{} atlas layout", config.label));
        let sampler = quad::sampler(device, &format!("{} sampler", config.label));
        let bound = bind_sides(device, &sides, &bgl, &sampler, &labels.bind_group);

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
            unallocated_expiry: ExpiryWheel::with_horizon(UNALLOCATED_KEEP_FRAMES + 2),
            bgl,
            sampler,
            bind_group: bound.bind_group,
            atlas_px: bound.atlas_px,
            counters: AtlasCounters::default(),
            pending_staging: Vec::new(),
            pending_staging_used: 0,
            pending_copies: Vec::new(),
            staging_buf: None,
        }
    }

    /// Bind this atlas and draw `span` of `vbuf`'s quad instances.
    ///
    /// The whole per-batch draw sequence, once. Text and icon are two
    /// tenants of one atlas with one shader and one bind-group shape
    /// (see [`quad`]), so their draws were byte-identical apart from the
    /// atlas path — including the comment explaining why the viewport is
    /// pushed here.
    ///
    /// Both halves of the shared immediate region get written because
    /// either tenant can be the first pipeline bound in a pass, so no
    /// earlier step is guaranteed to have pushed the viewport.
    pub(super) fn draw_span<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        pipelines: &'a StencilVariant,
        use_stencil: bool,
        viewport: &ViewportPush,
        vbuf: &'a DynamicBuffer<RasterQuad>,
        span: Span,
    ) {
        if span.len == 0 {
            return;
        }
        pass.set_pipeline(pipelines.select(use_stencil));
        pass.set_bind_group(0, &self.bind_group, &[]);
        viewport.push_into(pass);
        pass.set_immediates(PARAMS_OFFSET, bytemuck::bytes_of(&self.atlas_px));
        pass.set_vertex_buffer(0, vbuf.buffer.slice(..));
        pass.draw(0..4, span.start..span.start + span.len);
    }

    /// The layout the bind group was built against, for pipeline
    /// creation. Format-independent, so it survives a swapchain
    /// reformat.
    pub(crate) fn bind_group_layout(&self) -> &wgpu::BindGroupLayout {
        &self.bgl
    }

    /// `[color, mask]` side extents, as the shader reads them.
    pub(crate) fn atlas_px(&self) -> [u32; 2] {
        self.atlas_px
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
        debug_assert!(prev.is_none(), "raster inserted over a live cache entry");
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
        debug_assert!(region_start.is_multiple_of(COPY_BYTES_PER_ROW_ALIGNMENT as usize));
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
            .schedule(key, self.current_frame + UNALLOCATED_KEEP_FRAMES + 1);
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

    /// Frame teardown: take this atlas's `frame` clock and retire the
    /// non-drawing entries whose deadline came due on it.
    pub(crate) fn end_frame(&mut self, frame: u64) {
        debug_assert!(
            frame >= self.current_frame,
            "the atlas frame clock ran backwards",
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
        if !self.sides[content as usize].fits_ceiling(width, height) {
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
            let must_grow = !self.sides[content as usize].fits_now(width, height);
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
    /// resuming next call where this one stopped. See [`ClockSweep`].
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
        let sweep = ClockSweep::over(&self.slots, self.hand, target, self.current_frame);
        self.hand = sweep.hand;
        self.counters
            .evict_scans
            .edit(|n| *n += sweep.examined as u64);
        let Some(idx) = sweep.victim else {
            self.sides[target as usize].dry_frame = Some(self.current_frame);
            return false;
        };
        self.counters.evictions.bump();
        let key = self.slot_keys[idx as usize];
        let removed = self.cache.remove(&key);
        debug_assert_eq!(
            removed,
            Some(idx),
            "slot_keys disagreed with cache about slab index {idx}",
        );
        retire_slot(&mut self.slots, &mut self.sides, &mut self.free, idx);
        true
    }

    /// Drop every entry whose key `keep` rejects.
    ///
    /// For the one thing the clock cannot do on its own: retire a whole
    /// *family* of keys at once because what they name is gone. The icon
    /// backend calls it when sets are unloaded — those rasters can never
    /// be asked for again, and left in place they would hold their
    /// rectangles until ordinary pressure happened to sweep them, which
    /// on an atlas sized for the working set may be never.
    ///
    /// Walks [`Self::cache`] rather than the slab, so it visits live
    /// entries only and hashes nothing: a freed slab index still holds
    /// its old key, and asking the map about each one in turn made this
    /// a hash probe per resident raster. `keep` therefore has to answer
    /// for a *set* of doomed keys in one pass — the caller batches, so
    /// unloading N sets is one walk rather than N.
    ///
    /// Everything else lets the clock reclaim on its own schedule; an
    /// entry that is merely cold is not the same as one that is dead.
    pub(crate) fn forget(&mut self, keep: impl Fn(&K) -> bool) {
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
            retire_slot(slots, sides, free, idx);
            false
        });
    }

    /// Double the side of `content`, reporting whether it moved.
    fn grow(&mut self, device: &wgpu::Device, content: ContentType) -> bool {
        let stem = self.labels.stem;
        if !self.sides[content as usize].grow(device, content, stem) {
            return false;
        }
        self.counters.grows.bump();
        // In the same step that invalidated them, so no caller can
        // observe the stale pair.
        let bound = bind_sides(
            device,
            &self.sides,
            &self.bgl,
            &self.sampler,
            &self.labels.bind_group,
        );
        self.bind_group = bound.bind_group;
        self.atlas_px = bound.atlas_px;
        true
    }
}

/// Hand slab index `idx`'s resources back: its rectangle to the side's
/// packer, and the index itself to the free list.
///
/// Advancing the generation is what makes the index safe to hand on: an
/// encoded run still holding it reads the slot as stale rather than
/// drawing whatever took its place. A non-drawing entry owns no rectangle
/// and no run ever records its index, so the bump is skipped there — the
/// same case [`retire_unallocated`] handles.
///
/// A free function borrowing the three columns rather than `&mut self`,
/// for the reason [`retire_unallocated`] is one too: both callers hold
/// the key map borrowed across the call.
fn retire_slot(slots: &mut [AtlasSlot], sides: &mut [Side], free: &mut Vec<u32>, idx: u32) {
    let slot = &mut slots[idx as usize];
    if let Some(id) = slot.alloc.take() {
        slot.generation = slot
            .generation
            .checked_add(1)
            .expect("glyph slot generation overflowed");
        sides[slot.content as usize].packer.deallocate(id);
    }
    free.push(idx);
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
    // eviction.
    let &idx = cache.get(&key)?;
    let slot = &slots[idx as usize];
    // Allocated entries are the clock's to reclaim, and it advances
    // their generation when it does. Defensive rather than reachable —
    // every path that allocates over a slab index removes the old key
    // from `cache` first, so the lookup above would have missed.
    if slot.alloc.is_some() {
        return None;
    }
    // `touch` refreshes `last_use` without filing anything, so the real
    // deadline is re-read here.
    let dies_at = slot.last_use + UNALLOCATED_KEEP_FRAMES + 1;
    if dies_at > frame {
        return Some(dies_at);
    }
    cache.remove(&key);
    free.push(idx);
    None
}

#[cfg(test)]
mod tests;
