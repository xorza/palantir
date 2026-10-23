//! Intrusive most-recently-used list over gradient atlas row ids.
//!
//! Two link columns indexed by row. Rows `1..capacity` are members for the
//! atlas's whole life (only moved, never inserted or removed), so
//! [`MruList::touch`] needs no "is it linked?" branch. Row 0 is the permanent
//! magenta fallback and is never a member.
//!
//! Why the tail alone answers eviction: [`CpuGradientAtlas::register`] moves a
//! row to the head on every registration and stamps the epoch, and nothing
//! moves a row backward, so the epoch-current rows are a head prefix. The atlas
//! checks the tail's stamp; if it is current, every row is, and the atlas must
//! grow rather than repaint a row this frame's draws reference.
//!
//! [`CpuGradientAtlas::register`]:
//!     crate::renderer::gradient_atlas::CpuGradientAtlas::register

use crate::common::non_max_u32::NonMaxU32;

#[derive(Debug)]
pub(super) struct MruList {
    /// Toward the head (more recently used), `None` at the head. Slot 0 unused.
    prev: Vec<Option<NonMaxU32>>,
    /// Toward the tail (less recently used), `None` at the tail. Slot 0 unused.
    next: Vec<Option<NonMaxU32>>,
    head: Option<NonMaxU32>,
    tail: Option<NonMaxU32>,
}

impl MruList {
    /// List over rows `1..capacity`, LRU end first, so a fresh atlas hands out
    /// ascending ids and a warm-up frame's dirty span stays contiguous.
    pub(super) fn seeded(capacity: u32) -> Self {
        let mut list = Self {
            prev: Vec::new(),
            next: Vec::new(),
            head: None,
            tail: None,
        };
        list.extend_to(1, capacity);
        list
    }

    pub(super) const fn tail(&self) -> u32 {
        self.tail.expect("MRU list is empty").get()
    }

    /// Link rows `first..end` behind the tail, largest id first. New rows hold no
    /// gradient, so they sit at the eviction end and are claimed first.
    pub(super) fn extend_to(&mut self, first: u32, end: u32) {
        self.prev.resize(end as usize, None);
        self.next.resize(end as usize, None);
        for row in (first..end).rev() {
            self.push_back(NonMaxU32::index(row as usize));
        }
    }

    /// Move `row` to the head.
    pub(super) fn touch(&mut self, row: u32) {
        debug_assert!(
            row != 0 && (row as usize) < self.next.len(),
            "row {row} is not an MRU member",
        );
        let row = NonMaxU32::index(row as usize);
        if self.head == Some(row) {
            return;
        }
        self.unlink(row);
        self.push_front(row);
    }

    fn unlink(&mut self, row: NonMaxU32) {
        let (prev, next) = (self.prev[row.idx()], self.next[row.idx()]);
        match prev {
            None => self.head = next,
            Some(prev) => self.next[prev.idx()] = next,
        }
        match next {
            None => self.tail = prev,
            Some(next) => self.prev[next.idx()] = prev,
        }
    }

    fn push_front(&mut self, row: NonMaxU32) {
        self.prev[row.idx()] = None;
        self.next[row.idx()] = self.head;
        match self.head {
            None => self.tail = Some(row),
            Some(head) => self.prev[head.idx()] = Some(row),
        }
        self.head = Some(row);
    }

    /// Every `next` has a matching `prev`, the walk ends at the recorded tail and
    /// visits exactly rows `1..len`. [`CpuGradientAtlas::grow`] asserts on it so
    /// corruption doesn't surface as a far-off wrong eviction.
    ///
    /// [`CpuGradientAtlas::grow`]:
    ///     crate::renderer::gradient_atlas::CpuGradientAtlas::grow
    pub(super) fn is_well_formed(&self) -> bool {
        let members = self.next.len().saturating_sub(1);
        let mut seen = 0usize;
        let mut prev = None;
        let mut cursor = self.head;
        while let Some(row) = cursor {
            if row.get() == 0 || self.prev[row.idx()] != prev {
                return false;
            }
            seen += 1;
            if seen > members {
                return false;
            }
            prev = Some(row);
            cursor = self.next[row.idx()];
        }
        seen == members && self.tail == prev
    }

    fn push_back(&mut self, row: NonMaxU32) {
        self.next[row.idx()] = None;
        self.prev[row.idx()] = self.tail;
        match self.tail {
            None => self.head = Some(row),
            Some(tail) => self.next[tail.idx()] = Some(row),
        }
        self.tail = Some(row);
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use super::*;

    impl MruList {
        /// Rows from most- to least-recently-used via `next`; pair with
        /// [`MruList::is_well_formed`] to cover `prev`.
        pub(crate) fn to_vec(&self) -> Vec<u32> {
            let mut out = Vec::new();
            let mut cursor = self.head;
            while let Some(row) = cursor {
                out.push(row.get());
                cursor = self.next[row.idx()];
            }
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A seeded list holds every row once, MRU first, with the smallest row at the
    /// eviction end, so the first claims walk 1, 2, 3.
    #[test]
    fn seeded_list_evicts_ascending_from_row_one() {
        let list = MruList::seeded(5);
        assert!(list.is_well_formed());
        assert_eq!(list.to_vec(), vec![4, 3, 2, 1]);
        assert_eq!(list.tail(), 1);
    }

    /// Touching moves a row to the head from head (no-op), middle or tail.
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

    /// Grown rows land behind every resident row in ascending order.
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

    /// Draining every row through the tail and back to the head leaves the links
    /// intact.
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
