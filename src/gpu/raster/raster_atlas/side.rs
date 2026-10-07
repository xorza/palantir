//! One per-content-type atlas texture: its allocator, growth by doubling, and
//! the old texture preserved across a grow.

use crate::primitives::paint::content_type::ContentType;
use etagere::{BucketedAtlasAllocator, size2};
use glam::U16Vec2;
use std::fmt;
use std::mem;

const ATLAS_GROWTH_FACTOR: u32 = 2;

/// One per-content-type backing store, indexed by `ContentType as usize`; owns
/// its texture through every doubling.
pub(super) struct Side {
    pub(super) texture: wgpu::Texture,
    pub(super) view: wgpu::TextureView,
    pub(super) size: u32,
    /// Largest edge this side will reach ([`Side::growth_ceiling`]), resolved once
    /// because `RasterAtlas::allocate` reads it per entry and recomputing cost a
    /// `u64` divide and an `isqrt` per cache miss.
    ceiling: u32,
    /// The frame a full clock rotation over this side last came up empty on.
    ///
    /// A rotation is O(slab) and `allocate` asks for a victim per entry it can't
    /// place, so a frame needing more than the ceiling holds would pay it per
    /// starving entry. Each walk is wasted: `last_use` never moves down within a
    /// frame, so once walked dry nothing is evictable until the clock advances.
    pub(super) dry_frame: Option<u64>,
    pub(super) packer: BucketedAtlasAllocator,
    /// On grow, the previous-frame texture is moved here so the shared-encoder
    /// flush can record the copy. `None` when no grow blit is pending.
    pub(super) pending_grow: Option<PendingGrow>,
    /// GPU debug name for this side's texture, built once so a grow reuses it.
    label: String,
}

/// Old texture and its size (square edge length) preserved across the grow
/// point; consumed by
/// [`RasterAtlas::flush_pending_uploads`](super::RasterAtlas).
#[derive(Debug)]
pub(super) struct PendingGrow {
    pub(super) old_texture: wgpu::Texture,
    pub(super) old_size: u32,
}

// Manual: etagere's `BucketedAtlasAllocator` isn't `Debug`.
impl fmt::Debug for Side {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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
        let texture = make_texture(device, texture_format(content), size, &label);
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

    /// Largest side length a `content` atlas will grow to: the device maximum or
    /// the byte budget, whichever binds first. Takes the device limit so it is
    /// testable without a `wgpu::Device`.
    pub(super) fn growth_ceiling(
        max_texture_dimension_2d: u32,
        content: ContentType,
        max_bytes: u64,
    ) -> u32 {
        let by_bytes = (max_bytes / u64::from(content.bytes_per_pixel())).isqrt() as u32;
        max_texture_dimension_2d.min(by_bytes)
    }

    /// Whether a rect of `size` can be placed as the side stands. Exact, so it can
    /// gate: the packer uses one column and unit alignment, so its reject is
    /// `w > edge || h > edge`, matching this texel for texel.
    pub(super) const fn fits_now(&self, size: U16Vec2) -> bool {
        fits_edge(size, self.size)
    }

    /// Whether a rect of `size` could *ever* be placed here, asked before evicting
    /// since freeing rectangles can't widen a texture.
    pub(super) const fn fits_ceiling(&self, size: U16Vec2) -> bool {
        fits_edge(size, self.ceiling)
    }

    /// Double this side's texture, stashing the old one for the grow blit. `false`
    /// at the ceiling, where the atlas recycles rectangles. etagere preserves
    /// rects on `packer.grow`, so no re-rasterization or uv invalidation.
    pub(super) fn grow(&mut self, device: &wgpu::Device, content: ContentType) -> bool {
        if self.size >= self.ceiling {
            return false;
        }
        let new_size = (self.size * ATLAS_GROWTH_FACTOR).min(self.ceiling);
        let new_texture = make_texture(device, texture_format(content), new_size, &self.label);
        let old_size = self.size;
        let old_texture = mem::replace(&mut self.texture, new_texture);

        // A pending grow this frame keeps the oldest texture: it holds the live pixels.
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

/// Whether a rect of `size` fits inside a square side of `edge` texels.
const fn fits_edge(size: U16Vec2, edge: u32) -> bool {
    size.x as u32 <= edge && size.y as u32 <= edge
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

/// The texture format an atlas side stores `content` in. Here rather than on
/// [`ContentType`] (primitives-layer): naming a device format is the graphics
/// layer's job.
const fn texture_format(content: ContentType) -> wgpu::TextureFormat {
    match content {
        ContentType::Mask => wgpu::TextureFormat::R8Unorm,
        ContentType::Color => wgpu::TextureFormat::Rgba8UnormSrgb,
    }
}
