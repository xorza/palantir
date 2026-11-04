//! One per-content-type atlas texture and the eviction latch its clock keeps.

use crate::gpu::device::gpu::Gpu;
use crate::gpu::pipeline::raster_pipeline::raster_quad::U_BITS;
use crate::gpu::raster::raster_atlas::RasterAtlasConfig;
use crate::gpu::resource::packed_atlas::{AtlasSpec, PackedAtlas};
use crate::primitives::paint::content_type::ContentType;

/// The widest a side grows: a quad packs a slot's `u` in [`U_BITS`], so every origin lies below
/// `1 << U_BITS` whatever the budget asks. Refused: a check of the budget's edge against the
/// encoding, which a device edge past it would still have passed.
const EDGE_MAX: u32 = 1 << U_BITS;

/// One per-content-type backing store, indexed by `ContentType as usize`.
#[derive(Debug)]
pub(super) struct Side {
    /// The texture and its packer, grown by doubling up to the side's budgets.
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
    /// The `content` side of an atlas configured by `config`, at its initial size within its
    /// budget.
    pub(super) fn new(gpu: &Gpu, content: ContentType, config: RasterAtlasConfig) -> Self {
        let initial = match content {
            ContentType::Mask => config.initial_mask_px,
            ContentType::Color => config.initial_color_px,
        };
        Self {
            atlas: PackedAtlas::new(
                gpu,
                spec(content, config, gpu.max_texture_dim().get()),
                initial,
            ),
            dry_frame: None,
        }
    }
}

/// The texture of the `content` side of an atlas configured by `config`, on a device whose edge
/// is `device_edge`: no wider than the device or [`EDGE_MAX`].
fn spec(content: ContentType, config: RasterAtlasConfig, device_edge: u32) -> AtlasSpec {
    AtlasSpec {
        label: format!("{} {} atlas", config.label, content.side_name()),
        format: texture_format(content),
        usage: wgpu::TextureUsages::TEXTURE_BINDING,
        ceiling: device_edge.min(EDGE_MAX),
        budget_bytes: config.budget_bytes,
        max_bytes: config.max_bytes,
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

#[cfg(test)]
mod tests {
    use crate::gpu::raster::raster_atlas::RasterAtlasConfig;
    use crate::gpu::raster::raster_atlas::side;
    use crate::primitives::paint::content_type::ContentType;

    /// A side is no wider than a quad's 14-bit `u` reaches, 16384 texels, though the device
    /// and the budget allow 32768; a narrower device binds first, at 8192.
    #[test]
    fn a_side_stays_inside_the_uv_encoding() {
        let config = RasterAtlasConfig {
            label: "test",
            initial_mask_px: 64,
            initial_color_px: 64,
            max_bytes: u64::MAX,
            budget_bytes: u64::MAX,
        };
        assert_eq!(side::spec(ContentType::Mask, config, 32768).ceiling, 16384);
        assert_eq!(side::spec(ContentType::Color, config, 8192).ceiling, 8192);
    }
}
