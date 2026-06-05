//! Construction of the per-node `cascade_input` hash — the fingerprint
//! damage pairs with `subtree_hash` to decide "did this node change."
//! Split into an ancestor prefix (folded once per stack frame, cloned
//! per descendant) and a per-node suffix (the arranged rect).

use crate::common::hash::Hasher;
use crate::forest::rollups::CascadeInputHash;
use crate::primitives::rect::Rect;
use crate::primitives::transform::TranslateScale;
use std::hash::Hasher as _;

/// Ancestor-derived portion of the `cascade_input` hash — folded once
/// per stack frame at push time (32 B) and cloned per descendant. Split
/// out from the per-node suffix (`layout_rect`) so a tree-shaped UI
/// avoids re-hashing the parent context on every node.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::NoUninit)]
struct CascadePrefixBytes {
    parent_transform: TranslateScale, // 12B
    clip_rect: Rect,                  // 16B (zeroed when absent)
    clip_present: u8,
    parent_dis: u8,
    parent_inv: u8,
    _pad: u8,
}

#[inline]
pub(crate) fn build_cascade_prefix(
    parent_transform: TranslateScale,
    parent_clip: Option<Rect>,
    parent_dis: bool,
    parent_inv: bool,
) -> Hasher {
    let (clip_rect, clip_present) = match parent_clip {
        Some(c) => (c, 1u8),
        None => (Rect::ZERO, 0u8),
    };
    let packed = CascadePrefixBytes {
        parent_transform,
        clip_rect,
        clip_present,
        parent_dis: parent_dis as u8,
        parent_inv: parent_inv as u8,
        _pad: 0,
    };
    let mut h = Hasher::new();
    h.pod(&packed);
    h
}

#[inline]
pub(crate) fn finish_cascade_input(
    prefix: &Hasher,
    layout_rect: Rect,
    invisible: bool,
) -> CascadeInputHash {
    let mut h = prefix.clone();
    h.pod(&layout_rect);
    CascadeInputHash::pack(h.finish(), invisible)
}
