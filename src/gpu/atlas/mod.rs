//! The atlas pool: every page list's bookkeeping, its CPU half, with no device, so each rule is
//! testable without one, and [`atlas_textures`], its GPU half, the textures that follow it. The
//! design and the plan are `.notes/proposals/atlas-pool.md`; the tenants move onto it in steps 3
//! to 6.

#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the pool's tenants move onto it in steps 3 to 6 of `.notes/proposals/atlas-pool.md`; \
                  until then its tests alone reach it"
    )
)]

pub(crate) mod atlas_format;
pub(crate) mod atlas_handle;
pub(crate) mod atlas_textures;
pub(crate) mod error;
mod eviction_clock;
pub(crate) mod page_group;
pub(crate) mod page_list;
pub(crate) mod scratch_cache;
pub(crate) mod tenant;

use crate::gpu::atlas::atlas_format::AtlasFormat;
use crate::gpu::atlas::atlas_handle::{AtlasHandle, Placement};
use crate::gpu::atlas::error::AtlasError;
use crate::gpu::atlas::page_list::{PageList, PageListConfig};
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
            evictions: Evictions::default(),
        }
    }

    /// The list of `format`.
    pub(crate) const fn list(&self, format: AtlasFormat) -> &PageList {
        &self.lists[format as usize]
    }

    /// The display frame the pool ages on.
    pub(crate) const fn frame(&self) -> u64 {
        self.frame
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

    /// Close `format`'s page `idx`, which holds no item.
    pub(crate) fn close_page(&mut self, format: AtlasFormat, idx: usize) {
        self.lists[format as usize].close_page(idx);
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

    /// The size of the texture of `format` `id` holds, `None` once released.
    pub(crate) fn scratch_size(&self, format: AtlasFormat, id: ScratchId) -> Option<UVec2> {
        self.lists[format as usize].scratch_size(id)
    }
}

#[cfg(test)]
mod tests;
