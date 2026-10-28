//! The panes whose captures a frame's damage left stale.

use crate::primitives::identity::pane_key::PaneKey;
use tinyvec::ArrayVec;

/// The panes a frame must capture again, as damage found them: content painted before each
/// changed under its footprint. More than [`Self::CAP`] makes the frame whole, which captures
/// every pane.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct DirtyPanes {
    keys: ArrayVec<[PaneKey; Self::CAP]>,
}

impl DirtyPanes {
    pub(crate) const CAP: usize = 8;

    pub(crate) fn contains(&self, key: PaneKey) -> bool {
        self.keys.contains(&key)
    }

    pub(super) fn clear(&mut self) {
        self.keys.clear();
    }

    /// Add `key`; `false` when the set is full.
    pub(super) fn push(&mut self, key: PaneKey) -> bool {
        self.keys.try_push(key).is_none()
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::damage::dirty_panes::DirtyPanes;
    use crate::primitives::identity::pane_key::PaneKey;

    /// A set holding `keys`, as a frame's damage would; at most [`DirtyPanes::CAP`].
    impl FromIterator<PaneKey> for DirtyPanes {
        fn from_iter<I: IntoIterator<Item = PaneKey>>(keys: I) -> Self {
            let mut set = Self::default();
            for key in keys {
                assert!(set.push(key), "at most {} dirty panes", Self::CAP);
            }
            set
        }
    }
}
