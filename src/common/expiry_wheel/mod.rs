//! Deadline index for age-bounded caches: which keys come due on which
//! frame, so a sweep costs what actually expires instead of what is
//! resident.
//!
//! # Why not a scan
//!
//! The obvious sweep walks every entry once a frame and drops the ones
//! past their deadline. Its cost is set by the *working set*, not by the
//! churn — a cache holding a scrolled document's few thousand shaped runs
//! pays for all of them every frame to reclaim the one label that changed.
//! Gating the scan behind "earliest deadline in the cache" only helps
//! while nothing is churning: one key that changes every frame pins that
//! minimum permanently and the gate stops firing, which is exactly the
//! workload the bound exists for.
//!
//! A wheel inverts the question. Tickets are filed under the frame they
//! come due, so a frame drains one bucket and touches nothing else.
//!
//! # Tickets are hints
//!
//! A ticket is never authority to delete. Deadlines move after a ticket
//! is filed — a cache hit pushes one out, a supersede pulls one in — and
//! rewriting the ticket in place would mean finding it, which is the scan
//! again. So the caller re-reads the entry's real deadline when its
//! ticket fires and either drops it or files a fresh one. That makes
//! every awkward case fall out for free: a stale ticket for an
//! already-removed key finds nothing, a clock that jumps several frames
//! drains several buckets, and a jump wider than the ring drains all of
//! them and re-files whatever is still live.
//!
//! What the caller owes in return, so nothing is silently forgotten:
//!
//! - **File on insert**, at the first frame the entry would be dead.
//! - **File again when a deadline moves *in*.** The outstanding ticket
//!   sits at the old, later frame; without a second one the entry
//!   outlives its shortened deadline.
//! - **Do nothing when a deadline moves *out*.** The outstanding ticket
//!   fires early, sees a live entry, and re-files. This is what keeps a
//!   run that is touched every frame from filing a ticket every frame.
//! - **Keep the serial of the live ticket, and let the rest die.**
//!   [`ExpiryWheel::schedule`] hands back a [`TicketSeq`] naming the
//!   ticket it just filed; the owner stamps its entry with it, and
//!   [`ExpiryWheel::retire`] reports the serial of every ticket that
//!   fires. One that is not the entry's stamp was supplanted by a later
//!   filing and must answer `None` rather than re-file.
//!
//! That last rule is what keeps the drain proportional to churn. A
//! deadline that moves in and back out — a resize drag demoting a run
//! that the next frame promotes again — files a ticket per cycle while
//! the one it supplanted is still outstanding. Re-file both and the
//! ticket count per entry grows without bound for as long as the entry
//! lives, which turns the per-frame drain into a function of uptime.
//! Stamping costs one `u32` on the entry and makes the surplus
//! self-retiring: each supplanted ticket fires once and is gone.
//!
//! A re-file from inside [`ExpiryWheel::retire`] keeps the serial it
//! fired under, so a serial names the whole chain of re-files of one live
//! ticket rather than a single filing. That is what lets an owner stamp
//! exactly where it decides something — at its own `schedule` call — and
//! never on the wheel's behalf.
//!
//! **Only some owners need the last two rules.** An owner whose deadlines
//! only ever move *out* never supplants a ticket, so it has nothing to
//! stamp and ignores the serial `retire` reports; both of the caches in
//! that position say so at their `retire` call. One that starts pulling a
//! deadline in — a probation tier for the encoded rows is the obvious
//! candidate, and its counters were built to size one — takes on the rule
//! with it.

use std::fmt::Debug;

/// Names one live ticket, so an owner can tell it from the ones later
/// filings have supplanted. Minted by [`ExpiryWheel::schedule`].
///
/// A serial, not the frame the ticket was filed for. The frame is not an
/// identity: the clamp can move a ticket the owner asked to file past the
/// ring, and a drain that aliased hands tickets back under some *other*
/// bucket's frame — so an entry stamped with a frame can stop
/// recognising its own ticket, and then nothing keeps it alive. A serial
/// is true in both cases, which is what makes those two mechanisms
/// invisible to owners rather than hazards they have to reason about.
///
/// 32 bits is not a wrap risk. Two serials are only ever compared while
/// both their tickets are outstanding, and a ticket outlives its filing
/// by at most one ring — so a collision would need 2³² filings inside a
/// window a couple of hundred frames wide.
pub(crate) type TicketSeq = u32;

