//! The lowered form of one paint primitive, and the only shape vocabulary the encoder reads.

use crate::common::content_hash::ContentHash;
use crate::common::hash::Hasher;
use crate::common::span::Span;
use crate::icons::icon_set::IconHandle;
use crate::primitives::geometry::circular_arc::CircularArc;
use crate::primitives::geometry::cubic_bezier::CubicBezier;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::layout::align::Align;
use crate::primitives::math::float_hash::FloatHash;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::image::{ImageDownsample, ImageFilter, ImageFit};
use crate::primitives::text::recorded_text::RecordedText;
use crate::shape::icon::IconFit;
use crate::shape::paint::curve_basis::CurveBasis;
use crate::shape::paint::curve_ramp::CurveRamp;
use crate::shape::paint::image_source::ImageSource;
use crate::shape::paint::quad_shape::QuadShape;
use crate::shape::paint::shape_brush::{BrushHash, ShapeBrush};
use crate::shape::paint::shape_stroke::ShapeStroke;
use crate::shape::style::{LineCap, LineJoin};
use crate::text::glyph_font::GlyphFont;
use crate::text::wrap::TextWrap;
use glam::Vec2;
use std::hash;
use std::hash::{Hash, Hasher as _};
use std::mem;

#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum ColorMode {
    #[default]
    Single = 0,
    PerPoint = 1,
    PerSegment = 2,
}

/// `#[repr(u8)]` pins the discriminant to one byte (`hot_struct_sizes`). Reorder variants freely: hashes use `mem::discriminant` and are only compared within a process run.
#[repr(u8)]
#[derive(Clone, Debug)]
pub(crate) enum ShapeRecord {
    /// Rounded rectangle, box-shadow, or rounded triangle, per [`QuadShape`].
    Quad(QuadShape),
    /// Stroked polyline. `points`/`colors` index the `RecordStore`; `colors` has 1 entry for `Single`, `points.len()` for `PerPoint`, `points.len() - 1` for `PerSegment`. `bbox` is the owner-relative centerline AABB; damage and composition apply the stroke inflation.
    Polyline {
        width: f32,
        color_mode: ColorMode,
        cap: LineCap,
        join: LineJoin,
        points: Span,
        colors: Span,
        bbox: Rect,
        content_hash: u64,
    },
    /// Shaped text run, *authoring inputs only*: measured size and the shaped-buffer key are layout outputs in `Layout.text_shapes`. `wrap` selects shape-once (`Single`) or reshape under a narrower committed width (`Wrap`); `align` positions the glyph bbox (`Auto`/`Stretch` collapse to top-left).
    ///
    /// `None` paints into the owner's rect deflated by `padding`. `Some(origin)` paints at `owner.min + origin` with the shaped measurement as the bbox, so a widget can apply offsets that depend on shaped-buffer state.
    Text {
        local_origin: Option<Vec2>,
        /// Lowered text storage and its content hash; arena-backed input is normalized into the active record store first, so span and hash can't come from different passes.
        text: RecordedText,
        color: RgbaF16,
        /// The face and metrics to shape in, the type [`TextShape`](crate::widget::TextShape) authors and [`TextShapeKey`](crate::text::key::TextShapeKey) is minted from.
        ///
        /// `line_height` is a resolved px leading fed to `Metrics::new` (authoring sets `size * line_height_factor`); carrying px keeps widget conventions out of the shaper and gives different leading distinct buffers.
        font: GlyphFont,
        wrap: TextWrap,
        align: Align,
    },
    /// User-supplied colored triangle mesh. Spans index the `RecordStore`'s `meshes` pool; `content_hash` covers vertex+index bytes.
    Mesh {
        local_rect: Option<Rect>,
        tint: RgbaF16,
        vertices: Span,
        indices: Span,
        /// Owner-local AABB of the vertex positions, frozen from `Mesh::bbox()` so the encoder doesn't re-scan.
        bbox: Rect,
        content_hash: u64,
    },
    /// Textured rectangle: a registered image or a `GpuView` target, per [`ImageSource`]. `local_rect = None` paints the owner's rect, `Some(r)` paints `r` owner-relative. `tint` multiplies in linear-RGB premultiplied space.
    Image {
        local_rect: Option<Rect>,
        tint: RgbaF16,
        source: ImageSource,
        fit: ImageFit,
        min_filter: ImageFilter,
        mag_filter: ImageFilter,
        downsample: ImageDownsample,
    },
    /// A baked SVG icon, rasterized at the exact physical size into the icon atlas. The [`IconHandle`] carries the viewBox, so `fit` needs no registry lookup. `tint` multiplies a tintable icon whole and a colour icon's alpha only, see [`IconShape`](crate::widget::IconShape).
    Icon {
        local_rect: Option<Rect>,
        handle: IconHandle,
        fit: IconFit,
        tint: RgbaF16,
        /// Draws a colour icon as its own luminance, see [`IconShape::desaturate`](crate::widget::IconShape::desaturate).
        desaturate: bool,
    },
    /// Native GPU stroke: a cubic Bézier or exact circular arc, per [`CurveBasis`] (quadratics promote to cubic, lines degenerate; see `CurveShape`'s lowering). Owner-local, no joins. `bbox` is the tight centerline AABB.
    ///
    /// Field order is the layout: this variant fills `ShapeRecord`'s 88 B with the byte-wide `cap` beside the tag and the 8-aligned `ramp` last.
    Curve {
        /// End-cap style; `Round`/`Square` extend the strip by `width/2` past each endpoint.
        cap: LineCap,
        basis: CurveBasis,
        stroke: ShapeStroke,
        bbox: Rect,
        /// The ramp the stroke colour multiplies, sampled along `t` (p0 to p3 for a cubic, a0 to a1 for an arc).
        ramp: CurveRamp,
    },
}

