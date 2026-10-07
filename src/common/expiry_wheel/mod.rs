//! Deadline index for age-bounded caches: which keys come due on which frame, so a
//! sweep costs what expires, not what is resident. A per-frame scan costs the
//! working set, and gating it behind the earliest deadline fails when one key
//! changes every frame. A wheel files tickets under their due frame, so a frame
//! drains one bucket and touches nothing else.
//!
//! # Tickets are hints
//!
//! A ticket is never authority to delete. Deadlines move after filing (a hit pushes
//! one out, a supersede pulls one in), and rewriting in place means finding the
//! ticket, which is the scan again. When a ticket fires the caller re-reads the
//! entry's real deadline and drops it or files a fresh ticket. Stale tickets find
//! nothing; a clock jump wider than the ring drains all buckets and re-files what
//! is live.
//!
//! The caller owes:
//!
//! - **File on insert**, at the first frame the entry would be dead.
//! - **File again when a deadline moves in.** The outstanding ticket sits at the
//!   old, later frame.
//! - **Nothing when a deadline moves out.** The ticket fires early, sees a live
//!   entry and re-files.
//! - **Keep the serial of the live ticket, and let the rest die.**
//!   [`ExpiryWheel::schedule`] returns a [`TicketSeq`] the owner stamps on its
//!   entry; [`ExpiryWheel::retire`] reports the serial of every firing ticket. One
//!   that isn't the entry's stamp was supplanted and must answer `None`, not re-file.
//!
//! The last rule keeps the drain proportional to churn: a deadline that moves in
//! and back out files a ticket per cycle, and re-filing the supplanted ones would
//! grow the per-entry ticket count with uptime. A re-file inside
//! [`ExpiryWheel::retire`] keeps its serial, so a serial names a whole chain of
//! re-files and an owner stamps only at its own `schedule` call. Owners whose
//! deadlines only move out may ignore the serial.

use std::fmt::Debug;
use std::mem;

/// Names one live ticket, so an owner can tell it from ones later filings
/// supplanted. A serial, not the due frame: the clamp can move a ticket past the
/// ring and an aliased drain returns tickets under another bucket's frame, so a
/// frame-stamped entry could lose its ticket and nothing would keep it alive.
/// 32 bits cannot wrap in practice: serials compare only while both tickets are
/// outstanding, at most one ring apart.
pub(crate) type TicketSeq = u32;

#[derive(Clone, Copy, Debug)]
struct Ticket<K> {
    key: K,
    seq: TicketSeq,
}

/// Ring of pending expiry tickets keyed by due frame, sized from the longest
/// deadline the owner can hand out ([`Self::with_keep`]).
#[derive(Debug)]
pub(crate) struct ExpiryWheel<K> {
    /// Ring indexed by `due & mask`. Buckets keep capacity across drains, so a steady
    /// workload doesn't allocate. One `Vec` per bucket makes a drain a memcpy; a flat
    /// arena would cost a pointer chase per ticket on the hot path.
    buckets: Box<[Vec<Ticket<K>>]>,
    mask: u64,
    /// Highest frame whose bucket has been drained. Tickets must be filed strictly
    /// after it, or they wait a full ring.
    drained_through: u64,
    next_seq: TicketSeq,
    /// Landing area for the tickets [`Self::retire`] walks, so the ring can re-file meanwhile.
    scratch: Vec<Ticket<K>>,
}

impl<K: Copy + Debug> ExpiryWheel<K> {
    /// A wheel for an owner that keeps an entry `keep_frames` past its last use (its
    /// longest window, if several). The ring is sized two frames past it, since an
    /// owner that files during a frame and sweeps at its end files against a
    /// `drained_through` one frame behind. Too small is not a correctness bug (the
    /// deadline clamps inward and fires early) but turns one ticket per row per window
    /// into a re-file of every row every window.
    pub(crate) fn with_keep(keep_frames: u64) -> Self {
        Self::with_horizon(keep_frames + 2)
    }

    /// One spare slot beyond the horizon, rounded up to a power of two.
    const fn slots_for_horizon(horizon: u64) -> u64 {
        (horizon + 1).next_power_of_two()
    }

