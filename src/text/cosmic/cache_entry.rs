//! One entry in the shaped-buffer cache, and the two-tier retention
//! state it carries.
//!
//! The four operations that move an entry between the windows — insert
//! files a probationary ticket, a lookup promotes, a supersede demotes,
//! the end-of-frame sweep settles whatever came due — are methods on
//! [`CosmicMeasure`](crate::text::cosmic::CosmicMeasure), together and
//! beside every other reader of the cache, because they are one
//! protocol: the
//! [`ExpiryWheel`](crate::common::expiry_wheel::ExpiryWheel) contract in
//! [`crate::common::expiry_wheel`] is only upheld by all four agreeing,
//! and reading any one of them alone is how the ticket-leak regression
//! got written.

use crate::common::expiry_wheel::TicketSeq;
use crate::text::key::TextShapeKey;
use crate::text::root::TextRoot;
use cosmic_text::Buffer;
use rustc_hash::FxHashMap;

#[derive(Debug)]
pub(super) struct CacheEntry {
    /// Shaped buffer. Looked up by [`TextShapeKey`] at render time so the
    /// text backend can build a `TextArea` without reshaping.
    pub(super) buffer: Buffer,
    /// What this buffer measured to. A bounded entry's floor is `None`
    /// and its single-line flag describes the resolve rather than the
    /// run; both are inert, since only the unbounded root's copy is ever
    /// read back.
    pub(super) root: TextRoot,
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
    /// Last frame on which this entry is kept; [`CosmicMeasure::end_frame`]
    /// drops it once the clock passes this. Insertion sets it one
    /// probation window out and every lookup pushes it a protected window
    /// out, so the two-tier policy needs no separate "has been reused"
    /// flag — the deadline *is* the tier.
    pub(super) keep_until: u64,
    /// Serial of this entry's live expiry ticket. A ticket firing under
    /// any other one was supplanted by a later
    /// [`CosmicMeasure::supersede`] and dies in the sweep instead of
    /// re-filing itself — without which a run that is demoted and
    /// promoted each frame accumulates one permanent ticket per cycle.
    pub(super) ticket_seq: TicketSeq,
}

impl CacheEntry {
    /// The cached unbounded shape a truncating fit cuts from.
    ///
    /// [`CosmicMeasure::measure_truncated`] calls
    /// [`CosmicMeasure::ensure_buffer`] on this key before reaching for it,
    /// and re-reads it once per back-off round because the shaping in
    /// between needs `&mut self`, so the borrow cannot be held across the
    /// loop.
    ///
    /// Hands back the whole entry rather than its buffer: the caller wants
    /// the measured [`TextRoot`] as well as the glyphs, and both come out of
    /// the one lookup.
    ///
    /// Takes the map rather than `&self` on purpose: the caller holds
    /// `&mut self.logical_order` at the same time, and only a borrow of the
    /// one field stays disjoint from it.
    #[inline]
    pub(super) fn probe(cache: &FxHashMap<TextShapeKey, Self>, key: TextShapeKey) -> &Self {
        cache
            .get(&key)
            .expect("truncation requires the cached unbounded shape")
    }
}