/// Owner-local paint bbox of a [`ShapeRecord::Mesh`]: the vertex hull, which can exceed the owner rect (the owner rect would make partial damage too small). `local_rect` only offsets it.
pub(crate) fn mesh_paint_bbox_local(bbox: Rect, local_rect: Option<Rect>) -> Rect {
    let origin = local_rect.map_or(Vec2::ZERO, |r| r.min);
    Rect {
        min: bbox.min + origin,
        size: bbox.size,
    }
}

/// Tight owner-local paint bbox of a [`ShapeRecord::Text`] from the shaped extent (`LayerLayout::text_shapes`); the encoder uses the same [`Align::place_in`] formula in screen space, so damage and draw rects can't drift.
///
/// **Damage inflation lives in cascade**, not here: the ladder-snap overshoot is `measured × STEP/2` in absolute screen pixels, so it applies after the cascade lifts the rect to screen; locally it would scale by `cascade_scale`.
///
/// - `local_origin: Some(origin)`: rect is `origin + measured`.
/// - `local_origin: None`: placed by [`Align::place_in`] in the owner's padded inner rect.
pub(crate) fn text_paint_bbox_local(
    local_origin: Option<Vec2>,
    align: Align,
    padding: Spacing,
    owner_size: Size,
    measured: Size,
) -> Rect {
    if let Some(origin) = local_origin {
        Rect {
            min: origin,
            size: measured,
        }
    } else {
        let owner_local = Rect {
            min: Vec2::ZERO,
            size: owner_size,
        };
        align.place_in(owner_local.deflated_by(padding), measured)
    }
}

