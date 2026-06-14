//! Canonical per-[`ShapeRecord`] hash. One entry point —
//! [`compute_record_hash`] — used by `Shapes::add` to populate the
//! parallel `Shapes::hashes` arena, and by tests that pin the hash
//! schedule. `Tree::compute_hashes` and damage diff both read those
//! precomputed `NodeHash`es; no production code rehashes records.
//!
//! The schedule is `discriminant byte → per-variant fields`. Stable
//! discriminants come from the explicit `= N` annotations on
//! [`ShapeRecord`].

use crate::common::hash::Hasher;
use crate::forest::rollups::NodeHash;
use crate::forest::shapes::record::{ShapeBrush, ShapeRecord};
use crate::primitives::image::ImageFit;
use std::hash::{Hash, Hasher as _};

/// Hash a fully-lowered `ShapeRecord` into a stable `NodeHash`.
/// Sole public entry; the production call site is `Shapes::add`,
/// which pushes the result onto the parallel `Shapes::hashes` arena.
pub(crate) fn compute_record_hash(record: &ShapeRecord) -> NodeHash {
    let mut h = Hasher::new();
    h.write_u8(record.tag());
    match record {
        ShapeRecord::RoundedRect {
            local_rect,
            corners,
            fill,
            stroke,
            fill_grad_hash,
        } => {
            match local_rect {
                None => h.write_u8(0),
                Some(r) => {
                    h.write_u8(1);
                    r.hash(&mut h);
                }
            }
            corners.hash(&mut h);
            match fill {
                ShapeBrush::Solid(c) => {
                    h.write_u8(0);
                    c.hash(&mut h);
                }
                ShapeBrush::Gradient(_) => {
                    h.write_u8(1);
                    h.write_u64(*fill_grad_hash);
                }
            }
            // Pod-byte hash for `(color, width)` — one `write()` dispatch.
            h.write(bytemuck::bytes_of(stroke));
        }
        // `content_hash` already folds width + color_mode + cap + join
        // + points + colors; bbox/spans are frame-local and excluded.
        ShapeRecord::Polyline { content_hash, .. } => h.write_u64(*content_hash),
        ShapeRecord::Text {
            local_origin,
            text: _,
            text_hash,
            color,
            font_size_px,
            line_height_px,
            wrap,
            align,
            family,
        } => {
            match local_origin {
                None => h.write_u8(0),
                Some(o) => {
                    h.write_u8(1);
                    h.pod(o);
                }
            }
            h.write_u64(*text_hash);
            color.hash(&mut h);
            let dims = ((font_size_px.to_bits() as u64) << 32) | line_height_px.to_bits() as u64;
            h.write_u64(dims);
            let style = ((align.raw() as u32) << 16) | ((*wrap as u32) << 8) | (*family as u32);
            h.write_u32(style);
        }
        ShapeRecord::Mesh {
            local_rect,
            tint,
            content_hash,
            ..
        } => {
            match local_rect {
                None => h.write_u8(0),
                Some(r) => {
                    h.write_u8(1);
                    r.hash(&mut h);
                }
            }
            tint.hash(&mut h);
            h.write_u64(*content_hash);
        }
        ShapeRecord::Shadow {
            local_rect,
            corners,
            shadow,
        } => {
            match local_rect {
                None => h.write_u8(0),
                Some(r) => {
                    h.write_u8(1);
                    r.hash(&mut h);
                }
            }
            corners.hash(&mut h);
            shadow.hash(&mut h);
        }
        ShapeRecord::Image {
            local_rect,
            tint,
            id,
            size,
            fit,
        } => {
            match local_rect {
                None => h.write_u8(0),
                Some(r) => {
                    h.write_u8(1);
                    r.hash(&mut h);
                }
            }
            tint.hash(&mut h);
            // Hash the registration `id` + intrinsic `size` (packed
            // `x | y`), then fold in the fit (incl. `Tile`'s UV transform,
            // which changes every pan/zoom frame and must repaint).
            h.write_u64(id.0);
            h.write_u64((size.x as u64) | ((size.y as u64) << 16));
            hash_fit(fit, &mut h);
        }
        // `content_hash` summarizes p0..p3 + width + cap + (solid)
        // inline colour. Brush variant folded in separately so curves
        // with the same geometry but different fills don't collide.
        ShapeRecord::Curve {
            content_hash,
            fill,
            fill_grad_hash,
            ..
        } => {
            h.write_u64(*content_hash);
            match fill {
                ShapeBrush::Solid(c) => {
                    h.write_u8(0);
                    c.hash(&mut h);
                }
                ShapeBrush::Gradient(_) => {
                    h.write_u8(1);
                    h.write_u64(*fill_grad_hash);
                }
            }
        }
    }
    NodeHash(h.finish())
}

/// Fold an [`ImageFit`] into the shape hash: a discriminant tag plus,
/// for `Tile`, the UV transform bits (these vary per pan/zoom frame, so
/// they must drive a repaint). The other variants carry no payload.
fn hash_fit(fit: &ImageFit, h: &mut Hasher) {
    let tag = match fit {
        ImageFit::Fill => 0u8,
        ImageFit::Contain => 1,
        ImageFit::Cover => 2,
        ImageFit::None => 3,
        ImageFit::Tile { .. } => 4,
    };
    h.write_u8(tag);
    if let ImageFit::Tile { offset, scale } = fit {
        h.pod(&[offset.x, offset.y, scale.x, scale.y]);
    }
}
