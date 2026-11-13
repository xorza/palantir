//! [`AtlasSlots`]: the open page slots of the lists a raster's and a shadow's pipelines read,
//! which they are built for.

use crate::gpu::atlas::AtlasPool;
use crate::gpu::atlas::atlas_format::AtlasFormat;
use crate::gpu::atlas::page_slots::PageSlots;

/// The open slots of the lists a frame's raster and shadow draws read, after the pool's commit:
/// what their pipelines are built for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AtlasSlots {
    /// The `R8` list's: glyph and icon masks, and the shadows' cutout tables.
    pub(crate) mask: PageSlots,
    /// The `Rgba8` list's: color glyphs and icons.
    pub(crate) color: PageSlots,
}

impl AtlasSlots {
    /// Slot 0 of each list.
    pub(crate) const FIRST: Self = Self {
        mask: PageSlots::FIRST,
        color: PageSlots::FIRST,
    };

    /// The slots `pool`'s lists hold open now.
    pub(crate) fn of(pool: &AtlasPool) -> Self {
        Self {
            mask: pool.list(AtlasFormat::R8).slots(),
            color: pool.list(AtlasFormat::Rgba8).slots(),
        }
    }
}
