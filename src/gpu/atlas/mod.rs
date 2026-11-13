//! The atlas pool: every page list's bookkeeping, its CPU half, with no device, so each rule is
//! testable without one, and [`atlas_textures`], its GPU half, the textures that follow it. The
//! decisions are `src/gpu/AGENTS.md`, Atlases.

pub(crate) mod atlas_format;
pub(crate) mod atlas_handle;
pub(crate) mod atlas_textures;
pub(crate) mod error;
mod eviction_clock;
pub(crate) mod page_group;
pub(crate) mod page_list;
pub(crate) mod page_slots;
pub(crate) mod row_table;
pub(crate) mod scratch_cache;
pub(crate) mod tenant;

use crate::gpu::atlas::atlas_format::AtlasFormat;
use crate::gpu::atlas::atlas_handle::{AtlasHandle, Placement};
use crate::gpu::atlas::error::AtlasError;
use crate::gpu::atlas::page_list::{MoveBudget, PageList, PageListConfig};
use crate::gpu::atlas::page_slots::AtlasSlots;
use crate::gpu::atlas::scratch_cache::ScratchId;
use crate::gpu::atlas::tenant::Tenant;
use glam::{U16Vec2, UVec2};
use std::vec::Drain;

/// Each list's constants, by [`AtlasFormat`]: soft budgets the sum of the budgets the tenants held
/// before the pool, page edges whose four pages are the cap one tenant held, until the bench hosts'
/// memory study tunes them (`.notes/research/2026-11-12-atlas-pool-measurements.md`).
pub(crate) const LIST_CONFIGS: [PageListConfig; 3] = [
    PageListConfig {
        first_edge: 256,
        page_edge: 2048,
        soft_bytes: 8 << 20,
    },
    PageListConfig {
        first_edge: 256,
        page_edge: 1024,
        soft_bytes: 8 << 20,
    },
    PageListConfig {
        first_edge: 256,
        page_edge: 1024,
        soft_bytes: 32 << 20,
    },
];

/// The handles evicted since each tenant last drained its queue. Each holds a handle at most once
/// a generation, so a queue is bounded by its list's slab.
#[derive(Debug, Default)]
pub(crate) struct Evictions {
    queues: [Vec<AtlasHandle>; Tenant::COUNT],
}

impl Evictions {
    fn push(&mut self, tenant: Tenant, handle: AtlasHandle) {
        self.queues[tenant as usize].push(handle);
    }
}

/// Every page list of one device, by [`AtlasFormat`], the display frame they age on, and the
/// evictions their tenants have yet to drop.
#[derive(Debug)]
pub(crate) struct AtlasPool {
    lists: [PageList; 3],
    frame: u64,
    /// The display frame the lists last decayed in, `None` before the first.
    decayed: Option<u64>,
    evictions: Evictions,
}

impl AtlasPool {
    /// The pool of a device whose largest texture edge is `device_edge`, at
    /// [`LIST_CONFIGS`].
    pub(crate) fn new(device_edge: u32) -> Self {
        Self::with_configs(LIST_CONFIGS, device_edge)
    }

    /// The pool with each list as `configs` sets it, by [`AtlasFormat`].
    pub(crate) fn with_configs(configs: [PageListConfig; 3], device_edge: u32) -> Self {
        Self {
            lists: AtlasFormat::ALL
                .map(|format| PageList::new(format, configs[format as usize], device_edge)),
            frame: 0,
            decayed: None,
            evictions: Evictions::default(),
        }
    }

    /// The list of `format`.
    pub(crate) const fn list(&self, format: AtlasFormat) -> &PageList {
        &self.lists[format as usize]
    }

    /// The open slots a frame's raster and shadow draws read now.
    pub(crate) fn slots(&self) -> AtlasSlots {
        AtlasSlots {
            mask: self.list(AtlasFormat::R8).slots(),
            color: self.list(AtlasFormat::Rgba8).slots(),
        }
    }

    /// Move to display frame `frame`, and drop the scratch textures idle for an expiry window.
    ///
    /// # Panics
    ///
    /// If `frame` is before the pool's.
    pub(crate) fn advance_to(&mut self, frame: u64) {
        assert!(
            frame >= self.frame,
            "the display frames ran backwards, {} to {frame}",
            self.frame
        );
        self.frame = frame;
        for list in &mut self.lists {
            list.expire(frame);
        }
    }

    /// Room for an item of `size` of `format` for `tenant`, drawn this frame; the items evicted
    /// for it wait in their tenants' queues.
    pub(crate) fn allocate(
        &mut self,
        format: AtlasFormat,
        tenant: Tenant,
        size: U16Vec2,
    ) -> Result<AtlasHandle, AtlasError> {
        self.lists[format as usize].allocate(tenant, size, self.frame, &mut self.evictions)
    }

