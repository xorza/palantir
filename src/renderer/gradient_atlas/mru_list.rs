//! Intrusive most-recently-used list over gradient atlas row ids.
//!
//! Two `u32` columns indexed by row, so a touch is four stores and an
//! eviction candidate is one load. Every row in `1..capacity` is a
//! member for the atlas's whole life — a row is never inserted or
//! removed, only moved — which is what lets [`MruList::touch`] skip the
//! "is it linked?" branch a general list would need.
//!
//! Row 0 is the atlas's permanent magenta fallback and is never a
//! member.
//!
//! ## Why the tail alone answers eviction
//!
//! [`CpuGradientAtlas::register_stops`] moves a row to the head on
//! every registration — hit or claim — and stamps it with the current
//! epoch. Nothing ever moves a row backward, so every row registered
//! this epoch sits strictly ahead of every row that wasn't: **the
//! epoch-current rows are exactly a head prefix.** The atlas therefore
//! reads [`MruList::tail`] and checks one epoch stamp instead of
//! scanning for a victim — if the tail is epoch-current then every row
//! is, and the atlas must grow rather than repaint a row this frame's
//! draws already reference.
//!
//! [`CpuGradientAtlas::register_stops`]:
//!     crate::renderer::gradient_atlas::CpuGradientAtlas::register_stops

/// Absent link. Distinguishable from every real row id because the row
/// count is bounded by
/// [`MAX_ATLAS_ROWS`](crate::renderer::gradient_atlas::MAX_ATLAS_ROWS).
///
/// The block arena's free lists carry the same value for the same
/// reason; see [`crate::common::block_arena`] for why the two are not
/// one constant. The lists themselves have less in common than the
/// sentinel suggests — this one is a doubly-linked total order over a
/// fixed membership, that one a per-class LIFO stack whose members come
/// and go.
const NIL: u32 = u32::MAX;

#[derive(Debug)]
pub(super) struct MruList {
    /// Toward the head — the more-recently-used neighbour, `NIL` at the
    /// head. Indexed by row; slot 0 is unused.
    prev: Vec<u32>,
    /// Toward the tail — the less-recently-used neighbour, `NIL` at the
    /// tail. Indexed by row; slot 0 is unused.
    next: Vec<u32>,
    head: u32,
    tail: u32,
}

impl MruList {
    /// List over rows `1..capacity`, least-recently-used end first, so a
    /// fresh atlas hands out ascending row ids and a warm-up frame's
    /// dirty span stays contiguous from the low end.
    pub(super) fn seeded(capacity: u32) -> Self {
        let mut list = Self {
            prev: Vec::new(),
            next: Vec::new(),
            head: NIL,
            tail: NIL,
        };
        list.extend_to(1, capacity);
        list
    }

    /// The least-recently-used row — the atlas's eviction candidate.
    pub(super) fn tail(&self) -> u32 {
        debug_assert_ne!(self.tail, NIL, "MRU list is empty");
        self.tail
    }

    /// Link rows `first..end` in behind the current tail, largest id
    /// first so the smallest ends up last. Newly grown rows hold no
    /// gradient, so they belong at the eviction end and get claimed
    /// before any resident row is evicted.
    pub(super) fn extend_to(&mut self, first: u32, end: u32) {
        self.prev.resize(end as usize, NIL);
        self.next.resize(end as usize, NIL);
        for row in (first..end).rev() {
            self.push_back(row);
        }
    }

    /// Move `row` to the head.
    pub(super) fn touch(&mut self, row: u32) {
        debug_assert!(
            row != 0 && (row as usize) < self.next.len(),
            "row {row} is not an MRU member",
        );
        if self.head == row {
            return;
        }
        self.unlink(row);
        self.push_front(row);
    }

    fn unlink(&mut self, row: u32) {
        let (prev, next) = (self.prev[row as usize], self.next[row as usize]);
        match prev {
            NIL => self.head = next,
            prev => self.next[prev as usize] = next,
        }
        match next {
            NIL => self.tail = prev,
            next => self.prev[next as usize] = prev,
        }
    }

