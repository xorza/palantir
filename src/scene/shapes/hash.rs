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
use crate::primitives::approx::FloatHash;
use crate::primitives::image::ImageFit;
use crate::primitives::rect::Rect;
use crate::scene::shapes::paint::{BrushHash, CurveBasis, ImageSource, QuadShape, ShapeBrush};
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
                    a.hash_visual(&mut h);
                    b.hash_visual(&mut h);
                    c.hash_visual(&mut h);
                    radius.hash_visual(&mut h);
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
            font,
            wrap,
            align,
        } => {
            match local_origin {
                None => h.write_u8(0),
                Some(origin) => {
                    h.write_u8(1);
                    origin.hash_visual(&mut h);
                }
            }
            text.hash(&mut h);
            color.hash(&mut h);
            font.size_px.hash_visual(&mut h);
            font.line_height_px.hash_visual(&mut h);
            // `weight` rides the free high byte of `style`; `align`/`wrap`/
            // `family` occupy bytes 2/1/0, so bold vs regular can't collide
            // in the node hash (would break damage/reuse).
            let style = ((font.weight as u32) << 24)
                | ((align.raw() as u32) << 16)
                | ((*wrap as u32) << 8)
                | (font.family as u32);
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
        // The handle's `view_box` is baked data — constant for a given
        // `(set, icon)` — so identity plus the rect, fit and tint is the
        // whole of what can change. Constant because `IconSetId` carries a
        // generation: a slot reused by another set answers to a different
        // id, so one `(slot, icon)` pair can never name two artworks. The
        // raster size is *not* hashed: it is a
        // function of the resolved screen rect, which the paint bound already
        // tracks, and folding it in would need the display scale the record
        // does not carry.
        ShapeRecord::Icon {
            local_rect,
            handle,
            fit,
            tint,
            desaturate,
        } => {
            hash_optional_rect(*local_rect, &mut h);
            tint.hash(&mut h);
            h.write_u32(handle.icon.set.bits());
            h.write_u16(handle.icon.icon.0);
            h.write_u8((*fit as u8) | (u8::from(*desaturate) << 2));
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
                        point.hash_visual(&mut h);
                    }
                }
                CurveBasis::Arc {
                    center,
                    radius,
                    a0,
                    a1,
                } => {
                    center.hash_visual(&mut h);
                    radius.hash_visual(&mut h);
                    a0.hash_visual(&mut h);
                    a1.hash_visual(&mut h);
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
            rect.hash_visual(h);
        }
    }
}

/// Fold a lowered fill into the shape hash. The two values come off
/// [`ShapeBrush::hash_parts`], which the chrome hash reads too.
fn hash_brush(fill: &ShapeBrush, fill_grad_hash: u64, h: &mut Hasher) {
    let BrushHash { tag, payload } = fill.hash_parts(fill_grad_hash);
    h.write_u8(tag);
    h.write_u64(payload);
}

/// Fold an [`ImageFit`] into the shape hash: the discriminant plus, for
/// `Tile`, the UV transform bits (these vary per pan/zoom frame, so
/// they must drive a repaint). The other variants carry no payload.
fn hash_fit(fit: &ImageFit, h: &mut Hasher) {
    mem::discriminant(fit).hash(h);
    if let ImageFit::Tile { offset, scale } = fit {
        offset.hash_visual(h);
        scale.hash_visual(h);
    }
}

#[cfg(test)]
mod tests {
    use crate::common::hash::hash_str;
    use crate::layout::types::align::Align;
    use crate::primitives::color::Color;
    use crate::primitives::recorded_text::RecordedText;
    use crate::primitives::span::Span;
    use crate::scene::shapes::hash::compute_record_hash;
    use crate::scene::shapes::record::ShapeRecord;
    use crate::text::glyph_font::GlyphFont;
    use crate::text::wrap::TextWrap;
    use crate::text::{FontFamily, FontWeight};

    fn text_shape(
        line_height_px: f32,
        weight: FontWeight,
        local_origin: Option<glam::Vec2>,
    ) -> ShapeRecord {
        ShapeRecord::Text {
            local_origin,
            text: RecordedText::new(Span::default(), hash_str("hi")),
            color: Color::WHITE.into(),
            font: GlyphFont {
                size_px: 16.0,
                line_height_px,
                family: FontFamily::Sans,
                weight,
            },
            wrap: TextWrap::Truncate,
            align: Align::default(),
        }
    }

    fn hash_shape(s: &ShapeRecord) -> u64 {
        compute_record_hash(s).0
    }

    /// Pin: every authoring-relevant `ShapeRecord::Text` field participates
    /// in the node hash so layout and paint caches invalidate when text
    /// metrics, appearance, or position changes. New fields go in the table,
    /// not in a new test.
    #[test]
    fn text_shape_hash_distinguishes_each_authoring_field() {
        use FontWeight::{Bold, Regular};
        let o_a = Some(glam::Vec2::new(0.0, 0.0));
        let o_b = Some(glam::Vec2::new(5.0, 5.0));
        let cases: [(&str, ShapeRecord, ShapeRecord); 4] = [
            (
                "line_height_px",
                text_shape(16.0 * 1.2, Regular, None),
                text_shape(16.0 * 1.5, Regular, None),
            ),
            (
                "weight Regular vs Bold",
                text_shape(19.2, Regular, None),
                text_shape(19.2, Bold, None),
            ),
            (
                "local_origin None vs Some",
                text_shape(19.2, Regular, None),
                text_shape(19.2, Regular, o_a),
            ),
            (
                "local_origin Some(a) vs Some(b)",
                text_shape(19.2, Regular, o_a),
                text_shape(19.2, Regular, o_b),
            ),
        ];
        for (label, a, b) in cases {
            assert_ne!(
                hash_shape(&a),
                hash_shape(&b),
                "case `{label}`: distinct fields must hash differently",
            );
        }
    }

    /// Sanity counterpart: identical shapes hash identically (guards
    /// against accidental non-determinism, e.g. a future field
    /// hashed via a `RandomState` or rand call).
    #[test]
    fn text_shape_hash_matches_when_inputs_match() {
        assert_eq!(
            hash_shape(&text_shape(19.2, FontWeight::Regular, None)),
            hash_shape(&text_shape(19.2, FontWeight::Regular, None)),
        );
    }
}
