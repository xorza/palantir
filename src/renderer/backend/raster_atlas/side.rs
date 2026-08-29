//! One per-content-type atlas texture: its allocator, its growth by
//! doubling, and the old texture preserved across a grow.

use crate::renderer::backend::raster_atlas::content_type::ContentType;
use etagere::{BucketedAtlasAllocator, size2};

const ATLAS_GROWTH_FACTOR: u32 = 2;

/// One per-content-type backing store, indexed by `ContentType as usize`.
/// Owns its texture from first allocation through every doubling, so the
/// atlas above it never names a `wgpu::Texture` directly.
pub(super) struct Side {
    pub(super) texture: wgpu::Texture,
    pub(super) view: wgpu::TextureView,
    pub(super) size: u32,
    /// Largest edge this side will ever reach — see
    /// [`Side::growth_ceiling`].
    ///
    /// Resolved once at construction because its three inputs never
    /// change, and [`RasterAtlas::allocate`](super::RasterAtlas) reads it
    /// per entry it is asked to place: recomputing meant a `u64` divide
    /// and an `isqrt` on every glyph and icon that missed the cache.
    ceiling: u32,
    /// The frame a full clock rotation over this side last came up
    /// empty on, or `None` until one has.
    ///
    /// A rotation is O(slab), and `allocate` asks for a victim once for
    /// every entry it cannot place — so a frame asking for more than the
    /// ceiling holds pays that walk per starving entry, which is
    /// quadratic in the slab. Every one of those walks is provably
    /// wasted: a slot is eligible only while `last_use < current_frame`,
    /// and `last_use` never moves *down* within a frame (`touch` and
    /// `store` both stamp it with `current_frame`), so once a side has
    /// been walked dry nothing can become evictable until the clock
    /// advances. Remembering which frame that happened on turns the
    /// second and every later miss into one comparison.
    pub(super) dry_frame: Option<u64>,
    pub(super) packer: BucketedAtlasAllocator,
    /// On grow, the previous-frame texture is moved here so the
    /// shared-encoder flush can record the copy alongside pending
    /// glyph writes. `None` whenever there's no pending grow blit
    /// for this side.
    pub(super) pending_grow: Option<PendingGrow>,
    /// GPU debug name for this side's texture, built once here.
    ///
    /// A grow replaces the texture and reuses the name, which is what
    /// keeps the atlas's label stem from having to travel down to
    /// [`Self::grow`] — and keeps the two sites from formatting it two
    /// ways.
    label: String,
}

/// Old texture + its size (= square edge length, == old.width ==
/// old.height) preserved across the grow point. Consumed by
/// [`RasterAtlas::flush_pending_uploads`](super::RasterAtlas).
#[derive(Debug)]
pub(super) struct PendingGrow {
    pub(super) old_texture: wgpu::Texture,
    pub(super) old_size: u32,
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
    pub(super) fn new(
        device: &wgpu::Device,
        content: ContentType,
        size: u32,
        ceiling: u32,
        stem: &str,
    ) -> Self {
        let label = format!("{stem} {} atlas", content.side_name());
        let texture = make_texture(device, content.format(), size, &label);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            texture,
            view,
            size,
            ceiling,
            dry_frame: None,
            packer: BucketedAtlasAllocator::new(size2(size as i32, size as i32)),
            pending_grow: None,
            label,
        }
    }

    /// Largest side length a `content` atlas will grow to: whichever of
    /// the device maximum and the instance's byte budget binds first.
    ///
    /// Takes the device limit rather than reading it off a
    /// `wgpu::Device`, so the arithmetic is testable without one.
    pub(super) fn growth_ceiling(
        max_texture_dimension_2d: u32,
        content: ContentType,
        max_bytes: u64,
    ) -> u32 {
        let by_bytes = (max_bytes / u64::from(content.bytes_per_pixel())).isqrt() as u32;
        max_texture_dimension_2d.min(by_bytes)
    }

    /// Whether a `width × height` rect can be placed inside this side as
    /// it stands. Exact rather than conservative, and that is what makes
    /// it usable as a gate: the packer is configured with one column and
    /// unit alignment, so its own reject is `w > edge || h > edge` and
    /// this agrees with it texel for texel. A stricter test would refuse
    /// entries the packer would have taken.
    pub(super) const fn fits_now(&self, width: u16, height: u16) -> bool {
        fits_edge(width, height, self.size)
    }

    /// Whether a `width × height` rect could *ever* be placed here — the
    /// one question [`RasterAtlas::allocate`](super::RasterAtlas) has to
    /// answer before it is allowed to evict anything, since freeing
    /// rectangles cannot widen a texture.
    pub(super) const fn fits_ceiling(&self, width: u16, height: u16) -> bool {
        fits_edge(width, height, self.ceiling)
    }

    /// Double this side's texture, stashing the old one for the grow
    /// blit. `false` at the ceiling, where the atlas holds its size by
    /// recycling rectangles instead.
    ///
    /// etagere preserves rects on `packer.grow`, so the cache stays valid
    /// — no re-rasterization, and no cached uv to invalidate.
    pub(super) fn grow(&mut self, device: &wgpu::Device, content: ContentType) -> bool {
        if self.size >= self.ceiling {
            return false;
        }
        let new_size = (self.size * ATLAS_GROWTH_FACTOR).min(self.ceiling);
        let new_texture = make_texture(device, content.format(), new_size, &self.label);
        let old_size = self.size;
        let old_texture = std::mem::replace(&mut self.texture, new_texture);

        // If a previous grow this frame hasn't flushed yet, keep the
        // oldest texture — that's the one holding live pixel data
        // (the intermediate-size texture was never written into).
        if self.pending_grow.is_none() {
            self.pending_grow = Some(PendingGrow {
                old_texture,
                old_size,
            });
        }

        self.view = self
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.size = new_size;
        self.packer.grow(size2(new_size as i32, new_size as i32));
        true
    }
}

/// Whether a `width × height` rect can be placed inside a square side of
/// `edge` texels.
const fn fits_edge(width: u16, height: u16, edge: u32) -> bool {
    width as u32 <= edge && height as u32 <= edge
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
