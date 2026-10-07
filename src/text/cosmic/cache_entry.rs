//! One entry in the shaped-buffer cache, and its two-tier retention state.

use crate::common::expiry_wheel::TicketSeq;
use crate::text::extent::TextExtent;
use crate::text::root::TextRoot;
use cosmic_text::Buffer;

#[derive(Debug)]
pub(super) struct CacheEntry {
    /// Shaped buffer, looked up by [`TextShapeKey`](crate::text::key::TextShapeKey).
    pub(super) buffer: Buffer,
    /// What this buffer measured to; see [`CachedExtent`].
    pub(super) extent: CachedExtent,
    /// x of the block's left edge in buffer space, subtracted by readers.
    /// Cosmic offsets non-left-aligned lines and any RTL run in a bounded
    /// buffer; measuring from 0 would count that gap in the width and the
    /// encoder would apply the offset twice. Zero for unbounded buffers.
    pub(super) left: f32,
    /// First frame on which this entry is dead: set one probation window out on
    /// insertion and a protected window out on each lookup, so the deadline is
    /// the tier. Matches [`ExpiryWheel::schedule`](crate::common::expiry_wheel::ExpiryWheel::schedule).
    pub(super) dies_at: u64,
    /// Serial of this entry's live expiry ticket. A ticket firing under another
    /// serial was superseded by
    /// [`ShapedBufferCache::supersede`](crate::text::cosmic::shaped_buffer_cache::ShapedBufferCache::supersede)
    /// and dies in the sweep, so demote/promote cycles don't accumulate tickets.
    pub(super) ticket_seq: TicketSeq,
}

/// What one cached buffer measured to: an **unbounded** buffer is a
/// [`TextRoot`], a **bounded** one only an extent and its ink. The kind follows
/// from the key; [`Self::root`] asserts that pairing.
#[derive(Clone, Copy, Debug)]
pub(super) enum CachedExtent {
    Root(TextRoot),
    Bounded(TextExtent),
}

impl CachedExtent {
    /// The shaped block and its ink.
    pub(super) const fn extent(self) -> TextExtent {
        match self {
            Self::Root(root) => root.extent,
            Self::Bounded(extent) => extent,
        }
    }

    pub(super) fn root(self) -> TextRoot {
        match self {
            Self::Root(root) => root,
            Self::Bounded(_) => panic!("{BOUNDED_AS_ROOT}"),
        }
    }

    pub(super) fn root_mut(&mut self) -> &mut TextRoot {
        match self {
            Self::Root(root) => root,
            Self::Bounded(_) => panic!("{BOUNDED_AS_ROOT}"),
        }
    }
}

const BOUNDED_AS_ROOT: &str = "a bounded shape has no wrapping floor and no line count of the run: \
     the key that reached this entry should have been the unbounded one";
