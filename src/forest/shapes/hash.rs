//! Canonical per-[`ShapeRecord`] hash. One entry point —
//! [`compute_record_hash`] — used by `Shapes::add` to populate the
//! parallel `Shapes::hashes` arena, and by tests that pin the hash
//! schedule. `Tree::compute_hashes` and damage diff both read those
//! precomputed `ContentHash`es; no production code rehashes records.
//!
//! The schedule is `discriminant byte → per-variant fields`. Stable
//! discriminants come from the explicit `= N` annotations on
//! [`ShapeRecord`].

use crate::common::content_hash::ContentHash;
use crate::common::hash::Hasher;
use crate::forest::shapes::paint::ShapeBrush;
use crate::forest::shapes::record::ShapeRecord;
use crate::primitives::approx;
use crate::primitives::image::ImageFit;
use crate::primitives::rect::Rect;
use std::hash::{Hash, Hasher as _};

/// Hash a fully-lowered `ShapeRecord` into a stable `ContentHash`.
/// Sole public entry; the production call site is `Shapes::add`,
/// which pushes the result onto the parallel `Shapes::hashes` arena.
pub(crate) fn compute_record_hash(record: &ShapeRecord) -> ContentHash {
    let mut h = Hasher::new();
    h.write_u8(record.tag());
    match record {
        // WindowedRect shares the field schedule — the tag byte written
        // above keeps the two from colliding.
        ShapeRecord::RoundedRect {
            local_rect,
            corners,
            fill,
            stroke,
            fill_grad_hash,
        }
        | ShapeRecord::WindowedRect {
            local_rect,
            corners,
            fill,
            stroke,
            fill_grad_hash,
        } => {
            hash_optional_rect(*local_rect, &mut h);
            corners.hash(&mut h);
            hash_brush(fill, *fill_grad_hash, &mut h);
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
            weight,
        } => {
            match local_origin {
                None => h.write_u8(0),
                Some(origin) => {
                    h.write_u8(1);
                    approx::hash_visual_vec2(*origin, &mut h);
                }
            }
            h.write_u64(*text_hash);
            color.hash(&mut h);
            approx::hash_visual_f32(*font_size_px, &mut h);
            approx::hash_visual_f32(*line_height_px, &mut h);
            // `weight` rides the free high byte of `style`; `align`/`wrap`/
            // `family` occupy bytes 2/1/0, so bold vs regular can't collide
            // in the node hash (would break damage/reuse).
            let style = ((*weight as u32) << 24)
                | ((align.raw() as u32) << 16)
                | ((*wrap as u32) << 8)
                | (*family as u32);
            h.write_u32(style);
        }
        ShapeRecord::Mesh {
            local_rect,
            tint,
            content_hash,
            ..
        } => {
            hash_optional_rect(*local_rect, &mut h);
            tint.hash(&mut h);
            h.write_u64(*content_hash);
        }
        ShapeRecord::Shadow {
            local_rect,
            corners,
            shadow,
        } => {
            hash_optional_rect(*local_rect, &mut h);
            corners.hash(&mut h);
            shadow.hash(&mut h);
        }
        ShapeRecord::Image {
            local_rect,
            tint,
            id,
            size,
            fit,
            filter,
        } => {
            hash_optional_rect(*local_rect, &mut h);
            tint.hash(&mut h);
            // Hash the registration `id` + intrinsic `size` (packed
            // `x | y`), then fold in the fit (incl. `Tile`'s UV transform,
            // which changes every pan/zoom frame and must repaint) and
            // the sampling filter.
            h.write_u64(id.0);
            h.write_u64((size.x as u64) | ((size.y as u64) << 16));
            hash_fit(fit, &mut h);
            h.write_u8(*filter as u8);
        }
        // Geometry + style hashed inline — every input lives on the
        // record, so no lowering-time content hash is needed (unlike
        // `Polyline`/`Mesh`, whose payload bytes live in the record store).
        // `bbox` derives from geometry + width + cap and is excluded.
        // Brush folded separately so strokes with the same geometry
        // but different fills don't collide; the tag byte above keeps
        // the two kinds apart.
        ShapeRecord::Curve {
            p0,
            p1,
            p2,
            p3,
            width,
            fill,
            fill_grad_hash,
            cap,
            bbox: _,
        } => {
            for point in [p0, p1, p2, p3] {
                approx::hash_visual_vec2(*point, &mut h);
            }
            h.write_u64((u64::from(approx::canon_bits(*width)) << 8) | u64::from(*cap as u8));
            hash_brush(fill, *fill_grad_hash, &mut h);
        }
        ShapeRecord::Arc {
            center,
            radius,
            a0,
            a1,
            width,
            fill,
            fill_grad_hash,
            cap,
            bbox: _,
        } => {
            approx::hash_visual_vec2(*center, &mut h);
            approx::hash_visual_f32(*radius, &mut h);
            approx::hash_visual_f32(*a0, &mut h);
            approx::hash_visual_f32(*a1, &mut h);
            h.write_u64((u64::from(approx::canon_bits(*width)) << 8) | u64::from(*cap as u8));
            hash_brush(fill, *fill_grad_hash, &mut h);
        }
        // `epoch` is the view's damage version: `Ui::gpu_view` bumps it
        // to the frame id on `repaint(true)` (hash changes → the rect
        // repaints and the texture re-renders) and holds it stable on
        // `.repaint(false)` (hash matches → the view culls). The view's
        // id + paint live in `Ui::gpu_views`, which the hash can't see;
        // `epoch` rides the shape precisely so this stays correct.
        ShapeRecord::GpuView { epoch } => {
            h.write_u64(*epoch);
        }
        // `bbox` is derived from `a`/`b`/`c` + `radius`, so it's excluded —
        // the geometry that determines it is already hashed.
        ShapeRecord::Triangle {
            a,
            b,
            c,
            radius,
            fill,
            stroke,
            bbox: _,
        } => {
            approx::hash_visual_vec2(*a, &mut h);
            approx::hash_visual_vec2(*b, &mut h);
            approx::hash_visual_vec2(*c, &mut h);
            approx::hash_visual_f32(*radius, &mut h);
            fill.hash(&mut h);
            h.write(bytemuck::bytes_of(stroke));
        }
    }
    ContentHash(h.finish())
}

fn hash_optional_rect(rect: Option<Rect>, h: &mut Hasher) {
    match rect {
        None => h.write_u8(0),
        Some(rect) => {
            h.write_u8(1);
            approx::hash_visual_rect(rect, h);
        }
    }
}

/// Fold a lowered fill into the shape hash: variant byte, then the
/// inline colour for `Solid` or the pre-computed gradient content
/// hash for `Gradient` (the `GradientId` itself is frame-local and
/// excluded).
fn hash_brush(fill: &ShapeBrush, fill_grad_hash: u64, h: &mut Hasher) {
    match fill {
        ShapeBrush::Solid(c) => {
            h.write_u8(0);
            c.hash(h);
        }
        ShapeBrush::Gradient(_) => {
            h.write_u8(1);
            h.write_u64(fill_grad_hash);
        }
    }
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
        approx::hash_visual_vec2(*offset, h);
        approx::hash_visual_vec2(*scale, h);
    }
}
