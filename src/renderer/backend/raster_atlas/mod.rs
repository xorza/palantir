//! Rasterized-quad atlas: one struct for both mask + colour content, keyed by
//! whatever its tenant rasterizes from.
//!
//! Two instances exist. The text backend keys one on a glyph's cosmic
//! `CacheKey`; the icon backend keys another on
//! [`IconRasterKey`](crate::icons::icon_raster_key::IconRasterKey). They share
//! every policy below — bucketed packing, clock-sweep eviction, grow-with-blit,
//! and batched staging uploads — plus the layout and sampler their bind
//! groups are built against, which belong to the shared
//! [`RasterProgram`]. What stays separate is the **space**: its own
//! textures, its own bind group, its own eviction budget, so a
//! colour-icon-heavy frame cannot take rectangles from the glyphs of the
//! label beside it.
//!
//! The packing and growth of one side live on [`Side`]; victim selection
//! lives on [`ClockSweep`]. What stays here is the slab, the key index,
//! and the upload queue that turns both into GPU traffic.

pub(crate) mod atlas_slot;
mod bound_sides;
mod clock_sweep;
pub(crate) mod counters;
mod free_slots;
pub(crate) mod packed_metadata;
pub(crate) mod raster_quad;
mod side;

use crate::common::expiry_wheel::ExpiryWheel;
use crate::primitives::content_type::ContentType;
use crate::primitives::span::Span;
use crate::renderer::backend::debug_marker;
use crate::renderer::backend::dynamic_buffer::DynamicBuffer;
use crate::renderer::backend::gpu_ctx::GpuCtx;
use crate::renderer::backend::raster_atlas::atlas_slot::{AtlasSlot, SlotPlacement};
use crate::renderer::backend::raster_atlas::bound_sides::BoundSides;
use crate::renderer::backend::raster_atlas::clock_sweep::ClockSweep;
use crate::renderer::backend::raster_atlas::counters::AtlasCounters;
use crate::renderer::backend::raster_atlas::free_slots::FreeSlots;
use crate::renderer::backend::raster_atlas::packed_metadata::PackedMetadata;
use crate::renderer::backend::raster_atlas::raster_quad::RasterQuad;
use crate::renderer::backend::raster_atlas::side::Side;
use crate::renderer::backend::raster_program::RasterProgram;
use crate::renderer::backend::stencil_variant::StencilVariant;
use crate::renderer::backend::viewport::ViewportPush;
use etagere::size2;
use glam::{U16Vec2, UVec2};
use rustc_hash::FxHashMap;
use std::collections::hash_map::Entry;
use std::fmt::Debug;
use std::hash::Hash;
use std::ops::Range;
use wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;

/// How one [`RasterAtlas`] differs from the other: what it calls itself in GPU
/// debug labels, and how big each side starts.
///
/// Initial sizes are a tenant's judgement about its own content, not a shared
/// default — a session full of text and no emoji wants the opposite split from
/// one full of colour icons.
#[derive(Clone, Copy, Debug)]
pub(super) struct RasterAtlasConfig {
    /// Label stem, e.g. `"palantir.text"`. Every texture, marker, and buffer
    /// this atlas creates is named from it.
    pub(super) label: &'static str,
    pub(super) initial_mask_px: u32,
    pub(super) initial_color_px: u32,
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
    pub(super) max_bytes: u64,
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
    pub(super) eager_growth_bytes: u64,
}

