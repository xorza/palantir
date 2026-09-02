//! A split's address, packed into one byte.

use serde::{Deserialize, Serialize};

/// A split's address: the turns taken from the root, packed into one
/// byte — a leading sentinel bit, then one bit per level (`0` = first
/// child, `1` = second). The root split is the bare sentinel. One `Copy`
/// byte instead of a `Vec<bool>`, with capacity for 7 levels, which
/// [`DockState::max_depth`](crate::DockState::max_depth) keeps real
/// trees well inside.
///
/// Like any address into the tree it is only stable between structural
/// changes; a stale path that no longer lands on a split is ignored by
/// the op it feeds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DockPath(u8);

impl DockPath {
    /// The root node's address (the empty path).
    pub const ROOT: DockPath = DockPath(1);

    /// The most levels the packed byte can address.
    pub(crate) const CAPACITY: u32 = 7;

    /// The address of `self`'s first (left or top) child.
    pub fn first(self) -> DockPath {
        self.child(false)
    }

    /// The address of `self`'s second (right or bottom) child.
    pub fn second(self) -> DockPath {
        self.child(true)
    }

    fn child(self, second: bool) -> DockPath {
        assert!(
            self.0 < 0x80,
            "dock path capacity (7 levels) exceeded — the depth cap should stop far earlier"
        );
        DockPath((self.0 << 1) | second as u8)
    }

    /// Whether the byte carries no sentinel bit — a corrupt address
    /// rather than the root, reachable only through serde.
    pub(crate) fn is_corrupt(self) -> bool {
        self.0 == 0
    }

    /// Turns from the root, in root-to-leaf order. Saturating, so the
    /// invalid sentinel-less `0` byte yields no turns instead of
    /// underflowing.
    pub(crate) fn directions(self) -> impl Iterator<Item = bool> {
        let depth = Self::CAPACITY.saturating_sub(self.0.leading_zeros());
        (0..depth).rev().map(move |i| (self.0 >> i) & 1 == 1)
    }
}

impl Default for DockPath {
    fn default() -> Self {
        Self::ROOT
    }
}
