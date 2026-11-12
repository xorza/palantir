//! [`AtlasHandle`]: an item of the pool, and [`Placement`], where it sits now.

use crate::gpu::atlas::atlas_format::AtlasFormat;
use glam::U16Vec2;

/// An item a tenant holds: its list, its slot in the list's slab and the generation it took the
/// slot at. A move keeps the handle; a release or an eviction moves the slot's generation, so a
/// handle kept past it reads as stale.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct AtlasHandle {
    pub(crate) format: AtlasFormat,
    pub(crate) idx: u32,
    pub(crate) generation: u32,
}

/// Where an item sits: its page, the slot of the list's binding a draw reads it through, and its
/// rectangle there, in texels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Placement {
    pub(crate) page: u8,
    pub(crate) origin: U16Vec2,
    pub(crate) size: U16Vec2,
}

impl Placement {
    /// The texels the rectangle holds.
    pub(crate) fn texels(self) -> u64 {
        u64::from(self.size.x) * u64::from(self.size.y)
    }
}
