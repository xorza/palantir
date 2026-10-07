//! How an owner-relative rect lands on its owner.

use crate::primitives::geometry::rect::Rect;

/// Resolve a shape's owner-relative `local_rect` against the owner's arranged rect: `None` is the owner's full rect, `Some(lr)` offsets `lr` by the owner's origin. Shared by the rectangle and `Image` arms.
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
