//! Glyph atlas: one struct for both mask + color content.

use cosmic_text::CacheKey;
use etagere::{AllocId, BucketedAtlasAllocator, size2};
use rustc_hash::FxHashMap;
use wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;

use crate::renderer::backend::gpu_ctx::GpuCtx;
use crate::renderer::backend::text::ContentType;

/// Initial atlas side length. Bumped from glyphon's 256 to skip the
/// 256→512→1024 grow chain on first frame with non-trivial text.
const INITIAL_ATLAS_SIZE: u32 = 1024;
const ATLAS_GROWTH_FACTOR: u32 = 2;

/// Sweep cadence (frames) for stale zero-area entries (`alloc: None`).
/// `evict_one` skips them (nothing to deallocate), so every whitespace
/// glyph at every scale rung would otherwise accumulate forever and
/// bloat its linear scan. 512 ≈ 8 s at 60 fps — far outside any
/// flicker, and rare enough that the O(map) retain amortizes to noise.
const EMPTY_SWEEP_INTERVAL: u64 = 512;

#[derive(Clone, Copy, Debug)]
pub(crate) struct GlyphSlot {
    pub(crate) x: u16,
    pub(crate) y: u16,
    pub(crate) width: u16,
    pub(crate) height: u16,
    pub(crate) left: i16,
    pub(crate) top: i16,
    pub(crate) content: ContentType,
    pub(crate) alloc: Option<AllocId>,
    pub(crate) last_use: u64,
}

/// One per-content-type backing store. Indexed by `ContentType as usize`.
pub(crate) struct Side {
    pub(crate) texture: wgpu::Texture,
    pub(crate) view: wgpu::TextureView,
    pub(crate) size: u32,
    pub(crate) packer: BucketedAtlasAllocator,
    pub(crate) format: wgpu::TextureFormat,
    pub(crate) bpp: u32,
    pub(crate) label: &'static str,
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
pub(crate) struct PendingGrow {
    pub(crate) old_texture: wgpu::Texture,
    pub(crate) old_size: u32,
}

#[derive(Debug)]
pub(crate) struct GlyphAtlas {
    pub(crate) sides: [Side; 2],
    /// Dense slot slab; `cache` maps each key to an index into it.
    /// Encoded-run caches record these indices so their hot-path LRU
    /// refresh is an indexed store instead of a map probe per glyph —
    /// safe because every recorded index is validated against
    /// `eviction_count` before use, and only `evict_one` (which bumps
    /// it) ever reassigns an *allocated* slot's index.
    pub(crate) slots: Vec<GlyphSlot>,
    pub(crate) cache: FxHashMap<CacheKey, u32>,
    /// Slab indices freed by `evict_one` / the empty sweep, reused by
    /// the next `store`.
    free: Vec<u32>,
    pub(crate) current_frame: u64,
    /// Bumped every time `evict_one` reuses a slot. Encoded-glyph
    /// caches keyed on slot positions latch this on insert and
    /// re-validate on lookup; any eviction invalidates every entry
    /// (conservative — slot rectangles are stable across grows because
    /// `etagere::grow` preserves rects).
    pub(crate) eviction_count: u64,
    pub(crate) max_texture_dimension_2d: u32,
    /// Set on grow; the renderer rebuilds its bind group and clears it.
    pub(crate) bind_group_dirty: bool,

    /// Glyph pixel data queued by `insert`, packed with per-row padding
    /// so each glyph's copy can satisfy
    /// `wgpu::COPY_BYTES_PER_ROW_ALIGNMENT = 256`. Drained by
    /// [`Self::flush_pending_uploads`] into one staging buffer + one
    /// encoder with N `copy_buffer_to_texture` commands.
    pending_staging: Vec<u8>,
    pending_copies: Vec<PendingCopy>,
    /// Retained staging buffer; grown on demand, reused across frames.
    staging_buf: Option<wgpu::Buffer>,
    staging_cap: u64,
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
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let max = device.limits().max_texture_dimension_2d;
        let size = INITIAL_ATLAS_SIZE.min(max);

        // Order matches `ContentType as usize`: [Mask, Color].
        let sides = [
            Side::new(
                device,
                size,
                wgpu::TextureFormat::R8Unorm,
                1,
                "aperture text mask atlas",
            ),
            Side::new(
                device,
                size,
                wgpu::TextureFormat::Rgba8UnormSrgb,
                4,
                "aperture text color atlas",
            ),
        ];

        Self {
            sides,
            slots: Vec::new(),
            cache: FxHashMap::default(),
            free: Vec::new(),
            current_frame: 1,
            eviction_count: 0,
            max_texture_dimension_2d: max,
            bind_group_dirty: false,
            pending_staging: Vec::new(),
            pending_copies: Vec::new(),
            staging_buf: None,
            staging_cap: 0,
        }
    }

    pub(crate) fn mask_view(&self) -> &wgpu::TextureView {
        &self.sides[ContentType::Mask as usize].view
    }
    pub(crate) fn color_view(&self) -> &wgpu::TextureView {
        &self.sides[ContentType::Color as usize].view
    }
    pub(crate) fn mask_size(&self) -> u32 {
        self.sides[ContentType::Mask as usize].size
    }
    pub(crate) fn color_size(&self) -> u32 {
        self.sides[ContentType::Color as usize].size
    }

