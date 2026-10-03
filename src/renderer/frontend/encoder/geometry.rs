//! How an owner-relative rect lands on its owner.

use crate::primitives::rect::Rect;

/// Resolve a shape's owner-relative `local_rect` against the owner's
/// arranged rect. `None` means "paint the owner's full rect"; `Some(lr)`
/// offsets `lr` by the owner's origin. Shared by the rectangle /
/// `Image` arms so the offset convention can't drift.
#[inline]
pub(super) fn resolve_local_rect(owner_rect: Rect, local_rect: Option<Rect>) -> Rect {
    match local_rect {
        None => owner_rect,
        Some(lr) => Rect {
            min: owner_rect.min + lr.min,
            size: lr.size,
        },
    }
}