/// GPU debug labels, built once at construction. Held as owned strings rather
/// than formatted per flush because two of them are pushed on the encoder
/// every frame that uploads, and a per-frame `format!` is exactly the
/// allocation this crate does not do.
#[derive(Debug)]
struct AtlasLabels {
    grow_blit: String,
    batch_upload: String,
    staging: String,
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
/// **Denominated in the shared text clock**, which both instances age
/// on — the icon backend is handed the reading `TextBackend::end_frame`
/// returns. It lands near 2 s of a 60 Hz session, which is what the
/// number has to be — far outside any flicker a visibility toggle or a
/// hover paint produces, and short enough that the deadline ring stays
/// 128 buckets rather than the 1024 an 8 s window cost. Getting it wrong
/// costs one rasterizer call that yields no pixels.
///
/// Its own constant rather than [`crate::text::RENDERED_RUN_KEEP_FRAMES`],
/// which it happens to equal. That one answers how long a *shaped buffer*
/// survives untouched, and one of this type's two instances holds no text
/// at all — deriving from it would let a text-side tuning silently move
/// the icon atlas.
const UNALLOCATED_KEEP_FRAMES: u64 = 120;

#[derive(Debug)]
pub(super) struct RasterAtlas<K> {
    sides: [Side; 2],
    labels: AtlasLabels,
    eager_growth_bytes: u64,
    /// Dense slot slab; `cache` maps each key to an index into it.
    /// Encoded-run caches record these indices so their hot-path LRU
    /// refresh is an indexed store instead of a map probe per glyph —
    /// safe because every recorded index carries the slot generation
    /// that eviction advances before making the index reusable.
    pub(super) slots: Vec<AtlasSlot>,
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
    pub(super) cache: FxHashMap<K, u32>,
    /// Released by eviction, by expiry, and by [`Self::forget`].
    free: FreeSlots,
    /// Rotating eviction cursor over [`Self::slots`] — see
    /// [`Self::evict_one`]. Persists across calls, which is the whole
    /// point: it is what turns the victim search from a scan of the
    /// whole slab per eviction into a walk that resumes where the last
    /// one stopped.
    hand: u32,
    /// Latest reading of the shared text clock, mirrored here by
    /// [`Self::advance_to`]. Both instances take it — the icon backend is
    /// handed what `TextBackend::end_frame` returns — so a keep count
    /// means the same span in either.
    ///
    /// Counting submits here instead, while the shaped-buffer cache
    /// counts recorded frames, would denominate the two retention windows
    /// `RENDERED_RUN_KEEP_FRAMES` orders in different units, and they
    /// would drift in both directions: a recorded frame that drew no text
    /// ages buffers only, and a `PaintOnly` frame the atlas only. Reading
    /// one clock is what makes the shared constant mean what it says.
    pub(super) current_frame: u64,
    /// Deadlines for non-drawing entries, which the clock cannot
    /// reclaim. Same file-once/re-file-on-fire protocol as the two
    /// caches above this one — see [`ExpiryWheel`] — so `touch` stays a
    /// single indexed store on the hot path and files nothing.
    unallocated_expiry: ExpiryWheel<K>,
    /// Everything group 0 needs to sample [`Self::sides`] — see
    /// [`BoundSides`].
    ///
    /// Owned here rather than by each tenant because it is a function of
    /// the atlas's own textures, and those move under a grow. A dirty flag
    /// would put the rebuild on every tenant — a dozen identical lines
    /// apiece, and a protocol whose failure mode is sampling a destroyed
    /// texture view. Rebinding inside [`Self::grow`] means there is no
    /// window in which the binding and the textures disagree, and nothing
    /// for a third tenant to forget.
    bound: BoundSides,
    /// Evictions, growths, refusals, and slots *examined* choosing a
    /// victim — see [`AtlasCounters`].
    pub(super) counters: AtlasCounters,

    /// Raster pixels queued by `insert`, one raster after another with
    /// their rows exactly as the rasterizer packed them.
    ///
    /// **No row padding here.** `copy_buffer_to_texture` wants each row
    /// at `wgpu::COPY_BYTES_PER_ROW_ALIGNMENT = 256`, which for a 12 px
    /// mask glyph is twenty times its pixels. Padding at this end meant
    /// retaining a buffer sized for that and staging every pad byte
    /// twice, so the padding happens once, into the belt's own mapped
    /// staging — see [`Self::flush_pending_uploads`].
    pending_pixels: Vec<u8>,
    pending_copies: Vec<PendingCopy>,
    /// Retained staging buffer; grown on demand, reused across frames.
    staging_buf: Option<wgpu::Buffer>,
}

/// One raster waiting to reach its atlas side.
#[derive(Clone, Copy, Debug)]
struct PendingCopy {
    content: ContentType,
    origin: UVec2,
    size: UVec2,
    /// Where this raster's rows start in
    /// [`RasterAtlas::pending_pixels`]. Its length follows from `size`
    /// and `content`, so it is derived rather than stored.
    pixels_start: usize,
}

impl PendingCopy {
    /// Pitch of one unpadded row — what the rasterizer wrote.
    fn bytes_per_row(self) -> u32 {
        self.size.x * self.content.bytes_per_pixel()
    }

    /// Pitch `copy_buffer_to_texture` reads this raster's rows at. Every
    /// raster's region is a whole number of these, so each one's buffer
    /// offset is 256-aligned too — the second alignment that copy wants.
    fn padded_bytes_per_row(self) -> u32 {
        self.bytes_per_row()
            .next_multiple_of(COPY_BYTES_PER_ROW_ALIGNMENT)
    }

