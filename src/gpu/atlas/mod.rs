//! The atlas pool: every page list's bookkeeping, its CPU half, with no device, so each rule is
//! testable without one, and [`atlas_textures`], its GPU half, the textures that follow it. The
//! decisions are `src/gpu/AGENTS.md`, Atlases.

pub(crate) mod atlas_format;
pub(crate) mod atlas_handle;
pub(crate) mod atlas_textures;
pub(crate) mod error;
pub(crate) mod page_group;
pub(crate) mod page_list;
pub(crate) mod page_slots;
pub(crate) mod row_texture;
pub(crate) mod scratch_cache;
pub(crate) mod tenant;

use crate::gpu::atlas::atlas_format::{AtlasFormat, PerFormat};
use crate::gpu::atlas::atlas_handle::{Allocation, AtlasHandle, Placement};
use crate::gpu::atlas::error::AtlasError;
use crate::gpu::atlas::page_list::{MoveBudget, PageList, PageListConfig};
use crate::gpu::atlas::scratch_cache::ScratchId;
use crate::gpu::atlas::tenant::Tenant;
use crate::gpu::device::pass_traffic::PassTraffic;
use glam::UVec2;
use std::vec::Drain;

/// What a pool is made with: each list's constants, the device's largest texture edge, and what a
/// display frame's moves may take on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PoolConfig {
    pub(crate) lists: PerFormat<PageListConfig>,
    pub(crate) device_edge: u32,
    pub(crate) move_budget: MoveBudget,
}

impl PoolConfig {
    /// A device's: each list's starting constants ([`PageListConfig::of`]), its largest texture
    /// edge `device_edge`, and the budget of a device whose passes move `traffic`.
    pub(crate) fn of_device(device_edge: u32, traffic: PassTraffic) -> Self {
        Self {
            lists: PerFormat::from_fn(PageListConfig::of),
            device_edge,
            move_budget: MoveBudget::of(traffic),
        }
    }
}

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

/// Every page list of one device, by [`AtlasFormat`], the display frame they age on, what a frame's
/// moves may take, and the evictions their tenants have yet to drop.
#[derive(Debug)]
pub(crate) struct AtlasPool {
    lists: PerFormat<PageList>,
    frame: u64,
    /// The display frame the lists last decayed in, `None` before the first.
    decayed: Option<u64>,
    move_budget: MoveBudget,
    evictions: Evictions,
}

impl AtlasPool {
    /// The pool `config` sets.
    pub(crate) fn new(config: PoolConfig) -> Self {
        Self {
            lists: PerFormat::from_fn(|format| {
                PageList::new(format, config.lists[format], config.device_edge)
            }),
            frame: 0,
            decayed: None,
            move_budget: config.move_budget,
            evictions: Evictions::default(),
        }
    }

    /// The list of `format`.
    pub(crate) fn list(&self, format: AtlasFormat) -> &PageList {
        &self.lists[format]
    }

