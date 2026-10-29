//! One per-content-type atlas texture and the eviction latch its clock keeps.

use crate::gpu::raster::raster_atlas::RasterAtlasConfig;
use crate::gpu::resource::packed_atlas::{AtlasSpec, PackedAtlas};
use crate::primitives::paint::content_type::ContentType;

/// One per-content-type backing store, indexed by `ContentType as usize`.
#[derive(Debug)]
pub(super) struct Side {
    /// The texture and its packer, grown by doubling up to [`Side::growth_ceiling`].
    pub(super) atlas: PackedAtlas,
    /// The frame a full clock rotation over this side last came up empty on.
    ///
    /// A rotation is O(slab) and `allocate` asks for a victim per entry it can't
    /// place, so a frame needing more than the ceiling holds would pay it per
    /// starving entry. Each walk is wasted: `last_use` never moves down within a
    /// frame, so once walked dry nothing is evictable until the clock advances.
    pub(super) dry_frame: Option<u64>,
}

impl Side {
    /// The `content` side of an atlas configured by `config`, at its initial size within the device's cap.
    pub(super) fn new(
        device: &wgpu::Device,
        content: ContentType,
        config: RasterAtlasConfig,
    ) -> Self {
        let max = device.limits().max_texture_dimension_2d;
        let initial = match content {
            ContentType::Mask => config.initial_mask_px,
            ContentType::Color => config.initial_color_px,
        };
        let spec = AtlasSpec {
            label: format!("{} {} atlas", config.label, content.side_name()),
            format: texture_format(content),
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            ceiling: Self::growth_ceiling(max, content, config.max_bytes),
        };
        Self {
            atlas: PackedAtlas::new(device, spec, initial),
            dry_frame: None,
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