    fn push_front(&mut self, row: u32) {
        self.prev[row as usize] = NIL;
        self.next[row as usize] = self.head;
        match self.head {
            NIL => self.tail = row,
            head => self.prev[head as usize] = row,
        }
        self.head = row;
    }

    /// Every `next` link has a matching `prev`, the walk terminates at
    /// the recorded tail, and it visits exactly the rows `1..len`. A
    /// corrupted list would otherwise surface as a wrong eviction far
    /// from the mistake, so [`CpuGradientAtlas::grow`] asserts on it.
    /// Allocation-free — a `debug_assert!` body is compiled into release
    /// even though it never runs there.
    ///
    /// [`CpuGradientAtlas::grow`]:
    ///     crate::renderer::gradient_atlas::CpuGradientAtlas::grow
    pub(super) fn is_well_formed(&self) -> bool {
        let members = self.next.len().saturating_sub(1);
        let mut seen = 0usize;
        let mut prev = NIL;
        let mut row = self.head;
        while row != NIL {
            if row == 0 || self.prev[row as usize] != prev {
                return false;
            }
            seen += 1;
            if seen > members {
                return false;
            }
            prev = row;
            row = self.next[row as usize];
        }
        seen == members && self.tail == prev
    }

    fn push_back(&mut self, row: u32) {
        self.next[row as usize] = NIL;
        self.prev[row as usize] = self.tail;
        match self.tail {
            NIL => self.head = row,
            tail => self.next[tail as usize] = row,
        }
        self.tail = row;
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    impl MruList {
        /// Rows from most- to least-recently-used. Walks `next`; pair it
        /// with [`MruList::is_well_formed`], which proves the `prev`
        /// links agree, to cover both directions.
        pub(crate) fn to_vec(&self) -> Vec<u32> {
            let mut out = Vec::new();
            let mut row = self.head;
            while row != NIL {
                out.push(row);
                row = self.next[row as usize];
            }
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A seeded list holds every row once, most-recently-used first,
    /// with the *smallest* row at the eviction end — so the first
    /// claims walk 1, 2, 3… and a warm-up frame dirties one low
    /// contiguous span.
    #[test]
    fn seeded_list_evicts_ascending_from_row_one() {
        let list = MruList::seeded(5);
        assert!(list.is_well_formed());
        assert_eq!(list.to_vec(), vec![4, 3, 2, 1]);
        assert_eq!(list.tail(), 1);
    }

    /// Touching walks a row to the head from any position — head
    /// (no-op), middle, and tail — and the tail follows.
    #[test]
    fn touch_moves_to_head_from_every_position() {
        let mut list = MruList::seeded(5);
        // Tail: 1 comes to the front, 2 becomes the new eviction target.
        list.touch(1);
        assert_eq!(list.to_vec(), vec![1, 4, 3, 2]);
        assert_eq!(list.tail(), 2);
        // Middle.
        list.touch(3);
        assert_eq!(list.to_vec(), vec![3, 1, 4, 2]);
        // Head: already there, nothing moves.
        list.touch(3);
        assert_eq!(list.to_vec(), vec![3, 1, 4, 2]);
        assert_eq!(list.tail(), 2);
        assert!(list.is_well_formed());
    }

    /// Grown rows land behind every resident row and in ascending order,
    /// so an empty row is always claimed before a resident one is
    /// evicted.
    #[test]
    fn extend_appends_new_rows_at_the_eviction_end() {
        let mut list = MruList::seeded(3);
        list.touch(1);
        assert_eq!(list.to_vec(), vec![1, 2]);
        list.extend_to(3, 6);
        assert_eq!(list.to_vec(), vec![1, 2, 5, 4, 3]);
        assert_eq!(list.tail(), 3);
        assert!(list.is_well_formed());
    }

    /// Draining every row through the tail and back to the head — what
    /// a full round of evictions does — leaves the links intact.
    #[test]
    fn full_rotation_preserves_structure() {
        let mut list = MruList::seeded(8);
        for _ in 0..7 {
            let victim = list.tail();
            list.touch(victim);
            assert!(list.is_well_formed(), "corrupted after touching {victim}");
        }
        // Seven touches from the tail reverse the seeded order exactly.
        assert_eq!(list.to_vec(), vec![7, 6, 5, 4, 3, 2, 1]);
    }
}