/// One filed ticket: which key to revisit, under which filing.
#[derive(Clone, Copy, Debug)]
struct Ticket<K> {
    key: K,
    seq: TicketSeq,
}

/// Ring of pending expiry tickets keyed by due frame.
///
/// Sized at construction from the longest deadline the owner can hand
/// out; see [`Self::with_keep`].
#[derive(Debug)]
pub(crate) struct ExpiryWheel<K> {
    /// Ring indexed by `due & mask`. Buckets keep their capacity across
    /// drains, so a steady workload files and fires tickets without
    /// allocating.
    ///
    /// A boxed slice because the ring is sized once and never grows —
    /// and one `Vec` per bucket because that is what makes a drain a
    /// memcpy of a contiguous run. Threading the tickets through a flat
    /// arena instead would save the bucket headers and cost a pointer
    /// chase per ticket on the one path that has to be fast.
    ///
    /// The headers are worth about 10 KB across the four wheels in the
    /// crate — 128 buckets each for the shaped-buffer cache and the two
    /// raster atlases, 32 for the encoded-run cache, at 24 bytes a
    /// `Vec`. That is a number set by the owners' retention windows, not
    /// by this type: an owner that files deadlines a thousand frames out
    /// buys a thousand-bucket ring, so a window is a memory decision as
    /// well as a retention one.
    buckets: Box<[Vec<Ticket<K>>]>,
    mask: u64,
    /// Highest frame whose bucket has been drained. Tickets must be
    /// filed strictly after it, or they land in a bucket this cycle has
    /// already passed and would not fire for another full ring.
    drained_through: u64,
    /// Serial for the next filing. Wraps, which is safe — see
    /// [`TicketSeq`].
    next_seq: TicketSeq,
    /// Retained landing area for the tickets [`Self::retire`] is walking.
    ///
    /// Lives here rather than on each owner because it is part of this
    /// type's protocol, not theirs: it exists only so the ring can be
    /// re-filed while its own drained tickets are being walked.
    scratch: Vec<Ticket<K>>,
}

impl<K: Copy + Debug> ExpiryWheel<K> {
    /// A wheel for an owner that keeps an entry `keep_frames` frames
    /// past its last use — its *longest* window, where the owner has
    /// more than one.
    ///
    /// The ring is sized two frames past that window, not one. The
    /// horizon counts from the most recently **drained** frame, and the
    /// two differ for an owner that schedules during a frame and sweeps
    /// at its end, which is every owner here: such an owner files
    /// against a `drained_through` still one frame behind. Getting it
    /// wrong is not a correctness bug — a deadline past the ring is
    /// clamped inward and fires early, and an early ticket costs one
    /// re-file — but it silently turns a cache that should file one
    /// ticket per row per window into one that re-files every row every
    /// window. Which is why the arithmetic is here rather than at each
    /// owner's constructor.
    pub(crate) fn with_keep(keep_frames: u64) -> Self {
        Self::with_horizon(keep_frames + 2)
    }

    /// A wheel that can hold a ticket up to `horizon` frames past the
    /// most recently **drained** frame.
    ///
    /// Rounded up to a power of two so the bucket index is a mask rather
    /// than a division — this runs per filed ticket. One spare slot
    /// beyond the horizon keeps the furthest ticket from aliasing the
    /// bucket being drained.
    fn with_horizon(horizon: u64) -> Self {
        let slots = (horizon + 1).next_power_of_two() as usize;
        Self {
            buckets: (0..slots).map(|_| Vec::new()).collect(),
            mask: slots as u64 - 1,
            drained_through: 0,
            next_seq: 0,
            scratch: Vec::new(),
        }
    }

    /// File a ticket to revisit `key` at frame `due` — the first frame on
    /// which the entry is dead, not the last on which it lives, so the
    /// caller's own `deadline < frame` test decides and the wheel holds
    /// no policy.
    ///
    /// A `due` outside the ring is clamped into it rather than rejected.
    /// Both ends are safe precisely because a ticket is a hint: firing
    /// early costs one re-file, while firing *late* is the one outcome
    /// the wheel must never produce — and a ticket filed more than a
    /// full ring out would alias the bucket just drained and do exactly
    /// that. Owners whose sweep can fall behind the clock (a window that
    /// stops submitting, a test driving frames by hand) would otherwise
    /// have to reason about the ring width themselves.
    ///
    /// **If `due` is earlier than the entry's outstanding ticket, this
    /// supplants that ticket rather than replacing it** — the wheel
    /// cannot reach into a bucket to remove one, so both are now live.
    /// Stamp the entry with the returned [`TicketSeq`] and let
    /// [`Self::retire`] drop whatever fires under an older one, or the
    /// pair re-files itself for as long as the entry lives.
    pub(crate) fn schedule(&mut self, key: K, due: u64) -> TicketSeq {
        debug_assert!(
            due > self.drained_through,
            "ticket for {key:?} at {due} is not past the drained frame {}",
            self.drained_through,
        );
        let seq = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(1);
        self.file(Ticket { key, seq }, due);
        seq
    }

