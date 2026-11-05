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

/// Tickets in one chunk: 512 B of 32-byte tickets, eight cache lines, so a drain follows one
/// link per eight lines it copies, and a partly filled chunk at the tail of each of 256 buckets
/// keeps at most 128 KiB.
const CHUNK_TICKETS: usize = 16;

// A chunk's fill is a `u8`.
const _: () = assert!(CHUNK_TICKETS <= u8::MAX as usize);

/// No chunk: the end of a bucket's chain, or a bucket with none.
const NO_CHUNK: u32 = u32::MAX;

/// A chunk's fill, and its successor in its bucket's chain.
#[derive(Clone, Copy, Debug)]
struct ChunkLink {
    len: u8,
    next: u32,
}

/// A bucket's chain of chunks, filled at the tail and drained from the head, so a bucket hands
/// its tickets back in the order they were filed.
#[derive(Clone, Copy, Debug)]
struct Chain {
    head: u32,
    tail: u32,
}

impl Chain {
    const EMPTY: Self = Self {
        head: NO_CHUNK,
        tail: NO_CHUNK,
    };
}

/// Ring of pending expiry tickets keyed by due frame, sized from the longest
/// deadline the owner can hand out ([`Self::with_keep`]).
///
/// Every bucket draws fixed chunks from one pool the whole ring shares, so the memory it touches
/// follows the most tickets outstanding at once, and a burst in one bucket costs what it holds: 4000
/// tickets take 250 chunks, 128 KiB, where a bucket of its own per frame held 256 times that
/// for the session. A drain gives its chunks back to the pool, so once the pool reaches its
/// peak no bucket allocates, whichever bucket a later burst lands in. The pool starts with room
/// for a chunk per bucket, so tickets spread over the ring never allocate, and only a burst past
/// that room grows it in a frame. Started empty, it grew on the frame a first expiry filed its
/// tickets, which the process's hash seed picks: 5 to 12 of 100 processes allocated that way after
/// their warmup. The room is capacity alone, so a page no ticket reached stays untouched. Refused:
/// a `Vec` per bucket grown together, which kept every bucket at the busiest bucket's load; one
/// grown alone, which allocates in whichever later frame a bucket first meets that load.
#[derive(Debug)]
pub(crate) struct ExpiryWheel<K> {
    /// Ring indexed by `due & mask`: each bucket's chain of chunks.
    buckets: Box<[Chain]>,
    mask: u64,
    /// Every chunk's tickets, [`CHUNK_TICKETS`] a chunk, in one buffer; a chunk's unfilled
    /// slots hold whatever ticket was filed there last, never read.
    tickets: Vec<Ticket<K>>,
    /// Each chunk's fill and link, by chunk index.
    links: Vec<ChunkLink>,
    /// The chunks no bucket holds. Its capacity is held at the chunk count, so handing a chunk
    /// back never allocates.
    free: Vec<u32>,
    /// Highest frame whose bucket has been drained. Tickets must be filed strictly
    /// after it, or they wait a full ring.
    drained_through: u64,
    next_seq: TicketSeq,
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
            buckets: vec![Chain::EMPTY; slots].into_boxed_slice(),
            mask: slots as u64 - 1,
            tickets: Vec::with_capacity(slots * CHUNK_TICKETS),
            links: Vec::with_capacity(slots),
            free: Vec::with_capacity(slots),
            drained_through: 0,
            next_seq: 0,
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
        let idx = (due & self.mask) as usize;
        let tail = self.buckets[idx].tail;
        let chunk =
            if tail != NO_CHUNK && usize::from(self.links[tail as usize].len) < CHUNK_TICKETS {
                tail
            } else {
                let chunk = self.take_chunk(ticket);
                match tail {
                    NO_CHUNK => self.buckets[idx].head = chunk,
                    tail => self.links[tail as usize].next = chunk,
                }
                self.buckets[idx].tail = chunk;
                chunk
            };
        let link = &mut self.links[chunk as usize];
        self.tickets[chunk as usize * CHUNK_TICKETS + usize::from(link.len)] = ticket;
        link.len += 1;
    }

    /// An empty chunk: one the pool holds, else a new one.
    fn take_chunk(&mut self, filler: Ticket<K>) -> u32 {
        self.free.pop().unwrap_or_else(|| self.grow_chunk(filler))
    }

    /// A new chunk, its slots filled with `filler`, which no read reaches: the pool grows only
    /// past the most tickets ever outstanding at once.
    #[cold]
    fn grow_chunk(&mut self, filler: Ticket<K>) -> u32 {
        let chunk = u32::try_from(self.links.len())
            .ok()
            .filter(|&chunk| chunk != NO_CHUNK)
            .expect("a wheel holds fewer than 2^32 − 1 chunks");
        self.tickets
            .extend(std::iter::repeat_n(filler, CHUNK_TICKETS));
        self.links.push(ChunkLink {
            len: 0,
            next: NO_CHUNK,
        });
        // Room for every chunk at once, so handing one back never allocates.
        self.free.reserve(self.links.len() - self.free.len());
        chunk
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
        // Detach every due bucket's chain into one before the first re-file, or a re-filed
        // ticket landing in a bucket not yet reached would fire again immediately.
        let mut due = Chain::EMPTY;
        for f in first..=frame {
            let chain = mem::replace(&mut self.buckets[(f & self.mask) as usize], Chain::EMPTY);
            if chain.head == NO_CHUNK {
                continue;
            }
            match due.tail {
                NO_CHUNK => due.head = chain.head,
                tail => self.links[tail as usize].next = chain.head,
            }
            due.tail = chain.tail;
        }

        let mut chunk = due.head;
        while chunk != NO_CHUNK {
            let ChunkLink { len, next } = self.links[chunk as usize];
            let start = chunk as usize * CHUNK_TICKETS;
            for i in start..start + usize::from(len) {
                let ticket = self.tickets[i];
                if let Some(next) = settle(ticket.key, ticket.seq) {
                    self.file(ticket, next);
                }
            }
            self.links[chunk as usize] = ChunkLink {
                len: 0,
                next: NO_CHUNK,
            };
            self.free.push(chunk);
            chunk = next;
        }
    }

    /// Drop every outstanding ticket, keeping every chunk in the pool, for an owner that
    /// cleared its whole map.
    pub(crate) fn clear(&mut self) {
        self.buckets.fill(Chain::EMPTY);
        self.links.fill(ChunkLink {
            len: 0,
            next: NO_CHUNK,
        });
        self.free.clear();
        self.free.extend(0..self.links.len() as u32);
    }
}

#[cfg(feature = "internals")]
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
            self.links.iter().map(|link| usize::from(link.len)).sum()
        }

        /// Chunks the pool has made, held by a bucket or free.
        #[cfg(test)]
        pub(crate) const fn chunks(&self) -> usize {
            self.links.len()
        }
    }
}

#[cfg(test)]
mod tests;
