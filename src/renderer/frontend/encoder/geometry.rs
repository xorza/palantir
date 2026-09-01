//! The arithmetic an encoded shape's paint rect is resolved with: how an
//! owner-relative rect lands on its owner, how a spinning stroke is bounded,
//! and how an icon or image is fitted into the box it paints.

use crate::primitives::image::ImageFit;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::shape::icon::IconFit;
use glam::Vec2;

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

/// Map an icon's `(base, view_box, fit)` onto the logical-px rect it paints.
///
/// Sibling of [`resolve_fit`] and deliberately smaller: an icon has no UV to
/// crop, so every mode is a rect and nothing else. A degenerate viewBox falls
/// through to the base rect — the same fail-safe the image path takes for a
/// missing intrinsic size.
pub(super) fn resolve_icon_fit(base: Rect, view_box: Vec2, fit: IconFit) -> Rect {
    // Through the image resolver, not a second copy of it: the three
    // variants an icon can express mean exactly what they mean for an
    // image, and an icon needs only the rect half of the answer (it
    // rasterizes to its box, so there is no UV to crop).
    resolve_fit(base, view_box, fit.to_image_fit()).rect
}

/// A `w`x`h` box centred inside `base`. Every aspect-preserving fit resolves
/// through here — icon and image alike — so the three of them cannot disagree
/// about where the leftover space goes.
fn centered_in(base: Rect, w: f32, h: f32) -> Rect {
    Rect {
        min: base.min + Vec2::new((base.size.w - w) * 0.5, (base.size.h - h) * 0.5),
        size: Size { w, h },
    }
}

/// Output of [`resolve_fit`]: the final paint rect + UV crop the
/// encoder hands to the sink.
#[derive(Debug)]
pub(super) struct Resolved {
    pub(super) rect: Rect,
    pub(super) uv_min: Vec2,
    pub(super) uv_size: Vec2,
}

const FULL_UV_MIN: Vec2 = Vec2::ZERO;
const FULL_UV_SIZE: Vec2 = Vec2::ONE;

/// Map `(base, image_size, fit)` → `(paint_rect, uv_crop)`. `base` is
/// the encoder-resolved paint rect (owner rect or local override).
/// `image_size = UVec2::ZERO` (missing registry entry at lowering time)
/// falls through to the base rect with full UV — the backend's
/// lookup-miss branch then skips the actual draw.
pub(super) fn resolve_fit(base: Rect, image_size: Vec2, fit: ImageFit) -> Resolved {
    let iw = image_size.x;
    let ih = image_size.y;
    let bw = base.size.w;
    let bh = base.size.h;
    if iw <= 0.0 || ih <= 0.0 || bw <= 0.0 || bh <= 0.0 {
        return Resolved {
            rect: base,
            uv_min: FULL_UV_MIN,
            uv_size: FULL_UV_SIZE,
        };
    }
    match fit {
        ImageFit::Fill => Resolved {
            rect: base,
            uv_min: FULL_UV_MIN,
            uv_size: FULL_UV_SIZE,
        },
        ImageFit::Contain => {
            // Preserve aspect; the smaller axis ratio decides scale.
            let scale = (bw / iw).min(bh / ih);
            Resolved {
                rect: centered_in(base, iw * scale, ih * scale),
                uv_min: FULL_UV_MIN,
                uv_size: FULL_UV_SIZE,
            }
        }
        ImageFit::Cover => {
            // Preserve aspect; the larger axis ratio decides scale —
            // image overhangs the rect. Crop the overhang via UV
            // (centered, so visible texels match `Contain`'s axis).
            let scale = (bw / iw).max(bh / ih);
            let w_phys = iw * scale; // >= bw
            let h_phys = ih * scale; // >= bh
            let uv_w = bw / w_phys; // <= 1
            let uv_h = bh / h_phys; // <= 1
            Resolved {
                rect: base,
                uv_min: Vec2::new((1.0 - uv_w) * 0.5, (1.0 - uv_h) * 0.5),
                uv_size: Vec2::new(uv_w, uv_h),
            }
        }
        ImageFit::None => {
            // Paint at intrinsic px, centered. An image larger than
            // `base` overflows it, uncropped.
            Resolved {
                rect: centered_in(base, iw, ih),
                uv_min: FULL_UV_MIN,
                uv_size: FULL_UV_SIZE,
            }
        }
        // Raw caller-driven UV; the shader wraps with `fract`. The
        // intrinsic image size is irrelevant — `scale`/`offset` already
        // express the repeat count and phase against the full rect.
        ImageFit::Tile { offset, scale } => Resolved {
            rect: base,
            uv_min: offset,
            uv_size: scale,
        },
    }
}
