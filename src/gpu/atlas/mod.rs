//! The atlas pool: every page list's bookkeeping, its CPU half, with no device, so each rule is
//! testable without one, and [`atlas_textures`], its GPU half, the textures that follow it. The
//! design is `.notes/proposals/atlas-pool.md`.

pub(crate) mod atlas_format;
pub(crate) mod atlas_handle;
pub(crate) mod atlas_textures;
pub(crate) mod error;
mod eviction_clock;
pub(crate) mod page_group;
pub(crate) mod page_list;
pub(crate) mod row_table;
pub(crate) mod scratch_cache;
pub(crate) mod tenant;

use crate::gpu::atlas::atlas_format::AtlasFormat;
use crate::gpu::atlas::atlas_handle::{AtlasHandle, Placement};
use crate::gpu::atlas::error::AtlasError;
use crate::gpu::atlas::page_list::{DecayCtx, Move, MoveBudget, PageList, PageListConfig};
use crate::gpu::atlas::scratch_cache::ScratchId;
use crate::gpu::atlas::tenant::Tenant;
use glam::{U16Vec2, UVec2};
use std::vec::Drain;

/// Each list's starting constants, by [`AtlasFormat`] (`.notes/proposals/atlas-pool.md`, Starting
/// constants): soft budgets the sum of today's tenants', page edges whose four pages are one
/// tenant's cap today.
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
    /// The display frame the lists last decayed in.
    decayed: u64,
    evictions: Evictions,
    /// Each list's drains' moves since the GPU half last took them, by [`AtlasFormat`].
    moves: [Vec<Move>; 3],
}

/// One list and the moves its drains made, as the GPU half's commit takes them.
#[derive(Debug)]
pub(crate) struct ListCommit<'a> {
    pub(crate) list: &'a PageList,
    pub(crate) moves: Drain<'a, Move>,
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
            decayed: 0,
            evictions: Evictions::default(),
            moves: Default::default(),
        }
    }

    /// The list of `format`.
    pub(crate) const fn list(&self, format: AtlasFormat) -> &PageList {
        &self.lists[format as usize]
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
    /// for the GPU half's next commit, ahead of every upload, bake and draw, so the next build of
    /// any window reads the new placements (`.notes/proposals/atlas-pool.md`, Decay). As WebRender
    /// runs its compaction once a frame.
    pub(crate) fn end_submit(&mut self) {
        if self.decayed == self.frame {
            return;
        }
        self.decayed = self.frame;
        let mut budget = MoveBudget::FRAME;
        for (list, moves) in self.lists.iter_mut().zip(&mut self.moves) {
            list.decay(
                self.frame,
                DecayCtx {
                    budget: &mut budget,
                    evictions: &mut self.evictions,
                    moves,
                },
            );
        }
    }

    /// Give back all the lists can at once, as the system runs short of memory: every item no
    /// one pins and this display frame did not draw, idle scratch, and pages a drain empties
    /// with no budget. Between frames alone, as the moves and the closed pages wait for the GPU
    /// half's next commit, which the caller records at once.
    pub(crate) fn purge(&mut self) {
        for (list, moves) in self.lists.iter_mut().zip(&mut self.moves) {
            list.purge(self.frame, &mut self.evictions, moves);
        }
    }

    /// `format`'s list and the moves its drains made since the GPU half last took them, which
    /// it takes now.
    #[expect(
        clippy::iter_with_drain,
        reason = "the pool keeps its buffer for the next moves, which `into_iter` would drop"
    )]
    pub(crate) fn commit_list(&mut self, format: AtlasFormat) -> ListCommit<'_> {
        ListCommit {
            list: &self.lists[format as usize],
            moves: self.moves[format as usize].drain(..),
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
            self.decayed == self.frame
        }
    }
}

#[cfg(test)]
mod tests;