    /// Put a ticket in its bucket, clamped into the ring.
    ///
    /// Split from [`Self::schedule`] because a re-file keeps the serial
    /// it fired under rather than minting one — see the module doc.
    fn file(&mut self, ticket: Ticket<K>, due: u64) {
        let due = due.clamp(self.drained_through + 1, self.drained_through + self.mask);
        self.buckets[(due & self.mask) as usize].push(ticket);
    }

    /// Hand every ticket due through `frame` to `settle`, re-filing each
    /// at the deadline it returns and forgetting the ones it answers
    /// `None` for.
    ///
    /// This is the whole owner-side protocol in one call: a ticket is a
    /// hint, so `settle` re-reads the entry's real deadline and says
    /// either "still live, come back at N" or "gone". `settle`'s second
    /// argument is the ticket's [`TicketSeq`], which an owner that stamps
    /// compares against its entry to tell its live ticket from one a
    /// later [`Self::schedule`] supplanted; owners whose deadlines only
    /// move out ignore it.
    ///
    /// `settle` may freely mutate the owner's other fields: the wheel
    /// borrows only itself, so disjoint field captures let the closure
    /// hold the map it is retiring from.
    pub(crate) fn retire(
        &mut self,
        frame: u64,
        mut settle: impl FnMut(K, TicketSeq) -> Option<u64>,
    ) {
        if frame <= self.drained_through {
            return;
        }
        // Out and back so the ring stays free to be re-filed below;
        // capacity is retained across the swap.
        let mut due = std::mem::take(&mut self.scratch);

        // A clock that jumped further than the ring is wide has aliased
        // every bucket, so every bucket is due. Draining a ticket early
        // costs one re-file, never a wrong drop.
        let slots = self.buckets.len() as u64;
        let first = if frame - self.drained_through >= slots {
            frame + 1 - slots
        } else {
            self.drained_through + 1
        };
        // `drained_through` moves first: the walk below re-files, and
        // those tickets are all past `frame`.
        self.drained_through = frame;
        // Every due bucket is emptied before the first re-file, because a
        // re-filed ticket can land in one this drain has not reached yet
        // and walking bucket-by-bucket would fire it again immediately.
        for f in first..=frame {
            due.append(&mut self.buckets[(f & self.mask) as usize]);
        }

        for ticket in due.drain(..) {
            if let Some(next) = settle(ticket.key, ticket.seq) {
                self.file(ticket, next);
            }
        }
        self.scratch = due;
    }

    /// Drop every outstanding ticket, keeping bucket capacity. For an
    /// owner that just cleared its whole map — every ticket would be
    /// stale, and re-filing from an empty map yields nothing.
    ///
    /// Registering a font is what empties a cache wholesale in
    /// production: both text caches are keyed on a resolution the new
    /// face can change.
    pub(crate) fn clear(&mut self) {
        for bucket in &mut self.buckets {
            bucket.clear();
        }
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    impl<K: Copy + Debug> ExpiryWheel<K> {
        /// Outstanding tickets across the whole ring.
        ///
        ///
        /// The number that says whether an owner is holding up its end
        /// of the protocol: file on insert, file again only when a
        /// deadline moves *in*, and let a supplanted ticket die rather
        /// than re-file. An owner that re-filed on every touch, or that
        /// re-files duplicates a `supersede` has replaced, would still
        /// expire correctly — just with the ticket count, and the
        /// per-frame drain, growing without bound. `EncodedCache`'s and
        /// `CosmicMeasure`'s tests assert against exactly that.
        pub(crate) fn pending(&self) -> usize {
            self.buckets.iter().map(Vec::len).sum()
        }
    }
}

#[cfg(test)]
mod tests;
