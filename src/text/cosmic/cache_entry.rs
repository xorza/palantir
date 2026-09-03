//! One entry in the shaped-buffer cache, and the two-tier retention
//! state it carries.
//!
//! The operations that move an entry between the windows all live on
//! [`ShapedBufferCache`](crate::text::cosmic::shaped_buffer_cache::ShapedBufferCache),
//! which is where its module doc says why they belong together.

use crate::common::expiry_wheel::TicketSeq;
use crate::primitives::size::Size;
use crate::text::root::TextRoot;
use cosmic_text::Buffer;

#[derive(Debug)]
pub(super) struct CacheEntry {
    /// Shaped buffer. Looked up by [`TextShapeKey`](crate::text::key::TextShapeKey)
    /// at render time so the
    /// text backend can build a `TextArea` without reshaping.
    pub(super) buffer: Buffer,
    /// What this buffer measured to, in whichever of the two kinds its
    /// key names — see [`CachedExtent`].
    pub(super) extent: CachedExtent,
    /// x of the block's left edge in buffer space — what every reader
    /// subtracts to put the block's own origin at 0.
    ///
    /// Cosmic does not lay every line out from 0. A non-left per-line
    /// align shifts each line right by `(line_width - line_w) * factor`,
    /// and *any* RTL run in a width-bounded buffer starts at `line_width`
    /// and advances leftward (`shape.rs`'s `start_x`), alignment or not.
    /// Measuring from 0 would then count that gap as part of the run's
    /// own width, and the encoder — which aligns the measured block
    /// inside the leaf rect — would apply the offset a second time.
    ///
    /// Zero for every unbounded buffer, so only a wrapped, width-bounded
    /// run can carry a non-zero value.
    pub(super) left: f32,
    /// First frame on which this entry is dead;
    /// [`CosmicMeasure::tick_frame`](crate::text::cosmic::CosmicMeasure::tick_frame)
    /// drops it once the clock reaches this. Insertion sets it one
    /// probation window out and every lookup pushes it a protected window
    /// out, so the two-tier policy needs no separate "has been reused"
    /// flag — the deadline *is* the tier.
    ///
    /// The first dead frame rather than the last live one, because that
    /// is what
    /// [`ExpiryWheel::schedule`](crate::common::expiry_wheel::ExpiryWheel::schedule)
    /// files under: the two vocabularies differ by one, and holding the
    /// wheel's own spares every site here from converting.
    pub(super) dies_at: u64,
    /// Serial of this entry's live expiry ticket. A ticket firing under
    /// any other one was supplanted by a later
    /// [`ShapedBufferCache::supersede`](crate::text::cosmic::shaped_buffer_cache::ShapedBufferCache::supersede)
    /// and dies in the sweep instead of
    /// re-filing itself — without which a run that is demoted and
    /// promoted each frame accumulates one permanent ticket per cycle.
    pub(super) ticket_seq: TicketSeq,
}

/// What one cached buffer measured to.
///
/// The two kinds of shape answer different questions, and keeping them
/// apart is what stops a reader taking one for the other. An **unbounded**
/// buffer is a run's [`TextRoot`]: an extent plus the wrapping floor and
/// the single-line flag every wrap policy reasons from. A **bounded** one
/// answers an extent and nothing else — it never scanned for a floor, and
/// its line count describes the resolve rather than the run. Storing the
/// distinction rather than a `TextRoot` with two inert fields is what lets
/// every reader take what it needs without knowing by convention which
/// half of the value applies to it.
///
/// Which kind an entry is follows from its key alone: a bounded key names
/// a bounded shape. [`Self::root`] asserts that pairing rather than
/// answering for a mismatch.
#[derive(Clone, Copy, Debug)]
pub(super) enum CachedExtent {
    Root(TextRoot),
    Bounded(Size),
}

impl CachedExtent {
    /// Extent of the shaped block — the one answer both kinds have.
    pub(super) fn size(self) -> Size {
        match self {
            Self::Root(root) => root.size,
            Self::Bounded(size) => size,
        }
    }

    /// The run's unbounded root. Reached only through an unbounded key,
    /// so a bounded entry here is a wiring bug rather than a case to
    /// answer with a floorless stand-in.
    pub(super) fn root(self) -> TextRoot {
        match self {
            Self::Root(root) => root,
            Self::Bounded(_) => panic!("{BOUNDED_AS_ROOT}"),
        }
    }

    /// The root, mutably — for the one writer there is, the wrap-floor
    /// backfill on a resident entry shaped without the scan.
    pub(super) fn root_mut(&mut self) -> &mut TextRoot {
        match self {
            Self::Root(root) => root,
            Self::Bounded(_) => panic!("{BOUNDED_AS_ROOT}"),
        }
    }
}

/// What reading a bounded entry as a root means, stated once for the two
/// accessors that can meet it.
const BOUNDED_AS_ROOT: &str = "a bounded shape has no wrapping floor and no line count of the run: \
     the key that reached this entry should have been the unbounded one";