    /// A wheel holding a ticket up to `horizon` frames past the most recently drained
    /// frame. A power of two so the bucket index is a mask; the spare slot keeps the
    /// furthest ticket from aliasing the bucket being drained.
    fn with_horizon(horizon: u64) -> Self {
        let slots = Self::slots_for_horizon(horizon) as usize;
        Self {
            buckets: (0..slots).map(|_| Vec::new()).collect(),
            mask: slots as u64 - 1,
            drained_through: 0,
            next_seq: 0,
            scratch: Vec::new(),
        }
    }

    /// File a ticket to revisit `key` at frame `due`: the first frame on which the
    /// entry is dead, so the caller's own `deadline < frame` test decides.
    ///
    /// A `due` outside the ring is clamped, not rejected: early firing costs one
    /// re-file, but late firing is the outcome the wheel must never produce, and a
    /// ticket more than a ring out would alias the drained bucket and do that.
    ///
    /// **A `due` earlier than the entry's outstanding ticket supplants it**: the wheel
    /// can't remove from a bucket, so both are live. Stamp the entry with the returned
    /// [`TicketSeq`] and let [`Self::retire`] drop whatever fires under an older one,
    /// or the pair re-files for the entry's life.
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

    /// Put a ticket in its bucket, clamped into the ring. A re-file keeps the serial it
    /// fired under, hence separate from [`Self::schedule`].
    fn file(&mut self, ticket: Ticket<K>, due: u64) {
        let due = due.clamp(self.drained_through + 1, self.drained_through + self.mask);
        let index = (due & self.mask) as usize;
        let len = self.buckets[index].len();
        if len == self.buckets[index].capacity() {
            self.grow_buckets(len + 1);
        }
        self.buckets[index].push(ticket);
    }

    /// Give every bucket room for `len` tickets, doubling past it. All at once: tickets
    /// drift across the ring, so a bucket can meet the busiest occupancy another
    /// already saw and would allocate long after warmup.
    #[cold]
    fn grow_buckets(&mut self, len: usize) {
        let capacity = len.next_power_of_two().max(4);
        for bucket in &mut self.buckets {
            bucket.reserve_exact(capacity - bucket.len());
        }
    }

    /// Hand every ticket due through `frame` to `settle`, re-filing each at the
    /// deadline it returns and forgetting those it answers `None` for. `settle`
    /// re-reads the entry's real deadline ("come back at N" or "gone"); its second
    /// argument is the ticket's [`TicketSeq`], which a stamping owner compares with
    /// its entry. `settle` may mutate the owner's other fields: the wheel borrows only itself.
    pub(crate) fn retire(
        &mut self,
        frame: u64,
        mut settle: impl FnMut(K, TicketSeq) -> Option<u64>,
    ) {
        if frame <= self.drained_through {
            return;
        }
        // Out and back so the ring stays free to re-file; capacity is retained.
        let mut due = mem::take(&mut self.scratch);

        // A clock jump past the ring has aliased every bucket, so all are due. Draining
        // early costs one re-file, never a wrong drop.
        let slots = self.buckets.len() as u64;
        let first = if frame - self.drained_through >= slots {
            frame + 1 - slots
        } else {
            self.drained_through + 1
        };
        // `drained_through` moves first: re-filed tickets are all past `frame`.
        self.drained_through = frame;
        // Empty every due bucket before the first re-file, or a re-filed ticket landing in
        // a bucket not yet reached would fire again immediately.
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

    /// Drop every outstanding ticket, keeping bucket capacity, for an owner that
    /// cleared its whole map.
    pub(crate) fn clear(&mut self) {
        for bucket in &mut self.buckets {
            bucket.clear();
        }
    }
}

#[cfg(any(test, feature = "internals"))]
pub(crate) mod internals {
    use super::*;

    impl<K: Copy + Debug> ExpiryWheel<K> {
        /// The bucket count [`Self::with_keep`] builds for `keep_frames`: the frames one
        /// revolution takes.
        pub(crate) const fn slots_for_keep(keep_frames: u64) -> u64 {
            Self::slots_for_horizon(keep_frames + 2)
        }

        /// Outstanding tickets across the whole ring. Violating the owner protocol still
        /// expires correctly but grows the count and per-frame drain without bound;
        /// `EncodedCache`'s and `CosmicMeasure`'s tests assert against it.
        #[cfg(test)]
        pub(crate) fn pending(&self) -> usize {
            self.buckets.iter().map(Vec::len).sum()
        }
    }
}

#[cfg(test)]
mod tests;
