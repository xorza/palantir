//! What one node's paint state inherits from its ancestors, as one word.

use crate::common::hash::Hasher;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::math::float_hash::FloatHash;
use std::hash::Hasher as _;

/// Per-node fingerprint of cascade inputs from ancestors (transform, clip, disabled, invisible) plus the node's arranged rect and what its layout read from outside its subtree, packed with the resolved `invisible` bit: a 63-bit `FxHash` with `invisible` in the high bit, read in one 8-byte load by encoder and damage.
///
/// `DamageEngine::compute` skips a subtree when this and `subtree[i]` both match, jumping to `subtree_end[i]`. Packing is sound because `subtree[i]` covers every descendant's `node_hash` (own visibility) and parent invisibility is a hash input.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CascadeInputHash(pub(crate) u64);

const INVISIBLE_BIT: u64 = 1u64 << 63;
const HASH_MASK: u64 = !INVISIBLE_BIT;

impl CascadeInputHash {
    /// Combine a raw 64-bit hash with the resolved `invisible` flag. The top hash bit is masked off first; 63 bits suffice, and branchless avoids a per-node conditional move.
    #[inline]
    pub(crate) const fn pack(hash: u64, invisible: bool) -> Self {
        Self((hash & HASH_MASK) | ((invisible as u64) << 63))
    }

    /// A node's input: its parent's [`CascadeCtx::prefix`](crate::cascade::engine::CascadeCtx::prefix) folded with its own arranged rect and the extent its layout read from outside its subtree ([`PlaneLayout::outside_input`](crate::layout::plane_layout::PlaneLayout::outside_input)), packed with its resolved `invisible`.
    #[inline]
    pub(crate) fn finish(
        prefix: &Hasher,
        layout_rect: Rect,
        outside: Option<Size>,
        invisible: bool,
    ) -> Self {
        let mut h = prefix.clone();
        layout_rect.hash_visual(&mut h);
        if let Some(outside) = outside {
            outside.hash_visual(&mut h);
        }
        Self::pack(h.finish(), invisible)
    }

    #[inline]
    pub(crate) const fn invisible(self) -> bool {
        self.0 & INVISIBLE_BIT != 0
    }
}
