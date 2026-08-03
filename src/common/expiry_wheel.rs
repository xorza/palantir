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
//! ticket fires and either drops it or files a fresh ticket. That makes
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

use std::fmt::Debug;

/// Ring of pending expiry tickets keyed by due frame.
///
/// Sized at construction from the longest deadline the owner can hand
/// out; see [`Self::with_horizon`].
#[derive(Debug)]
pub(crate) struct ExpiryWheel<K> {
    /// Ring indexed by `due & mask`. Buckets keep their capacity across
    /// drains, so a steady workload files and fires tickets without
    /// allocating.
    buckets: Vec<Vec<K>>,
    mask: u64,
    /// Highest frame whose bucket has been drained. Tickets must be
    /// filed strictly after it, or they land in a bucket this cycle has
    /// already passed and would not fire for another full ring.
    drained_through: u64,
    /// Retained landing area for the keys [`Self::retire`] is walking.
    ///
    /// Lives here rather than on each owner because it is part of this
    /// type's protocol, not theirs: it exists only so the ring can be
    /// re-filed while its own drained tickets are being walked.
    scratch: Vec<K>,
}

impl<K: Copy + Debug> ExpiryWheel<K> {
    /// A wheel that can hold a ticket up to `horizon` frames past the
    /// frame filing it.
    ///
    /// Rounded up to a power of two so the bucket index is a mask rather
    /// than a division — this runs per filed ticket. One spare slot
    /// beyond the horizon keeps the furthest ticket from aliasing the
    /// bucket being drained.
    pub(crate) fn with_horizon(horizon: u64) -> Self {
        let slots = (horizon + 1).next_power_of_two() as usize;
        Self {
            buckets: (0..slots).map(|_| Vec::new()).collect(),
            mask: slots as u64 - 1,
            drained_through: 0,
            scratch: Vec::new(),
        }
    }

    /// File a ticket to revisit `key` at frame `due` — the first frame on
    /// which the entry is dead, not the last on which it lives, so the
    /// caller's own `deadline < frame` test decides and the wheel holds
    /// no policy.
    /// A `due` outside the ring is clamped into it rather than rejected.
    /// Both ends are safe precisely because a ticket is a hint: firing
    /// early costs one re-file, while firing *late* is the one outcome
    /// the wheel must never produce — and a ticket filed more than a
    /// full ring out would alias the bucket just drained and do exactly
    /// that. Owners whose sweep can fall behind the clock (a window that
    /// stops submitting, a test driving frames by hand) would otherwise
    /// have to reason about the ring width themselves.
    pub(crate) fn schedule(&mut self, key: K, due: u64) {
        debug_assert!(
            due > self.drained_through,
            "ticket for {key:?} at {due} is not past the drained frame {}",
            self.drained_through,
        );
        let due = due.clamp(self.drained_through + 1, self.drained_through + self.mask);
        self.buckets[(due & self.mask) as usize].push(key);
    }

    /// Move every ticket due through `frame` into `out`, which is
    /// cleared first.
    ///
    /// Takes a destination rather than returning an iterator so the
    /// wheel can be re-filed while the drain is walked: a draining
    /// iterator would hold it borrowed for exactly the span in which
    /// [`Self::schedule`] has to be callable. [`Self::retire`] is the
    /// public form of that walk.
    fn take_due(&mut self, frame: u64, out: &mut Vec<K>) {
        out.clear();
        if frame <= self.drained_through {
            return;
        }
        // A clock that jumped further than the ring is wide has aliased
        // every bucket, so every bucket is due. Draining a ticket early
        // costs one re-file, never a wrong drop.
        let slots = self.buckets.len() as u64;
        let first = if frame - self.drained_through >= slots {
            frame + 1 - slots
        } else {
            self.drained_through + 1
        };
        // `drained_through` moves first: the caller re-files from inside
        // its walk, and those tickets are all past `frame`.
        self.drained_through = frame;
        for f in first..=frame {
            let bucket = &mut self.buckets[(f & self.mask) as usize];
            out.append(bucket);
        }
    }