    /// Return what the lists hold past their need, at the end of the first submit of each display
    /// frame, after every draw of it: the evictions wait in their tenants' queues, and the moves
    /// in their lists for the GPU half's next commit, ahead of every upload, bake and draw, so the
    /// next build of any window reads the new placements (`src/gpu/AGENTS.md`, Atlases). As
    /// WebRender runs its compaction once a frame. The moves spend `budget`, the GPU half's for
    /// its device ([`atlas_textures::MoveRoute::budget`]), across the lists.
    pub(crate) fn end_submit(&mut self, budget: MoveBudget) {
        if self.decayed == Some(self.frame) {
            return;
        }
        self.decayed = Some(self.frame);
        let mut budget = budget;
        for list in &mut self.lists {
            list.decay(self.frame, &mut budget, &mut self.evictions);
        }
    }

    /// Give back all the lists can at once, as the system runs short of memory: every item no
    /// one pins and this display frame did not draw, idle scratch, and pages a drain empties
    /// with no budget. Between frames alone, as the moves and the closed pages wait for the GPU
    /// half's next commit, which the caller records at once.
    pub(crate) fn purge(&mut self) {
        for list in &mut self.lists {
            list.purge(self.frame, &mut self.evictions);
        }
    }

    /// Forget every list's moves, which the GPU half's commit recorded.
    pub(crate) fn forget_moves(&mut self) {
        for list in &mut self.lists {
            list.forget_moves();
        }
    }

    /// Give `handle`'s item back.
    pub(crate) fn release(&mut self, handle: AtlasHandle) {
        self.lists[handle.format as usize].release(handle);
    }

    /// Where `handle`'s item sits, `None` once it went.
    pub(crate) fn placement(&self, handle: AtlasHandle) -> Option<Placement> {
        self.lists[handle.format as usize].placement(handle)
    }

    /// Mark `handle`'s item drawn this frame and say where it sits, `None` once it went.
    pub(crate) fn stamp(&mut self, handle: AtlasHandle) -> Option<Placement> {
        self.lists[handle.format as usize].stamp(handle, self.frame)
    }

    /// Keep `handle`'s item from every eviction until a matching [`Self::unpin`].
    pub(crate) fn pin(&mut self, handle: AtlasHandle) {
        self.lists[handle.format as usize].pin(handle);
    }

    /// Take back one pin of `handle`'s item.
    pub(crate) fn unpin(&mut self, handle: AtlasHandle) {
        self.lists[handle.format as usize].unpin(handle);
    }

    /// The handles of `tenant`'s items evicted since its last drain, each stale now: the tenant
    /// drops the keys it held them under.
    #[expect(
        clippy::iter_with_drain,
        reason = "the queue keeps its buffer for the next evictions, which `into_iter` would drop"
    )]
    pub(crate) fn drain_evicted(&mut self, tenant: Tenant) -> Drain<'_, AtlasHandle> {
        self.evictions.queues[tenant as usize].drain(..)
    }

    /// A texture of its own of `format` for an item of `size`.
    pub(crate) fn acquire_scratch(
        &mut self,
        format: AtlasFormat,
        size: UVec2,
    ) -> Result<ScratchId, AtlasError> {
        self.lists[format as usize].acquire_scratch(size)
    }

    /// Give back the texture of `format` `id` holds; it idles until it serves again or expires.
    pub(crate) fn release_scratch(&mut self, format: AtlasFormat, id: ScratchId) {
        self.lists[format as usize].release_scratch(id, self.frame);
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::gpu::atlas::AtlasPool;
    use crate::gpu::atlas::atlas_format::AtlasFormat;
    use crate::gpu::atlas::scratch_cache::ScratchId;
    use glam::UVec2;

    impl AtlasPool {
        /// The display frame the pool ages on.
        pub(crate) const fn frame(&self) -> u64 {
            self.frame
        }

        /// Close `format`'s page `idx`, which holds no item.
        pub(crate) fn close_page(&mut self, format: AtlasFormat, idx: usize) {
            self.lists[format as usize].close_page(idx);
        }

        /// The size of the texture of `format` `id` holds, `None` once released.
        pub(crate) fn scratch_size(&self, format: AtlasFormat, id: ScratchId) -> Option<UVec2> {
            self.lists[format as usize].scratch_size(id)
        }

        /// Whether the lists decayed in the current display frame.
        pub(crate) const fn has_decayed(&self) -> bool {
            matches!(self.decayed, Some(frame) if frame == self.frame)
        }
    }
}

#[cfg(test)]
mod tests;
