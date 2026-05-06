//! Sparse per-entity column: a dense `idx` index column (one `u16`
//! per entity, sentinel `u16::MAX` = absent) paired with a dense
//! `table` holding only the present entries. Same shape as the
//! classic ECS sparse-set, packaged so the two halves can't desync.
//!
//! Used by `Tree` for `chrome` and `extras` — fields most nodes leave
//! default. The index column stays small (2 bytes/node) regardless of
//! how rare the entries are; the table holds only what's present.

/// Sparse per-entity column. `idx[i] == ABSENT` means entity `i` has
/// no entry; otherwise `idx[i]` is a slot in `table`. Cap of
/// `u16::MAX` (65 535) present entries per frame.
#[derive(Default)]
pub(crate) struct SparseColumn<T> {
    /// One slot per entity, indexed externally (e.g. by `NodeId.0`).
    /// `Self::ABSENT` for entities without an entry; otherwise an
    /// index into `table`.
    pub(crate) idx: Vec<u16>,
    /// Dense storage of present entries.
    pub(crate) table: Vec<T>,
}

impl<T> SparseColumn<T> {
    /// Sentinel `idx` slot meaning "no entry for this entity".
    pub(crate) const ABSENT: u16 = u16::MAX;

    pub(crate) fn clear(&mut self) {
        self.idx.clear();
        self.table.clear();
    }

    /// Push a new entity's entry. `None` → sentinel; `Some(v)` →
    /// store `v` in `table` and record its slot in `idx`. Asserts
    /// the table stays within `u16` range.
    pub(crate) fn push(&mut self, value: Option<T>) {
        match value {
            None => self.idx.push(Self::ABSENT),
            Some(v) => {
                assert!(
                    self.table.len() < Self::ABSENT as usize,
                    "SparseColumn full — more than 65 535 entries in a single frame",
                );
                let slot = self.table.len() as u16;
                self.table.push(v);
                self.idx.push(slot);
            }
        }
    }

    /// Borrow the entry for entity at index `i`, or `None` if absent.
    #[inline]
    pub(crate) fn get(&self, i: usize) -> Option<&T> {
        let slot = self.idx[i];
        if slot == Self::ABSENT {
            None
        } else {
            Some(&self.table[slot as usize])
        }
    }
}
