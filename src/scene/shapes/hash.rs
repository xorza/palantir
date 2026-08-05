//! Canonical per-[`ShapeRecord`] hash. One entry point —
//! [`compute_record_hash`] — used by `Shapes::add` to populate the
//! parallel `Shapes::hashes` arena, and by tests that pin the hash
//! schedule. `Tree::compute_rollups` and damage diff both read those
//! precomputed `ContentHash`es; no production code rehashes records.
//!
//! The schedule is `discriminant → per-variant fields`, at every level
//! of the nesting. `mem::discriminant` writes the discriminant, so it
//! cannot drift from the enum it describes. Nothing here is persisted —
//! a `ContentHash` is only ever compared against another produced in the
//! same process run — so there is no numbering to hold stable and
//! variants can be added or reordered freely.

use crate::common::content_hash::ContentHash;
use crate::common::hash::Hasher;
use crate::primitives::approx;
use crate::primitives::image::ImageFit;
use crate::primitives::rect::Rect;
use crate::scene::shapes::paint::{CurveBasis, ImageSource, QuadShape, ShapeBrush};
use crate::scene::shapes::record::ShapeRecord;
use std::hash::{Hash, Hasher as _};
use std::mem;

/// Hash a fully-lowered `ShapeRecord` into a stable `ContentHash`.
/// Sole public entry; the production call site is `Shapes::add`,
/// which pushes the result onto the parallel `Shapes::hashes` arena.
pub(crate) fn compute_record_hash(record: &ShapeRecord) -> ContentHash {
    let mut h = Hasher::new();
    mem::discriminant(record).hash(&mut h);
    match record {
        // All three shapes share this record's discriminant, so
        // `QuadShape`'s goes in ahead of the per-shape fields to keep a
        // rectangle, a shadow, and a triangle apart — without it a rect
        // and a shadow over the same rounded box would differ only by
        // fields their schedules happen not to share.
        ShapeRecord::Quad(shape) => {
            mem::discriminant(shape).hash(&mut h);
            match shape {
                QuadShape::Rect {
                    kind,
                    local_rect,
                    corners,
                    fill,
                    stroke,
                    fill_grad_hash,
                } => {
                    h.write_u8(*kind as u8);
                    hash_optional_rect(*local_rect, &mut h);
                    corners.hash(&mut h);
                    hash_brush(fill, *fill_grad_hash, &mut h);
                    // Pod-byte hash for `(color, width)` — one dispatch.
                    h.pod(stroke);
                }
                QuadShape::Shadow {
                    local_rect,
                    corners,
                    shadow,
                } => {
                    hash_optional_rect(*local_rect, &mut h);
                    corners.hash(&mut h);
                    shadow.hash(&mut h);
                }
                // `bbox` is derived from `a`/`b`/`c` + `radius`, so it's
                // excluded — the geometry that determines it is already
                // hashed.
                QuadShape::Triangle {
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
                    h.pod(stroke);
                }
            }
        }
        // `content_hash` already folds width + color_mode + cap + join
        // + points + colors; bbox/spans are frame-local and excluded.
        //
        // Spelled out rather than `..`: naming every field is what makes
        // the compiler reject a *new* one until someone decides whether
        // it belongs in the hash. `..` would absorb it silently, and a
        // field missing from the hash is two records sharing one — which
        // damage diff reads as "unchanged" and skips the repaint.
        ShapeRecord::Polyline {
            content_hash,
            width: _,
            color_mode: _,
            cap: _,
            join: _,
            points: _,
            colors: _,
            bbox: _,
        } => h.write_u64(*content_hash),
        ShapeRecord::Text {
            local_origin,
            text,
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
            text.hash(&mut h);
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
        // Fields named exhaustively for the reason given on the
        // `Polyline` arm above.
        ShapeRecord::Mesh {
            local_rect,
            tint,
            content_hash,
            vertices: _,
            indices: _,
            bbox: _,
        } => {
            hash_optional_rect(*local_rect, &mut h);
            tint.hash(&mut h);
            h.write_u64(*content_hash);
        }
        // Both sources share this record's discriminant, so
        // `ImageSource`'s goes in ahead of the source fields to keep a
        // texture draw and a view composite apart; the placement fields
        // they share are hashed once, around the split.
        ShapeRecord::Image {
            local_rect,
            tint,
            source,
            fit,
            min_filter,
            mag_filter,
            downsample,
        } => {
            hash_optional_rect(*local_rect, &mut h);
            tint.hash(&mut h);
            mem::discriminant(source).hash(&mut h);
            match source {
                // The registration `id` + intrinsic `size`.
                ImageSource::Texture { id, size } => {
                    h.write_u64(id.0);
                    h.write_u64(u64::from(size.x) | (u64::from(size.y) << 32));
                }
                // `epoch` is the view's damage version: `Ui::gpu_view` bumps
                // it to the frame id on `repaint(true)` (hash changes → the
                // rect repaints and the texture re-renders) and holds it
                // stable on `repaint(false)` (hash matches → the view culls).
                // The view's id + paint live in `Ui::gpu_views`, which the
                // hash can't see; `epoch` rides the record precisely so this
                // stays correct.
                ImageSource::GpuView { epoch } => h.write_u64(*epoch),
            }
            // The fit (incl. `Tile`'s UV transform, which changes every
            // pan/zoom frame and must repaint), both sampling filters, and the
            // minification tap mode — one byte, two 1-bit filters in the low
            // bits and the 3-variant `downsample` above them.
            hash_fit(fit, &mut h);
            h.write_u8(
                (*min_filter as u8) | ((*mag_filter as u8) << 1) | ((*downsample as u8) << 2),
            );
        }
        // Geometry + style hashed inline — every input lives on the
        // record, so no lowering-time content hash is needed (unlike
        // `Polyline`/`Mesh`, whose payload bytes live in the record store).
        // `bbox` derives from geometry + width + cap and is excluded.
        // Brush folded separately so strokes with the same geometry
        // but different fills don't collide. Both bases share this
        // record's discriminant, so `CurveBasis`'s goes in ahead of the
        // basis fields to keep a cubic and an arc apart; the stroke
        // fields they share are hashed once, after the split.
        ShapeRecord::Curve {
            basis,
            width,
            fill,
            fill_grad_hash,
            cap,
            bbox: _,
        } => {
            mem::discriminant(basis).hash(&mut h);
            match basis {
                CurveBasis::Cubic { p0, p1, p2, p3 } => {
                    for point in [p0, p1, p2, p3] {
                        approx::hash_visual_vec2(*point, &mut h);
                    }
                }
                CurveBasis::Arc {
                    center,
                    radius,
                    a0,
                    a1,
                } => {
                    approx::hash_visual_vec2(*center, &mut h);
                    approx::hash_visual_f32(*radius, &mut h);
                    approx::hash_visual_f32(*a0, &mut h);
                    approx::hash_visual_f32(*a1, &mut h);
                }
            }
            h.write_u64((u64::from(approx::canon_bits(*width)) << 8) | u64::from(*cap as u8));
            hash_brush(fill, *fill_grad_hash, &mut h);
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

/// Fold a lowered fill into the shape hash: discriminant, then the
/// inline colour for `Solid` or the pre-computed gradient content
/// hash for `Gradient` (the `GradientId` itself is frame-local and
/// excluded).
fn hash_brush(fill: &ShapeBrush, fill_grad_hash: u64, h: &mut Hasher) {
    mem::discriminant(fill).hash(h);
    match fill {
        ShapeBrush::Solid(c) => c.hash(h),
        ShapeBrush::Gradient(_) => h.write_u64(fill_grad_hash),
    }
}

/// Fold an [`ImageFit`] into the shape hash: the discriminant plus, for
/// `Tile`, the UV transform bits (these vary per pan/zoom frame, so
/// they must drive a repaint). The other variants carry no payload.
fn hash_fit(fit: &ImageFit, h: &mut Hasher) {
    mem::discriminant(fit).hash(h);
    if let ImageFit::Tile { offset, scale } = fit {
        approx::hash_visual_vec2(*offset, h);
        approx::hash_visual_vec2(*scale, h);
    }
}