    /// Cache-hit fast path: bump the slot's LRU stamp and return its
    /// slab index (read the slot itself via `self.slots[idx]`).
    pub(crate) fn touch(&mut self, key: &CacheKey) -> Option<u32> {
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
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn insert(
        &mut self,
        device: &wgpu::Device,
        key: CacheKey,
        content: ContentType,
        width: u16,
        height: u16,
        left: i16,
        top: i16,
        pixels: &[u8],
    ) -> Option<u32> {
        let alloc = self.allocate(device, content, width, height)?;
        self.enqueue_upload(
            content,
            alloc.rectangle.min.x as u32,
            alloc.rectangle.min.y as u32,
            width as u32,
            height as u32,
            pixels,
        );

        let slot = GlyphSlot {
            x: alloc.rectangle.min.x as u16,
            y: alloc.rectangle.min.y as u16,
            width,
            height,
            left,
            top,
            content,
            alloc: Some(alloc.id),
            last_use: self.current_frame,
        };
        Some(self.store(key, slot))
    }

    /// Park `slot` in the slab (reusing a freed index when available)
    /// and map `key` to it.
    fn store(&mut self, key: CacheKey, slot: GlyphSlot) -> u32 {
        let idx = match self.free.pop() {
            Some(i) => {
                self.slots[i as usize] = slot;
                i
            }
            None => {
                self.slots.push(slot);
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
        let bpp = self.sides[content as usize].bpp;
        let unpadded = width * bpp;
        let bytes_per_row = unpadded.next_multiple_of(COPY_BYTES_PER_ROW_ALIGNMENT);
        // Start each glyph at a 256-aligned offset so the buffer-offset
        // alignment requirement holds for every PendingCopy.
        let start = self.pending_staging.len() as u64;
        let aligned_start = start.next_multiple_of(COPY_BYTES_PER_ROW_ALIGNMENT as u64);
        if aligned_start > start {
            self.pending_staging.resize(aligned_start as usize, 0);
        }
        let region_bytes = bytes_per_row as usize * height as usize;
        let region_start = self.pending_staging.len();
        self.pending_staging.resize(region_start + region_bytes, 0);
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
            staging_offset: aligned_start,
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
                    ctx.encoder
                        .push_debug_group("aperture text atlas grow blit");
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
            ctx.encoder.pop_debug_group();
        }

        if self.pending_copies.is_empty() {
            return;
        }
        let bytes = self.pending_staging.len() as u64;
        if bytes > self.staging_cap || self.staging_buf.is_none() {
            let new_cap = bytes
                .next_power_of_two()
                .max(self.staging_cap * 2)
                .max(4096);
            self.staging_buf = Some(ctx.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("aperture text atlas staging"),
                size: new_cap,
                usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            self.staging_cap = new_cap;
        }
        let buf = self.staging_buf.as_ref().unwrap();
        ctx.write(buf, 0, &self.pending_staging);

        ctx.encoder
            .push_debug_group("aperture text atlas batch upload");
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
        ctx.encoder.pop_debug_group();

        self.pending_staging.clear();
        self.pending_copies.clear();
    }

    /// Insert a zero-area glyph entry (no atlas slot, no upload).
    /// Subsequent lookups still hit the cache and skip swash. Returns
    /// the entry's slab index.
    pub(crate) fn insert_empty(
        &mut self,
        key: CacheKey,
        content: ContentType,
        left: i16,
        top: i16,
    ) -> u32 {
        let slot = GlyphSlot {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
            left,
            top,
            content,
            alloc: None,
            last_use: self.current_frame,
        };
        self.store(key, slot)
    }

    /// Frame teardown: advance the LRU frame counter and periodically
    /// sweep stale zero-area entries.
    pub(crate) fn end_frame(&mut self) {
        self.current_frame += 1;
        sweep_stale_empties(
            &mut self.cache,
            &self.slots,
            &mut self.free,
            self.current_frame,
        );
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
            if !self.evict_one(content) && !self.grow(device, content) {
                return None;
            }
        }
    }

    /// Evict any glyph of `target` content with `last_use <
    /// current_frame`. Linear scan over the glyph cache — but the cache
    /// is keyed on distinct `(glyph, scale, subpixel-bin)` rasterizations
    /// *in view*, not glyph instances, so for UI text (small alphabets;
    /// the `TEXT_SCALE_STEP` ladder bounding distinct scales) it stays in
    /// the tens-to-low-hundreds. Profiling the worst case (`text_atlas/
    /// zoom_cold` — a fresh scale rung every frame, so eviction fires for
    /// nearly every glyph) put this below 0.3 % of frame: invisible next
    /// to the per-glyph LRU refresh and the GPU submit.
    /// An O(1) intrusive LRU would only pay off for a
    /// many-thousand-unique-glyph workload (zooming a full CJK document,
    /// say); not worth the complexity until such a workload exists.
    fn evict_one(&mut self, target: ContentType) -> bool {
        let cf = self.current_frame;
        let Some((key, idx)) = self.cache.iter().find_map(|(k, &i)| {
            let s = &self.slots[i as usize];
            (s.content == target && s.last_use < cf && s.alloc.is_some()).then_some((*k, i))
        }) else {
            return false;
        };
        self.cache.remove(&key);
        let id = self.slots[idx as usize].alloc.take().unwrap();
        self.sides[target as usize].packer.deallocate(id);
        self.free.push(idx);
        self.eviction_count += 1;
        true
    }

    /// Double the atlas of `content`. Returns `false` at GPU-max. On
    /// success, stashes the old texture into `Side::pending_grow` so
    /// `flush_pending_uploads` can record the old→new blit on the
    /// shared encoder. etagere preserves rects on `packer.grow`, so
    /// the cache stays valid — no re-rasterization.
    fn grow(&mut self, device: &wgpu::Device, content: ContentType) -> bool {
        let side = &mut self.sides[content as usize];
        if side.size >= self.max_texture_dimension_2d {
            return false;
        }
        let new_size = (side.size * ATLAS_GROWTH_FACTOR).min(self.max_texture_dimension_2d);
        let new_texture = make_texture(device, side.format, new_size, side.label);
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
            .field("format", &self.format)
            .field("bpp", &self.bpp)
            .field("label", &self.label)
            .finish_non_exhaustive()
    }
}

impl Side {
    fn new(
        device: &wgpu::Device,
        size: u32,
        format: wgpu::TextureFormat,
        bpp: u32,
        label: &'static str,
    ) -> Self {
        let texture = make_texture(device, format, size, label);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            texture,
            view,
            size,
            packer: BucketedAtlasAllocator::new(size2(size as i32, size as i32)),
            format,
            bpp,
            label,
            pending_grow: None,
        }
    }
}

/// Drop zero-area entries (`alloc: None`) not used within the last
/// [`EMPTY_SWEEP_INTERVAL`] frames, returning their slab indices to
/// `free`. Runs only on interval frames so steady-state `end_frame`
/// stays O(1). Allocated entries are `evict_one`'s job; a swept empty
/// re-inserts via `insert_empty` on next use. No `eviction_count`
/// bump: empty slots carry no uv coords and encoded-run caches never
/// record them, so no encoded-cache entry can go stale.
fn sweep_stale_empties(
    cache: &mut FxHashMap<CacheKey, u32>,
    slots: &[GlyphSlot],
    free: &mut Vec<u32>,
    current_frame: u64,
) {
    if !current_frame.is_multiple_of(EMPTY_SWEEP_INTERVAL) {
        return;
    }
    let cutoff = current_frame - EMPTY_SWEEP_INTERVAL;
    cache.retain(|_, idx| {
        let s = &slots[*idx as usize];
        let keep = s.alloc.is_some() || s.last_use >= cutoff;
        if !keep {
            free.push(*idx);
        }
        keep
    });
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

#[cfg(test)]
mod tests {
    use super::*;
    use cosmic_text::{CacheKeyFlags, SubpixelBin, fontdb};

    fn key(glyph_id: u16) -> CacheKey {
        CacheKey {
            font_id: fontdb::ID::dummy(),
            glyph_id,
            font_size_bits: 14.0_f32.to_bits(),
            x_bin: SubpixelBin::Zero,
            y_bin: SubpixelBin::Zero,
            font_weight: fontdb::Weight::NORMAL,
            flags: CacheKeyFlags::empty(),
        }
    }

    fn slot(alloc: Option<AllocId>, last_use: u64) -> GlyphSlot {
        GlyphSlot {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
            left: 0,
            top: 0,
            content: ContentType::Mask,
            alloc,
            last_use,
        }
    }

    #[test]
    fn empty_sweep_drops_only_stale_unallocated_entries() {
        // Sweep at frame 1024 uses cutoff 1024 - 512 = 512: empties
        // with last_use < 512 go, everything else stays.
        let slots = vec![
            slot(None, 1),                          // stale empty -> swept
            slot(None, 512),                        // empty exactly at cutoff -> kept
            slot(None, 1024),                       // fresh empty -> kept
            slot(Some(AllocId::deserialize(0)), 1), // stale but allocated -> kept
        ];
        let mut cache = FxHashMap::default();
        for i in 0..slots.len() as u32 {
            cache.insert(key(i as u16 + 1), i);
        }
        let mut free = Vec::new();

        // Off-interval frame: no-op even though key(1) is already stale.
        sweep_stale_empties(&mut cache, &slots, &mut free, 1023);
        assert_eq!(cache.len(), 4);
        assert!(free.is_empty());

        sweep_stale_empties(&mut cache, &slots, &mut free, 1024);
        assert!(!cache.contains_key(&key(1)), "stale empty must be swept");
        assert!(cache.contains_key(&key(2)), "last_use == cutoff survives");
        assert!(cache.contains_key(&key(3)), "fresh empty survives");
        assert!(
            cache.contains_key(&key(4)),
            "allocated entry is never swept"
        );
        // The swept entry's slab slot is handed back for reuse.
        assert_eq!(free, vec![0]);
    }
}
