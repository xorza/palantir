//! [`Page`]: one page of a list, its packer and what its items fill.

use crate::gpu::atlas::atlas_handle::Placement;
use crate::gpu::atlas::page_list::{DrainCount, edge_u16};
use crate::gpu::resource::shelf_packer::ShelfPacker;
use glam::U16Vec2;

/// One page: its packer, whose edge is the page's, the texels its items fill, which opening of its
/// slot it is, and what its last drain that found no room counted.
#[derive(Debug)]
pub(crate) struct Page {
    pub(super) packer: ShelfPacker,
    pub(super) live_texels: u64,
    /// The list's count of pages opened when this one opened, so the GPU half tells a page grown
    /// in its slot, whose texels it copies, from another opened there since, whose it does not.
    pub(super) epoch: u32,
    /// What the page's last drain that found no room for an item counted, or its last shrink that
    /// no smaller page packs, so the same drain is not planned again while nothing it counts
    /// changed: the free texels fit its items, but not their shapes. A page's own, as another
    /// page's drain may still find room while this one's cannot.
    pub(super) stalled: Option<DrainCount>,
}

impl Page {
    pub(super) fn new(edge: u32, epoch: u32) -> Self {
        Self {
            packer: ShelfPacker::new(edge_u16(edge)),
            live_texels: 0,
            epoch,
            stalled: None,
        }
    }

    pub(crate) const fn epoch(&self) -> u32 {
        self.epoch
    }

    pub(crate) fn edge(&self) -> u32 {
        u32::from(self.packer.edge())
    }

    /// The texels the page holds, `edge²`.
    pub(super) fn texels(&self) -> u64 {
        u64::from(self.edge()).pow(2)
    }

    /// Whether an item of `size` fits inside the page's edge at all.
    pub(super) fn holds(&self, size: U16Vec2) -> bool {
        u32::from(size.max_element()) <= self.edge()
    }

    /// A rectangle of `size` on the page, which sits at `idx`, now taken; `None` where the page
    /// has no room.
    pub(super) fn allocate(&mut self, idx: usize, size: U16Vec2) -> Option<Placement> {
        let placement = Placement {
            page: u8::try_from(idx).expect("a list holds few pages"),
            origin: self.packer.allocate(size)?,
            size,
        };
        self.live_texels += placement.texels();
        Some(placement)
    }

    /// Give `placement`'s rectangle back.
    pub(super) fn deallocate(&mut self, placement: Placement) {
        self.packer.deallocate(placement.origin, placement.size.x);
        self.live_texels -= placement.texels();
    }
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::gpu::atlas::page_list::page::Page;

    impl Page {
        /// The texels its items fill.
        pub(crate) const fn live_texels(&self) -> u64 {
            self.live_texels
        }
    }
}