    /// Move to display frame `frame`.
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
    }

    /// Room for an item of `size` of `format` for `tenant`, drawn this frame, and where it sits;
    /// the items evicted for it wait in their tenants' queues.
    pub(crate) fn allocate(
        &mut self,
        format: AtlasFormat,
        tenant: Tenant,
        size: UVec2,
    ) -> Result<Allocation, AtlasError> {
        self.lists[format].allocate(tenant, size, self.frame, &mut self.evictions)
    }

    /// Return what the lists hold past their need, at the end of the first submit of each display
    /// frame, after every draw of it: the evictions wait in their tenants' queues, and the moves
    /// in their lists for the GPU half's next commit, ahead of every upload, bake and draw, so the
    /// next build of any window reads the new placements (`src/gpu/AGENTS.md`, Atlases). As
    /// WebRender runs its compaction once a frame. The moves spend the device's budget across the
    /// lists.
    pub(crate) fn end_submit(&mut self) {
        if self.decayed == Some(self.frame) {
            return;
        }
        self.decayed = Some(self.frame);
        let mut budget = self.move_budget;
        // A frame's first list a frame later each frame, as WebRender's compaction takes one list a
        // call round robin, so on a copy device's 16 moves a draining mask list holds no other's.
        let first = (self.frame % AtlasFormat::COUNT as u64) as usize;
        for at in 0..AtlasFormat::COUNT {
            let format = AtlasFormat::ALL[(first + at) % AtlasFormat::COUNT];
            self.lists[format].decay(self.frame, &mut budget, &mut self.evictions);
        }
    }

    /// Give back all the lists can at once, as the system runs short of memory: every item no
    /// one pins and this display frame did not draw, idle scratch, and pages a drain empties
    /// with no budget. Between frames alone, as the moves and the closed pages wait for the GPU
    /// half's next commit, which the caller records at once. Whether moves wait, after which the
    /// caller commits and purges again, as a drain that follows them could take the page they land
    /// on; at most [`MAX_PAGES`](page_list::MAX_PAGES) and one more rounds.
    pub(crate) fn purge(&mut self) -> bool {
        let mut more = false;
        for (_, list) in self.lists.iter_mut() {
            more |= list.purge(self.frame, &mut self.evictions);
        }
        more
    }

    /// Forget every list's moves, which the GPU half's commit recorded.
    pub(crate) fn forget_moves(&mut self) {
        for (_, list) in self.lists.iter_mut() {
            list.forget_moves();
        }
    }

    /// Give `handle`'s item back; nothing where it went already.
    pub(crate) fn release(&mut self, handle: AtlasHandle) {
        self.lists[handle.format].release(handle);
    }

    /// Where `handle`'s item sits, `None` once it went.
    pub(crate) fn placement(&self, handle: AtlasHandle) -> Option<Placement> {
        self.lists[handle.format].placement(handle)
    }

    /// Mark `handle`'s item drawn this frame and say where it sits, `None` once it went.
    pub(crate) fn stamp(&mut self, handle: AtlasHandle) -> Option<Placement> {
        self.lists[handle.format].stamp(handle, self.frame)
    }

    /// Keep `handle`'s item from every eviction until a matching [`Self::unpin`], and say where it
    /// sits; `None`, and no pin, once it went.
    pub(crate) fn pin(&mut self, handle: AtlasHandle) -> Option<Placement> {
        self.lists[handle.format].pin(handle)
    }

    /// Take back one pin of `handle`'s item, which counts as drawn this frame.
    pub(crate) fn unpin(&mut self, handle: AtlasHandle) {
        self.lists[handle.format].unpin(handle, self.frame);
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
        self.lists[format].scratch_mut().acquire(size)
    }

    /// Give back the texture `id` holds; it idles until it serves again or expires.
    pub(crate) fn release_scratch(&mut self, id: ScratchId) {
        let frame = self.frame;
        self.lists[id.format].scratch_mut().release(id, frame);
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::gpu::atlas::atlas_format::{AtlasFormat, PerFormat};
    use crate::gpu::atlas::page_list::{MoveBudget, PageListConfig};
    use crate::gpu::atlas::scratch_cache::ScratchId;
    use crate::gpu::atlas::{AtlasPool, PoolConfig};
    use crate::gpu::device::pass_traffic::PassTraffic;
    use glam::UVec2;

    impl AtlasPool {
        /// The pool of a device whose largest texture edge is `device_edge` at the starting
        /// constants, moving by copy commands.
        pub(crate) fn of_edge(device_edge: u32) -> Self {
            Self::new(PoolConfig::of_device(device_edge, PassTraffic::WholeTarget))
        }

        /// The pool whose lists all open at `first_edge`, double to `page_edge` and evict past
        /// `soft_bytes`, on a device of 16384, moving by copy commands.
        pub(crate) fn uniform(first_edge: u32, page_edge: u32, soft_bytes: u64) -> Self {
            Self::new(PoolConfig {
                lists: PerFormat::from_fn(|_| PageListConfig {
                    first_edge,
                    page_edge,
                    soft_bytes,
                }),
                device_edge: 16384,
                move_budget: MoveBudget::COPIES,
            })
        }

        /// The display frame the pool ages on.
        pub(crate) const fn frame(&self) -> u64 {
            self.frame
        }

        /// Close `format`'s page `idx`, which holds no item.
        pub(crate) fn close_page(&mut self, format: AtlasFormat, idx: usize) {
            self.lists[format].close_page(idx);
        }

        /// The size of the texture `id` holds, `None` once released.
        pub(crate) fn scratch_size(&self, id: ScratchId) -> Option<UVec2> {
            self.lists[id.format].scratch().size(id)
        }

        /// Whether the lists decayed in the current display frame.
        pub(crate) const fn has_decayed(&self) -> bool {
            matches!(self.decayed, Some(frame) if frame == self.frame)
        }
    }
}

#[cfg(test)]
mod tests;