    /// Hand every ticket due through `frame` to `settle`, re-filing each
    /// key at the deadline it returns and forgetting the ones it answers
    /// `None` for.
    ///
    /// This is the whole owner-side protocol in one call: a ticket is a
    /// hint, so `settle` re-reads the entry's real deadline and says
    /// either "still live, come back at N" or "gone". Owners used to
    /// open-code the drain — each carrying its own scratch `Vec` and the
    /// same four-line take/walk/put-back dance — which put this type's
    /// invariants in three places at once.
    ///
    /// `settle` may freely mutate the owner's other fields: the wheel
    /// borrows only itself, so disjoint field captures let the closure
    /// hold the map it is retiring from.
    pub(crate) fn retire(&mut self, frame: u64, mut settle: impl FnMut(K) -> Option<u64>) {
        // Out and back so the ring stays free to be re-filed below;
        // capacity is retained across the swap.
        let mut due = std::mem::take(&mut self.scratch);
        self.take_due(frame, &mut due);
        for key in due.drain(..) {
            if let Some(next) = settle(key) {
                self.schedule(key, next);
            }
        }
        self.scratch = due;
    }
}

#[cfg(any(test, feature = "internals"))]
pub(crate) mod internals {
    use super::*;

    impl<K: Copy + Debug> ExpiryWheel<K> {
        /// Drop every outstanding ticket, keeping bucket capacity. For
        /// an owner that just cleared its whole map — every ticket would
        /// be stale, and re-filing from an empty map yields nothing.
        ///
        /// Gated with its only caller,
        /// `CosmicMeasure::drop_all_buffers`: production has no path
        /// that empties a cache wholesale.
        pub(crate) fn clear(&mut self) {
            for bucket in &mut self.buckets {
                bucket.clear();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl<K: Copy + Debug> ExpiryWheel<K> {
        /// Outstanding tickets across the whole ring.
        ///
        /// The number that says whether an owner is holding up its end
        /// of the protocol: file on insert, file again only when a
        /// deadline moves *in*. An owner that also re-filed on every
        /// touch would still expire correctly, just with the ticket
        /// count — and the per-frame drain — growing by a factor of the
        /// horizon. `EncodedCache`'s tests assert exactly that.
        pub(crate) fn pending(&self) -> usize {
            self.buckets.iter().map(Vec::len).sum()
        }
    }

    /// The wheel is a schedule, not a policy: a ticket comes back on its
    /// due frame, once, and only then.
    #[test]
    fn tickets_fire_on_their_due_frame_and_only_then() {
        let mut wheel = ExpiryWheel::<u32>::with_horizon(8);
        let mut out = Vec::new();

        wheel.schedule(10, 3);
        wheel.schedule(20, 5);
        wheel.schedule(21, 5);

        for frame in 1..=2 {
            wheel.take_due(frame, &mut out);
            assert!(out.is_empty(), "nothing is due at {frame}");
        }
        wheel.take_due(3, &mut out);
        assert_eq!(out, vec![10]);
        wheel.take_due(4, &mut out);
        assert!(out.is_empty(), "a fired ticket must not fire twice");
        wheel.take_due(5, &mut out);
        assert_eq!(out, vec![20, 21], "one bucket can hold several keys");
        wheel.take_due(6, &mut out);
        assert!(out.is_empty());
    }

    /// A clock that advances by more than one — two windows recording
    /// before one shared submit — must not step over the buckets in
    /// between.
    #[test]
    fn a_jumping_clock_drains_every_bucket_it_passed() {
        let mut wheel = ExpiryWheel::<u32>::with_horizon(8);
        let mut out = Vec::new();
        for (key, due) in [(1u32, 2u64), (2, 3), (3, 4), (4, 7)] {
            wheel.schedule(key, due);
        }

        wheel.take_due(4, &mut out);
        let mut fired = out.clone();
        fired.sort_unstable();
        assert_eq!(fired, vec![1, 2, 3], "frames 1..=4 all drained");

        wheel.take_due(9, &mut out);
        assert_eq!(out, vec![4], "the later ticket still fires");
    }

    /// A jump wider than the ring aliases every bucket, so everything is
    /// handed back — including tickets that were not really due. Callers
    /// re-file those, so the contract is "never a missed ticket", not
    /// "never an early one".
    #[test]
    fn a_jump_wider_than_the_ring_hands_back_everything() {
        let mut wheel = ExpiryWheel::<u32>::with_horizon(8);
        let mut out = Vec::new();
        // Horizon 8 rounds to 16 slots.
        wheel.schedule(1, 2);
        wheel.schedule(2, 9);

        wheel.take_due(100, &mut out);
        let mut fired = out.clone();
        fired.sort_unstable();
        assert_eq!(fired, vec![1, 2], "both, though only one was due");

        // And the ring is genuinely empty afterwards — an early-drained
        // ticket must not also be left behind to fire again.
        wheel.take_due(200, &mut out);
        assert!(out.is_empty());
    }

    /// The re-file pattern the owners run: fire early, find the entry
    /// still live, put it back. This is what lets an entry touched every
    /// frame file one ticket per horizon rather than one per frame.
    #[test]
    fn refiling_from_inside_a_drain_defers_without_extra_tickets() {
        let mut wheel = ExpiryWheel::<u32>::with_horizon(8);
        let mut out = Vec::new();
        wheel.schedule(7, 2);

        // The owner "touches" the entry every frame, pushing its real
        // deadline out — but files nothing until its ticket fires at 2,
        // and then files one covering the whole span to 6.
        let mut fired_at = Vec::new();
        for frame in 1..=5 {
            wheel.take_due(frame, &mut out);
            for &key in out.iter() {
                fired_at.push(frame);
                wheel.schedule(key, 6);
            }
        }
        assert_eq!(
            fired_at,
            vec![2],
            "one ticket per deferral, not one per frame",
        );

        // Let it lapse: it fires at 6, is not re-filed, and is gone.
        wheel.take_due(6, &mut out);
        assert_eq!(out, vec![7]);
        wheel.take_due(20, &mut out);
        assert!(out.is_empty(), "a ticket not re-filed does not come back");
    }

    /// A ticket filed further out than the ring is wide must fire
    /// *early*, not alias its way into a bucket already drained and fire
    /// a whole ring late. The owner re-files it, so the only cost is one
    /// extra visit.
    #[test]
    fn a_ticket_past_the_ring_fires_early_rather_than_late() {
        // Horizon 8 rounds to 16 slots, so the furthest safe bucket is
        // 15 frames out; 200 would alias frame 8 (200 % 16 == 8).
        let mut wheel = ExpiryWheel::<u32>::with_horizon(8);
        let mut out = Vec::new();
        wheel.schedule(1, 200);

        // Walk the frames a naive `due & mask` would have fired on, plus
        // the whole first ring, and pin that it came back inside it.
        let mut fired = None;
        for frame in 1..=15 {
            wheel.take_due(frame, &mut out);
            if !out.is_empty() {
                fired = Some(frame);
                break;
            }
        }
        assert_eq!(fired, Some(15), "clamped to the ring's far edge");
    }

    #[test]
    fn clear_drops_every_outstanding_ticket() {
        let mut wheel = ExpiryWheel::<u32>::with_horizon(8);
        let mut out = Vec::new();
        wheel.schedule(1, 2);
        wheel.schedule(2, 6);

        wheel.clear();
        wheel.take_due(9, &mut out);
        assert!(out.is_empty());
    }

    /// Horizon rounds up to a power of two, and the mask must index
    /// inside the ring for every frame a caller can reach.
    #[test]
    fn horizon_rounds_up_and_indexes_in_range() {
        for (horizon, slots) in [(1u64, 2usize), (3, 4), (8, 16), (120, 128), (121, 128)] {
            let wheel = ExpiryWheel::<u32>::with_horizon(horizon);
            assert_eq!(wheel.buckets.len(), slots, "horizon {horizon}");
            assert_eq!(wheel.mask, slots as u64 - 1, "horizon {horizon}");
            assert!(
                horizon <= wheel.mask,
                "horizon {horizon} must fit the schedule assert",
            );
        }
    }
}
