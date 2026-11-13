//! What a backdrop's composite binds, which the backdrops make after the pool's commit and an
//! effect that reads its backdrop binds: the texture each result lies in, and a group over each.

use crate::gpu::atlas::page_list::MAX_PAGES;
use crate::gpu::atlas::scratch_cache::ScratchId;

/// The texture of the pool's half-float list a backdrop's result lives in: a page it shares, or a
/// scratch texture of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ResultTexture {
    Page(u8),
    Scratch(ScratchId),
}

/// The groups a backdrop's composite reads its result through, one over each texture a result can
/// lie in: the stand-in, for a composite that shows its clear; each open page; and each scratch slot
/// a result has read, by slot. A texture changes only with the list's generation, which makes them
/// all again, so the scratch groups are as many as the slots, however often a result takes one
/// again.
#[derive(Debug)]
pub(crate) struct CompositeGroups {
    pub(crate) absent: wgpu::BindGroup,
    pub(crate) pages: [Option<wgpu::BindGroup>; MAX_PAGES],
    pub(crate) scratch: Vec<Option<wgpu::BindGroup>>,
    /// What the groups bind, which a moved view or a grown slot table changes.
    pub(crate) bound: CompositeBound,
}

/// The generation of the list's views and the `slot_buffer` capacity composite groups were made
/// over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CompositeBound {
    pub(crate) generation: u64,
    pub(crate) capacity: usize,
}

/// The groups a frame's composites read through, by the backdrop each draws.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BackdropGroups<'a> {
    pub(crate) groups: &'a CompositeGroups,
    pub(crate) slot_reads: &'a [Option<ResultTexture>],
}

impl BackdropGroups<'_> {
    /// The texture backdrop `idx`'s composite reads, `None` for one that shows its clear: a draw
    /// splits where it changes.
    pub(crate) const fn read_of(&self, idx: u32) -> Option<ResultTexture> {
        self.slot_reads[idx as usize]
    }

    /// The group a composite that reads `read` binds.
    pub(crate) fn group(&self, read: Option<ResultTexture>) -> &wgpu::BindGroup {
        let made = "a result's group was made at the follow";
        match read {
            None => &self.groups.absent,
            Some(ResultTexture::Page(page)) => {
                self.groups.pages[usize::from(page)].as_ref().expect(made)
            }
            Some(ResultTexture::Scratch(id)) => {
                self.groups.scratch[id.idx as usize].as_ref().expect(made)
            }
        }
    }
}
