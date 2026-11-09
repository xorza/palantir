//! The backdrops whose captures a frame's damage left stale.

use crate::primitives::identity::backdrop_key::BackdropKey;
use tinyvec::ArrayVec;

/// The backdrops a frame must capture again, as damage found them: content painted before each
/// changed under its footprint. More than [`Self::CAP`] makes the frame whole, which captures
/// every backdrop.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct DirtyBackdrops {
    keys: ArrayVec<[BackdropKey; Self::CAP]>,
}

impl DirtyBackdrops {
    pub(crate) const CAP: usize = 8;

    pub(crate) fn contains(&self, key: BackdropKey) -> bool {
        self.keys.contains(&key)
    }

    pub(super) fn clear(&mut self) {
        self.keys.clear();
    }

    /// Add `key`; `false` when the set is full.
    pub(super) fn push(&mut self, key: BackdropKey) -> bool {
        self.keys.try_push(key).is_none()
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::damage::dirty_backdrops::DirtyBackdrops;
    use crate::primitives::identity::backdrop_key::BackdropKey;

    /// A set holding `keys`, as a frame's damage would; at most [`DirtyBackdrops::CAP`].
    impl FromIterator<BackdropKey> for DirtyBackdrops {
        fn from_iter<I: IntoIterator<Item = BackdropKey>>(keys: I) -> Self {
            let mut set = Self::default();
            for key in keys {
                assert!(set.push(key), "at most {} dirty backdrops", Self::CAP);
            }
            set
        }
    }
}