impl ShapeRecord {
    /// The record's canonical content hash, filling the parallel `Shapes::hashes` arena at `Shapes::add`; `Tree::compute_rollups` and damage diff read it, and production code never rehashes a record.
    ///
    /// The schedule is `discriminant → per-variant fields` at every nesting level (`mem::discriminant`, so it cannot drift from the enum). Nothing is persisted, so variants can be added or reordered freely. `Polyline` and `Mesh` hand over a `content_hash` their lowering computed, as their payload bytes live in the `RecordStore` this never sees.
    pub(crate) fn content_hash(&self) -> ContentHash {
        let mut h = Hasher::new();
        mem::discriminant(self).hash(&mut h);
        match self {
            // All three shapes share this record's discriminant, so `QuadShape`'s goes
            // in first to keep a rectangle, shadow and triangle apart.
            Self::Quad(shape) => {
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
            Self::Polyline {
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
            Self::Text {
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
                self.hash_layout_inputs(&mut h);
            }
            Self::Mesh {
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
            Self::Image {
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
            Self::Icon {
                local_rect,
                handle,
                fit,
                tint,
                desaturate,
            } => {
                hash_optional_rect(*local_rect, &mut h);
                tint.hash(&mut h);
                h.write_u32(handle.icon.set.bits());
                h.write_u16(handle.icon.icon.index());
                h.write_u8((*fit as u8) | (u8::from(*desaturate) << 2));
            }
            // Geometry + style are hashed inline (every input lives on the record).
            // `bbox` is excluded. The ramp goes in by content hash so equal geometry
            // with different ramps differs. `CurveBasis`'s discriminant goes in first
            // to keep a cubic and an arc apart; the shared stroke fields are hashed
            // after the split.
            Self::Curve {
                cap,
                basis,
                stroke,
                bbox: _,
                ramp,
            } => {
                mem::discriminant(basis).hash(&mut h);
                match basis {
                    CurveBasis::Cubic(CubicBezier { p0, p1, p2, p3 }) => {
                        for point in [p0, p1, p2, p3] {
                            point.hash_visual(&mut h);
                        }
                    }
                    CurveBasis::Arc(CircularArc {
                        center,
                        radius,
                        start_angle,
                        sweep,
                    }) => {
                        center.hash_visual(&mut h);
                        radius.hash_visual(&mut h);
                        start_angle.hash_visual(&mut h);
                        sweep.hash_visual(&mut h);
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

    /// Feeds what layout reads from this record into `h`, answering whether there was anything. Only text is read by layout (text, face, wrap, alignment); colour and origin are paint, so a recoloured label still hits the measure cache.
    pub(crate) fn hash_layout_inputs(&self, h: &mut impl hash::Hasher) -> bool {
        let Self::Text {
            text,
            font,
            wrap,
            align,
            local_origin: _,
            color: _,
        } = self
        else {
            return false;
        };
        text.hash(h);
        font.size.hash_visual(h);
        font.line_height.hash_visual(h);
        let face = (u64::from(font.family.raw()) << 40)
            | (u64::from(font.weight.get()) << 24)
            | ((font.slant as u64) << 16)
            | (u64::from(align.raw()) << 8)
            | (*wrap as u64);
        h.write_u64(face);
        true
    }
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

/// **The backstop behind the NaN gate**: `Shapes::add` screens the authored shape and debug-asserts this on the resulting record.
///
/// An assertion, not a check: by here a gradient sits behind a `GradientId` and a triangle's `radius` went through `radius.max(0.0)`, so a clean record proves nothing. Bulk inputs arrive as a `bbox` under the AABB NaN contract.
impl NanCheck for ShapeRecord {
    fn has_nan(&self) -> bool {
        match self {
            Self::Quad(shape) => shape.has_nan(),
            Self::Polyline { width, bbox, .. } => width.is_nan() || bbox.has_nan(),
            Self::Text {
                local_origin,
                color,
                font,
                ..
            } => local_origin.has_nan() || color.has_nan() || font.has_nan(),
            Self::Mesh {
                local_rect,
                tint,
                bbox,
                ..
            } => local_rect.has_nan() || tint.has_nan() || bbox.has_nan(),
            Self::Image {
                local_rect,
                tint,
                fit,
                ..
            } => local_rect.has_nan() || tint.has_nan() || fit.has_nan(),
            Self::Icon {
                local_rect, tint, ..
            } => local_rect.has_nan() || tint.has_nan(),
            Self::Curve { stroke, bbox, .. } => stroke.has_nan() || bbox.has_nan(),
        }
    }
}

#[cfg(test)]
mod tests;