    /// This raster's bytes in [`RasterAtlas::pending_pixels`].
    fn pixels(self) -> Range<usize> {
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
            grow_blit: format!("{} atlas grow blit", config.label),
            batch_upload: format!("{} atlas batch upload", config.label),
            staging: format!("{} atlas staging", config.label),
        };

        // Built here rather than by each tenant: the bind group is a
        // function of `sides`, so the atlas is the only thing that can keep
        // it in step across a grow.
        let bound = BoundSides::new(device, program, &sides, config.label);

        Self {
            sides,
            labels,
            eager_growth_bytes: config.eager_growth_bytes,
            slots: Vec::new(),
            slot_keys: Vec::new(),
            cache: FxHashMap::default(),
            free: FreeSlots::default(),
            hand: 0,
            current_frame: 0,
            unallocated_expiry: ExpiryWheel::with_keep(UNALLOCATED_KEEP_FRAMES),
            bound,
            counters: AtlasCounters::default(),
            pending_pixels: Vec::new(),
            pending_copies: Vec::new(),
            staging_buf: None,
        }
    }

    /// Bind this atlas and draw `span` of `vbuf`'s quad instances.
    ///
    /// The whole per-batch draw sequence, once. Text and icon are two
    /// tenants of one atlas with one shader and one bind-group shape
    /// (see [`RasterQuad`]), so their draws were byte-identical apart from the
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
        pass.set_bind_group(0, self.bound.bind_group(), &[]);
        viewport.push_into(pass);
        pass.set_immediates(
            RasterQuad::PARAMS_OFFSET,
            bytemuck::bytes_of(&self.bound.atlas_px()),
        );
        pass.set_vertex_buffer(0, vbuf.buffer.slice(..));
        pass.draw(0..4, span.start..span.start + span.len);
    }

    /// `[color, mask]` side extents, as the shader reads them.
    pub(super) fn atlas_px(&self) -> [u32; 2] {
        self.bound.atlas_px()
    }

    /// Cache-hit fast path: bump the slot's LRU stamp and return its
    /// slab index (read the slot itself via `self.slots[idx]`).
    pub(super) fn touch(&mut self, key: &K) -> Option<u32> {
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
        key: K,
        content: ContentType,
        metadata: PackedMetadata,
        pixels: &[u8],
    ) -> Option<u32> {
        let alloc = self.allocate(device, content, metadata.size)?;
        let origin = UVec2::new(alloc.rectangle.min.x as u32, alloc.rectangle.min.y as u32);
        self.enqueue_upload(content, origin, metadata.size.as_uvec2(), pixels);

        let slot = AtlasSlot {
            placement: Some(SlotPlacement {
                origin: origin.as_u16vec2(),
                size: metadata.size,
                bearing: metadata.bearing,
                content,
                alloc: alloc.id,
            }),
            generation: 0,
            last_use: self.current_frame,
            free: false,
        };
        Some(self.store(key, slot))
    }

    /// Park `slot` in the slab (reusing a freed index when available)
    /// and map `key` to it.
    fn store(&mut self, key: K, mut slot: AtlasSlot) -> u32 {
        let idx = match self.free.claim() {
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

    /// Queue one raster's pixels for the frame's drain.
    ///
    /// `pixels` is already exactly `size.x * size.y` texels with no row
    /// slack, so the whole raster is one append.
    fn enqueue_upload(&mut self, content: ContentType, origin: UVec2, size: UVec2, pixels: &[u8]) {
        let copy = PendingCopy {
            content,
            origin,
            size,
            pixels_start: self.pending_pixels.len(),
        };
        debug_assert_eq!(
            pixels.len(),
            copy.pixels().len(),
            "a raster's bytes must be its rows with no slack",
        );
        self.pending_pixels.extend_from_slice(pixels);
        self.pending_copies.push(copy);
    }

    /// Drain this frame's queued rasters onto the GPU, after any pending
    /// grow blit. Called once per frame, before any pass draws — so the
    /// pixels land in the same submit as the draws that read them. The
    /// renderer owns the submit; this method adds no extra one.
    ///
    /// **The row padding is applied here, into the belt's own mapped
    /// staging.** `copy_buffer_to_texture` wants each row at
    /// `COPY_BYTES_PER_ROW_ALIGNMENT`, which for a 12 px mask glyph is
    /// twenty times its pixels — so building the padded image on this
    /// side first and then handing it to the belt staged those bytes
    /// twice and kept a retained buffer sized for the padded peak.
    /// Composing straight into the mapped view pays for them once, and
    /// what this type retains is the pixels alone.
    ///
    /// **Not `queue.write_texture`, which needs no padding at all.**
    /// Queue writes run before every command buffer in the submit, so
    /// they would land under the grow blit recorded just above — and
    /// measured on the allocation suite's scale ramp, wgpu allocates per
    /// call: 400 blocks a frame became 863.
    pub(super) fn flush_pending_uploads(&mut self, ctx: &mut GpuCtx<'_>) {
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
        let bytes = self.reserve_staging(ctx.device);
        let buf = self.staging_buf.as_ref().unwrap();
        // Every queued raster covers at least one row, and a padded row
        // is 256 bytes, so the view is never the empty one `write_view`
        // declines — and an early return here would strand the queue.
        let mut view = ctx
            .write_view(buf, 0, bytes)
            .expect("a queued raster stages at least one padded row");

        // One walk, so the staging offset a row is written at and the
        // offset its copy reads from are the same number rather than two
        // running totals that have to agree. The pad bytes are left as
        // the belt's chunk last held them: no copy reads past a row's
        // pixels, so writing them would be work for nothing. Recording a
        // copy before its own rows are staged costs nothing either — the
        // command reads the buffer at submit, not here.
        debug_marker::push_encoder(ctx.encoder, &self.labels.batch_upload);
        let mut at = 0usize;
        for c in &self.pending_copies {
            let row_bytes = c.bytes_per_row() as usize;
            let padded = c.padded_bytes_per_row();
            ctx.encoder.copy_buffer_to_texture(
                wgpu::TexelCopyBufferInfo {
                    buffer: buf,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: at as u64,
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
        debug_marker::pop_encoder(ctx.encoder);

        self.pending_pixels.clear();
        self.pending_copies.clear();
    }

    /// Grow the retained staging buffer to hold this frame's padded
    /// rows, and answer how many bytes that is.
    fn reserve_staging(&mut self, device: &wgpu::Device) -> u64 {
        let bytes: u64 = self
            .pending_copies
            .iter()
            .map(|c| u64::from(c.padded_bytes_per_row()) * u64::from(c.size.y))
            .sum();
        let current_cap = self.staging_buf.as_ref().map_or(0, wgpu::Buffer::size);
        if bytes > current_cap {
            let new_cap = bytes.next_power_of_two().max(current_cap * 2).max(4096);
            self.staging_buf = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(&self.labels.staging),
                size: new_cap,
                usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        bytes
    }

    /// Cache a non-drawing glyph: it takes a slab index like any other
    /// entry, but no packer rectangle and no upload. Subsequent lookups
    /// still hit the cache and skip swash.
    pub(super) fn insert_unallocated(&mut self, key: K) -> u32 {
        self.unallocated_expiry
            .schedule(key, unallocated_dies_at(self.current_frame));
        let slot = AtlasSlot {
            placement: None,
            generation: 0,
            last_use: self.current_frame,
            free: false,
        };
        self.store(key, slot)
    }

    /// Advance this atlas's view of the shared clock to `frame` and
    /// retire the non-drawing entries whose deadline came due on it.
    ///
    /// Not `end_frame`, which the crate's other caches spell without an
    /// argument: this one owns no frame boundary. It ages to a reading
    /// someone else advanced.
    pub(super) fn advance_to(&mut self, frame: u64) {
        debug_assert!(
            frame >= self.current_frame,
            "the atlas frame clock ran backwards",
        );
        self.current_frame = frame;
        let cache = &mut self.cache;
        let slots = &mut self.slots;
        let sides = &mut self.sides;
        let free = &mut self.free;
        // No stamp to check: this wheel's deadlines only ever move out,
        // so a ticket is never supplanted and every one that fires is
        // the live one.
        self.unallocated_expiry.retire(frame, |key, _| {
            retire_unallocated(cache, slots, sides, free, key, frame)
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
    /// (see `EncodedCache::settle`), so it is asked for again on the
    /// next frame and the side is wiped again, for as long as it stays
    /// on screen. Both gates below are the same predicate — a rect
    /// taller or wider than an edge cannot be placed inside it — applied
    /// once to the ceiling and once to the current size.
    fn allocate(
        &mut self,
        device: &wgpu::Device,
        content: ContentType,
        size: U16Vec2,
    ) -> Option<etagere::Allocation> {
        if !self.sides[content as usize].fits_ceiling(size) {
            self.counters.oversized.bump();
            return None;
        }
        let need = size2(size.x as i32, size.y as i32);
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
            let must_grow = !self.sides[content as usize].fits_now(size);
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
    /// rasterizations, regenerable at any time.
    ///
    /// # What the clock protects
    ///
    /// What has been drawn *so far this frame*, not what the frame is
    /// going to draw. [`Self::current_frame`] moves in
    /// [`Self::advance_to`] at the end of a submit, and both
    /// [`Self::touch`] and [`Self::insert`] stamp that reading — so
    /// during a frame's prepare walk a slot the next batch still needs
    /// carries the previous frame's stamp and is exactly as eligible as
    /// a cold one.
    ///
    /// Under sustained pressure an early batch's misses can therefore
    /// take rectangles a later batch was about to hit. That batch's
    /// `TextEncoder::try_emit_cached` sees the generation move, drops its row and
    /// re-rasterizes, and the insert can take another slot the same
    /// frame still owes. The cascade is one pass rather than a loop,
    /// because everything already drawn this frame *is* protected, and
    /// it costs re-rasterization rather than a wrong pixel.
    ///
    /// Closing it means touching every glyph a frame will draw before
    /// rasterizing any of them — a whole-frame pre-pass, which the
    /// per-batch `prepare_batch` contract has no place to put.
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
            // A fourth write of `last_use` would break the invariant
            // that doc rests on, and the only symptom would be an atlas
            // that silently stops reclaiming. Debug pays one rotation to
            // keep it a checked contract.
            debug_assert!(
                ClockSweep::over(&self.slots, self.hand, target, self.current_frame)
                    .victim
                    .is_none(),
                "{target:?} latched dry on frame {} still has an evictable slot",
                self.current_frame,
            );
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
        self.free.release(&mut self.slots, &mut self.sides, idx);
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

    /// Double the side of `content`, reporting whether it moved.
    fn grow(&mut self, device: &wgpu::Device, content: ContentType) -> bool {
        if !self.sides[content as usize].grow(device, content) {
            return false;
        }
        self.counters.grows.bump();
        // Here rather than deferred behind a dirty flag: the side's old
        // texture is already gone, so any frame between the two would
        // sample a destroyed view.
        self.bound.rebind(device, &self.sides);
        true
    }
}

/// Settle one drained non-drawing ticket: `Some(due)` to re-file it,
/// `None` once it has been reclaimed or is no longer this wheel's
/// business.
///
/// A reclaimed entry re-inserts through `insert_unallocated` on next
/// use, with a fresh ticket. It releases through
/// [`FreeSlots::release`] like any other index, which skips the
/// generation bump on its own because the slot owns no rectangle.
///
/// A free function, and one that borrows the four fields rather than
/// `&mut self`, so the caller can hold `unallocated_expiry` borrowed
/// across the call to re-file into it.
fn retire_unallocated<K: Copy + Eq + Hash + Debug>(
    cache: &mut FxHashMap<K, u32>,
    slots: &mut [AtlasSlot],
    sides: &mut [Side],
    free: &mut FreeSlots,
    key: K,
    frame: u64,
) -> Option<u64> {
    // Gone: reclaimed by an earlier ticket, or its key removed by
    // eviction. One probe covers the read and the removal below, the way
    // the other two wheel owners settle their entries.
    let Entry::Occupied(entry) = cache.entry(key) else {
        return None;
    };
    let idx = *entry.get();
    let slot = &slots[idx as usize];
    // Allocated entries are the clock's to reclaim, and it advances
    // their generation when it does. Defensive rather than reachable —
    // every path that allocates over a slab index removes the old key
    // from `cache` first, so the lookup above would have missed.
    if slot.placement.is_some() {
        return None;
    }
    // `touch` refreshes `last_use` without filing anything, so the real
    // deadline is re-read here.
    let dies_at = unallocated_dies_at(slot.last_use);
    if dies_at > frame {
        return Some(dies_at);
    }
    entry.remove();
    free.release(slots, sides, idx);
    None
}

/// The frame a non-drawing entry last used on `last_use` is first dead —
/// what the wheel files under. One expression, read by the filing in
/// [`RasterAtlas::insert_unallocated`] and by the re-file in
/// [`retire_unallocated`], so the two cannot name different frames.
const fn unallocated_dies_at(last_use: u64) -> u64 {
    last_use + UNALLOCATED_KEEP_FRAMES + 1
}

#[cfg(test)]
mod tests;
