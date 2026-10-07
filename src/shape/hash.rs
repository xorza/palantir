//! Canonical per-[`ShapeRecord`] hash. [`compute_record_hash`] fills the parallel
//! `Shapes::hashes` arena (`Shapes::add`) and is pinned by tests;
//! `Tree::compute_rollups` and damage diff read those `ContentHash`es, and
//! production code never rehashes records. The schedule is `discriminant →
//! per-variant fields` at every nesting level (`mem::discriminant`, so it cannot
//! drift from the enum). Nothing is persisted, so variants can be added or
//! reordered freely.

use crate::common::content_hash::ContentHash;
use crate::common::hash::Hasher;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::math::float_hash::FloatHash;
use crate::primitives::paint::image::ImageFit;
use crate::shape::paint::curve_basis::CurveBasis;
use crate::shape::paint::image_source::ImageSource;
use crate::shape::paint::quad_shape::QuadShape;
use crate::shape::paint::shape_brush::BrushHash;
use crate::shape::paint::shape_brush::CurveRamp;
use crate::shape::paint::shape_brush::ShapeBrush;
use crate::shape::record::ShapeRecord;
use std::hash::{Hash, Hasher as _};
use std::mem;

/// Hash a fully-lowered `ShapeRecord` into a `ContentHash`. `Polyline` and `Mesh`
/// hand over a `content_hash` their lowering computed, as their payload bytes live
/// in the `RecordStore` this function never sees.
pub(crate) fn compute_record_hash(record: &ShapeRecord) -> ContentHash {
    let mut h = Hasher::new();
    mem::discriminant(record).hash(&mut h);
    match record {
        // All three shapes share this record's discriminant, so `QuadShape`'s goes
        // in first to keep a rectangle, shadow and triangle apart.
        ShapeRecord::Quad(shape) => {
            mem::discriminant(shape).hash(&mut h);
            match shape {
                QuadShape::Rect {
                    kind,
                    local_rect,
                    corners,
                    fill,
                    border,
                } => {
                    h.write_u8(*kind as u8);
                    hash_optional_rect(*local_rect, &mut h);
                    corners.hash(&mut h);
                    hash_brush(*fill, &mut h);
                    border.hash_into(&mut h);
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
                // `bbox` derives from `a`/`b`/`c` + `radius`, so it is excluded.
                QuadShape::Triangle {
                    a,
                    b,
                    c,
                    radius,
                    fill,
                    border,
                    bbox: _,
                } => {
                    a.hash_visual(&mut h);
                    b.hash_visual(&mut h);
                    c.hash_visual(&mut h);
                    radius.hash_visual(&mut h);
                    fill.hash(&mut h);
                    border.hash_into(&mut h);
                }
            }
        }
        // `content_hash` already folds width, color_mode, cap, join, points and
        // colors; bbox/spans are frame-local. Fields are named rather than `..` so
        // the compiler rejects a *new* one until someone decides if it belongs in
        // the hash: a missing field makes two records share a hash, which damage
        // diff reads as "unchanged".
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
        // The shaping inputs are layout's as well as paint's, so one method feeds
        // both hashes.
        ShapeRecord::Text {
            local_origin,
            color,
            text: _,
            font: _,
            wrap: _,
            align: _,
        } => {
            match local_origin {
                None => h.write_u8(0),
                Some(origin) => {
                    h.write_u8(1);
                    origin.hash_visual(&mut h);
                }
            }
            color.hash(&mut h);
            record.hash_layout_inputs(&mut h);
        }
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
        // `ImageSource`'s discriminant goes in first to keep a texture draw and a
        // view composite apart; the shared placement fields are hashed once, around
        // the split.
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
                // The registration `id`, the intrinsic `size`, and the write count
                // that moves the hash when texels change under an unchanged id.
                ImageSource::Texture {
                    id,
                    size,
                    generation,
                } => {
                    h.write_u64(id.0);
                    h.write_u64(u64::from(size.x) | (u64::from(size.y) << 32));
                    h.write_u32(*generation);
                }
                // `epoch` is the view's damage version: `Ui::gpu_view` bumps it to
                // the frame id on `repaint(true)` (hash changes, the rect repaints)
                // and holds it on `repaint(false)` (hash matches, the view culls).
                // The view's paint lives in `Ui::gpu_views`, which the hash cannot
                // see.
                ImageSource::GpuView { epoch } => h.write_u64(*epoch),
            }
            // The fit (including `Tile`'s UV transform, which changes every
            // pan/zoom frame), both sampling filters and the minification tap mode:
            // two 1-bit filters in the low bits, the 3-variant `downsample` above.
            hash_fit(fit, &mut h);
            h.write_u8(
                (*min_filter as u8) | ((*mag_filter as u8) << 1) | ((*downsample as u8) << 2),
            );
        }
        // The handle's `view_box` is baked and constant per `(set, icon)`, so
        // identity plus rect, fit and tint is all that can change; `IconSetId`
        // carries a generation, so a reused slot never names two artworks. The
        // raster size is *not* hashed: it follows from the resolved screen rect and
        // needs a display scale the record lacks.
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
        // Geometry + style are hashed inline (every input lives on the record).
        // `bbox` is excluded. The ramp goes in by content hash so equal geometry
        // with different ramps differs. `CurveBasis`'s discriminant goes in first
        // to keep a cubic and an arc apart; the shared stroke fields are hashed
        // after the split.
        ShapeRecord::Curve {
            cap,
            basis,
            stroke,
            bbox: _,
            ramp,
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
            stroke.hash_into(&mut h);
            match ramp {
                CurveRamp::None => h.write_u8(*cap as u8),
                CurveRamp::Interned { id: _, hash } => {
                    h.write_u8(*cap as u8 | 0x80);
                    h.write_u64(*hash);
                }
            }
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

/// Fold a lowered fill into the shape hash via [`ShapeBrush::hash_parts`], which
/// the chrome hash reads too.
fn hash_brush(fill: ShapeBrush, h: &mut Hasher) {
    let BrushHash { tag, payload } = fill.hash_parts();
    h.write_u8(tag);
    h.write_u64(payload);
}

/// Fold an [`ImageFit`] into the shape hash: the discriminant plus, for `Tile`, the
/// UV transform bits.
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
    use crate::common::span::Span;
    use crate::primitives::layout::align::Align;
    use crate::primitives::paint::color::RgbaF32;
    use crate::primitives::text::recorded_text::RecordedText;
    use crate::shape::hash::compute_record_hash;
    use crate::shape::record::ShapeRecord;
    use crate::text::font_family::FontFamily;
    use crate::text::font_slant::FontSlant;
    use crate::text::font_weight::FontWeight;
    use crate::text::glyph_font::GlyphFont;
    use crate::text::wrap::TextWrap;

    fn text_shape(
        line_height: f32,
        weight: FontWeight,
        local_origin: Option<glam::Vec2>,
    ) -> ShapeRecord {
        text_face(
            GlyphFont {
                size: 16.0,
                line_height,
                family: FontFamily::SANS,
                weight,
                slant: FontSlant::Normal,
            },
            local_origin,
        )
    }

    fn text_face(font: GlyphFont, local_origin: Option<glam::Vec2>) -> ShapeRecord {
        ShapeRecord::Text {
            local_origin,
            text: RecordedText::new(Span::default(), hash_str("hi")),
            color: RgbaF32::WHITE.into(),
            font,
            wrap: TextWrap::Truncate,
            align: Align::default(),
        }
    }

    fn hash_shape(s: &ShapeRecord) -> u64 {
        compute_record_hash(s).0
    }

    /// Pin: every authoring-relevant `ShapeRecord::Text` field participates in the
    /// hash so caches invalidate on a change. New fields go in the table.
    #[test]
    fn text_shape_hash_distinguishes_each_authoring_field() {
        let (regular, bold) = (FontWeight::REGULAR, FontWeight::BOLD);
        let o_a = Some(glam::Vec2::new(0.0, 0.0));
        let o_b = Some(glam::Vec2::new(5.0, 5.0));
        let upright = GlyphFont::new(16.0);
        let cases: [(&str, ShapeRecord, ShapeRecord); 6] = [
            (
                "line_height",
                text_shape(16.0 * 1.2, regular, None),
                text_shape(16.0 * 1.5, regular, None),
            ),
            (
                "weight regular vs bold",
                text_shape(19.2, regular, None),
                text_shape(19.2, bold, None),
            ),
            (
                "local_origin None vs Some",
                text_shape(19.2, regular, None),
                text_shape(19.2, regular, o_a),
            ),
            (
                "local_origin Some(a) vs Some(b)",
                text_shape(19.2, regular, o_a),
                text_shape(19.2, regular, o_b),
            ),
            (
                "family sans vs mono",
                text_face(upright, None),
                text_face(
                    GlyphFont {
                        family: FontFamily::MONO,
                        ..upright
                    },
                    None,
                ),
            ),
            (
                "style upright vs italic",
                text_face(upright, None),
                text_face(
                    GlyphFont {
                        slant: FontSlant::Italic,
                        ..upright
                    },
                    None,
                ),
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

    /// Identical shapes hash identically (no `RandomState`).
    #[test]
    fn text_shape_hash_matches_when_inputs_match() {
        assert_eq!(
            hash_shape(&text_shape(19.2, FontWeight::REGULAR, None)),
            hash_shape(&text_shape(19.2, FontWeight::REGULAR, None)),
        );
    }
}
