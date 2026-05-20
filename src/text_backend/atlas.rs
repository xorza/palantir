//! Glyph atlas: one struct for both mask + color content.

use cosmic_text::CacheKey;
use etagere::{AllocId, BucketedAtlasAllocator, size2};
use rustc_hash::FxHashMap;
use wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;

use super::ContentType;
use crate::renderer::backend::GpuCtx;

/// Initial atlas side length. Bumped from glyphon's 256 to skip the
/// 256→512→1024 grow chain on first frame with non-trivial text.
const INITIAL_ATLAS_SIZE: u32 = 1024;
const ATLAS_GROWTH_FACTOR: u32 = 2;

#[derive(Clone, Copy)]
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
pub(crate) struct PendingGrow {
    pub(crate) old_texture: wgpu::Texture,
    pub(crate) old_size: u32,
}

pub(crate) struct GlyphAtlas {
    pub(crate) sides: [Side; 2],
    pub(crate) cache: FxHashMap<CacheKey, GlyphSlot>,
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

#[derive(Clone, Copy)]
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
                "palantir text mask atlas",
            ),
            Side::new(
                device,
                size,
                wgpu::TextureFormat::Rgba8UnormSrgb,
                4,
                "palantir text color atlas",
            ),
        ];

        Self {
            sides,
            cache: FxHashMap::default(),
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

    /// Cache-hit fast path.
    pub(crate) fn touch(&mut self, key: &CacheKey) -> Option<GlyphSlot> {
        let slot = self.cache.get_mut(key)?;
        slot.last_use = self.current_frame;
        Some(*slot)
    }

    /// Insert a freshly-rasterized glyph. Queues the pixel data into
    /// a per-frame staging buffer (drained by
    /// [`Self::flush_pending_uploads`] before the text pass) so all
    /// glyph uploads land in one encoder/submit instead of N separate
    /// `queue.write_texture` calls. Grows if full; returns `None`
    /// only at GPU-max and still doesn't fit.
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
    ) -> Option<GlyphSlot> {
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
        self.cache.insert(key, slot);
        Some(slot)
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
        let bytes_per_row =
            unpadded.div_ceil(COPY_BYTES_PER_ROW_ALIGNMENT) * COPY_BYTES_PER_ROW_ALIGNMENT;
        // Start each glyph at a 256-aligned offset so the buffer-offset
        // alignment requirement holds for every PendingCopy.
        let start = self.pending_staging.len() as u64;
        let aligned_start = start.div_ceil(COPY_BYTES_PER_ROW_ALIGNMENT as u64)
            * COPY_BYTES_PER_ROW_ALIGNMENT as u64;
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
                        .push_debug_group("palantir text atlas grow blit");
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
                label: Some("palantir text atlas staging"),
                size: new_cap,
                usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            self.staging_cap = new_cap;
        }
        let buf = self.staging_buf.as_ref().unwrap();
        ctx.write(buf, 0, &self.pending_staging);

        ctx.encoder
            .push_debug_group("palantir text atlas batch upload");
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
    /// Subsequent lookups still hit the cache and skip swash.
    pub(crate) fn insert_empty(
        &mut self,
        key: CacheKey,
        content: ContentType,
        left: i16,
        top: i16,
    ) -> GlyphSlot {
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
        self.cache.insert(key, slot);
        slot
    }

    /// Frame teardown: bump LRU counter.
    pub(crate) fn trim(&mut self) {
        self.current_frame += 1;
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
    /// current_frame`. Linear scan; eviction is rare in practice.
    fn evict_one(&mut self, target: ContentType) -> bool {
        let cf = self.current_frame;
        let Some(key) = self.cache.iter().find_map(|(k, s)| {
            (s.content == target && s.last_use < cf && s.alloc.is_some()).then_some(*k)
        }) else {
            return false;
        };
        let slot = self.cache.remove(&key).unwrap();
        if let Some(id) = slot.alloc {
            self.sides[target as usize].packer.deallocate(id);
        }
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
