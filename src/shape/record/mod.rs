//! The lowered form of one paint primitive, and the only shape vocabulary the encoder reads.

use crate::common::content_hash::ContentHash;
use crate::common::hash::Hasher;
use crate::common::span::Span;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::layout::align::Align;
use crate::primitives::math::float_hash::FloatHash;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::image::ImageFit;
use crate::primitives::text::recorded_text::RecordedText;
use crate::shape::geometry::closed_geometry::ClosedGeometry;
use crate::shape::geometry::point_placement::PointPlacement;
use crate::shape::geometry::stroke_geometry::StrokeGeometry;
use crate::shape::lower::LoweredPaint;
use crate::shape::paint::capture_params::CaptureParams;
use crate::shape::paint::closed_paint::ClosedPaint;
use crate::shape::paint::image_source::ImageSource;
use crate::shape::paint::lowered_brush::{BrushHash, LoweredBrush};
use crate::shape::paint::lowered_stroke::LoweredStroke;
use crate::text::shaping_font::ShapingFont;
use crate::text::wrap::TextWrap;
use glam::Vec2;
use std::hash;
use std::hash::{Hash, Hasher as _};
use std::mem;

/// One draw as the frame recorded it: a geometry and the paint its family draws, geometry outside,
/// as a payload and a draw row hold them after it, and as Skia Graphite records a draw as its
/// geometry and its `PaintParams`
/// ([`Renderer.h`](https://skia.googlesource.com/skia/+/main/src/gpu/graphite/Renderer.h)). The
/// record keeps the authored pair, and the encoder picks the renderer family from it: an icon of
/// a rect lowers to the glyph atlas, a box's shadow to the blurred box. Refused: the paint owning
/// its shape, as Impeller's `Contents::GetGeometry` does, which an image and an effect payload did;
/// a record per renderer family, which puts the renderer's split in the authoring model.
///
/// `#[repr(u8)]` pins the discriminant to one byte (`hot_struct_sizes`). Reorder variants freely: hashes use `mem::discriminant` and are only compared within a process run.
#[repr(u8)]
#[derive(Clone, Debug)]
pub(crate) enum ShapeRecord {
    /// A closed shape, a box or a rounded triangle, and the paint that covers it.
    Closed {
        geometry: ClosedGeometry,
        paint: ClosedPaint,
    },
    /// A stroke along a path, a cubic, an arc or a polyline, all drawn on the one curve pipeline;
    /// damage and composition apply the stroke's reach. A cubic and an arc place against their
    /// owner where they are drawn, and a polyline fits its points there.
    Stroke {
        geometry: StrokeGeometry,
        stroke: LoweredStroke,
    },
    /// Shaped text run, *authoring inputs only*: measured size and the shaped-buffer key are layout
    /// outputs in `Layout.text_shapes`. `wrap` selects shape-once or reshape under a narrower
    /// committed width; `align` positions the glyph bbox (`Auto`/`Stretch` collapse to top-left).
    ///
    /// `local_origin` `None` paints into the owner's rect deflated by `padding`; `Some(origin)`
    /// paints at `owner.min + origin` with the shaped measurement as the bbox, so a widget can
    /// apply offsets that depend on shaped-buffer state.
    Text {
        local_origin: Option<Vec2>,
        /// Lowered text storage and its content hash; arena-backed input is normalized into the active record store first, so span and hash can't come from different passes.
        text: RecordedText,
        color: RgbaF16,
        /// The face and metrics to shape in, as [`TextShape`](crate::widget::TextShape) authors them with the family keyed by the store's shaper; [`TextShapeKey`](crate::text::key::TextShapeKey) is minted from it.
        ///
        /// `line_height` is a resolved px leading fed to `Metrics::new` (authoring sets `size * line_height_factor`); carrying px keeps widget conventions out of the shaper and gives different leading distinct buffers.
        font: ShapingFont,
        wrap: TextWrap,
        align: Align,
    },
    /// User-supplied colored triangle mesh. Spans index the `RecordStore`'s `mesh_vertices` and
    /// `mesh_idxs`; `content_hash` covers vertex+index bytes.
    Mesh {
        placement: PointPlacement,
        tint: RgbaF16,
        vertices: Span,
        idxs: Span,
        /// AABB of the vertex positions as authored, the extent `placement` fits; frozen from
        /// `Mesh::bbox()` so the encoder doesn't re-scan.
        bbox: Rect,
        content_hash: ContentHash,
    },
}

/// Where a [`ShapeRecord::Text`] puts its run in the owner: the record's `local_origin` and `align`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TextAnchor {
    pub(crate) local_origin: Option<Vec2>,
    pub(crate) align: Align,
}

impl TextAnchor {
    /// Tight owner-local paint bbox of a run that measured `measured` (`PlaneLayout::text_shapes`), in an owner of `owner_size` padded by `padding`; the encoder uses the same [`Align::place_in`] formula in screen space, so damage and draw rects can't drift.
    ///
    /// **Damage inflation lives in cascade**, not here: the ladder-snap overshoot is `measured × STEP/2` in absolute screen pixels, so it applies after the cascade lifts the rect to screen; locally it would scale by `cascade_scale`.
    ///
    /// - `local_origin: Some(origin)`: rect is `origin + measured`.
    /// - `local_origin: None`: placed by [`Align::place_in`] in the owner's padded inner rect.
    pub(crate) fn bbox_local(self, owner_size: Size, padding: Spacing, measured: Size) -> Rect {
        if let Some(origin) = self.local_origin {
            Rect {
                min: origin,
                size: measured,
            }
        } else {
            let owner_local = Rect {
                min: Vec2::ZERO,
                size: owner_size,
            };
            self.align
                .place_in(owner_local.deflated_by(padding), measured)
        }
    }
}

impl ShapeRecord {
    /// What the record reads as a backdrop over the pixels before it: a box's or a triangle's
    /// material or a backdrop effect's, each where it captures; `None` for every other record.
    pub(crate) const fn backdrop_capture(&self) -> Option<CaptureParams> {
        match self {
            Self::Closed { paint, .. } => paint.backdrop_capture(),
            _ => None,
        }
    }

    /// Whether the record reads a backdrop over the pixels before it.
    pub(crate) const fn reads_backdrop(&self) -> bool {
        self.backdrop_capture().is_some()
    }

    /// The record's canonical content hash, filling the parallel `Shapes::hashes` arena at `Shapes::push`; `Tree::compute_rollups` and damage diff read it, and production code never rehashes a record.
    ///
    /// The schedule is `discriminant → per-variant fields` at every nesting level. A polyline's
    /// geometry and `Mesh` hand over a `content_hash` their lowering computed, as their payload
    /// bytes live in the `RecordStore` this never sees.
    pub(crate) fn content_hash(&self) -> ContentHash {
        let mut h = Hasher::new();
        mem::discriminant(self).hash(&mut h);
        match self {
            Self::Closed { geometry, paint } => {
                geometry.hash_into(&mut h);
                hash_closed_paint(paint, &mut h);
            }
            Self::Stroke { geometry, stroke } => {
                geometry.hash_into(&mut h);
                stroke.hash_into(&mut h);
            }
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
                local_origin.hash_visual(&mut h);
                color.hash(&mut h);
                self.hash_layout_inputs(&mut h);
            }
            Self::Mesh {
                placement,
                tint,
                content_hash,
                vertices: _,
                idxs: _,
                bbox: _,
            } => {
                placement.hash_into(&mut h);
                tint.hash(&mut h);
                content_hash.hash(&mut h);
            }
        }
        ContentHash::finish(&h)
    }

    /// Feeds what layout reads from this record into `h`, answering whether there was anything. Only text is read by layout (text, face, wrap, alignment); color and origin are paint, so a recolored label still hits the measure cache.
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
        font.font_size.hash_visual(h);
        font.line_height.hash_visual(h);
        let face = (u64::from(font.family.get()) << 40)
            | (u64::from(font.weight.get()) << 24)
            | ((font.slant as u64) << 16)
            | (u64::from(align.raw()) << 8)
            | (*wrap as u64);
        h.write_u64(face);
        true
    }
}

/// Fold a closed shape's paint into the shape hash, its discriminant first, as the variants share
/// none of their fields.
fn hash_closed_paint(paint: &ClosedPaint, h: &mut Hasher) {
    mem::discriminant(paint).hash(h);
    match paint {
        ClosedPaint::Fill(LoweredPaint {
            fill,
            border,
            shadow,
            material,
        }) => {
            hash_brush(*fill, h);
            border.hash_into(h);
            shadow.hash(h);
            material.hash(h);
        }
        // `ImageSource`'s discriminant goes in first to keep a texture draw and a view composite
        // apart; the shared placement fields are hashed once, around the split.
        ClosedPaint::Image {
            tint,
            source,
            fit,
            min_filter,
            mag_filter,
            downsample,
        } => {
            tint.hash(h);
            mem::discriminant(source).hash(h);
            match source {
                // The registration `id`, the intrinsic `size`, and the write count that moves the
                // hash when texels change under an unchanged id.
                ImageSource::Texture {
                    id,
                    size,
                    generation,
                } => {
                    h.write_u64(id.0);
                    h.write_u64(u64::from(size.x) | (u64::from(size.y) << 32));
                    h.write_u32(*generation);
                }
                // `epoch` is the view's damage version: `Ui::gpu_view` bumps it to the frame id on
                // `repaint(true)` (hash changes, the rect repaints) and holds it on
                // `repaint(false)` (hash matches, the view culls). The view's paint lives in
                // `Ui::gpu_views`, which the hash cannot see; a new paint takes a new target `id`,
                // which it does.
                ImageSource::GpuView { id, epoch } => {
                    h.write_u64(id.0);
                    h.write_u64(*epoch);
                }
            }
            // The fit (including `Tile`'s UV transform, which changes every pan/zoom placement),
            // both sampling filters and the minification tap mode: two 1-bit filters in the low
            // bits, the 3-variant `downsample` above.
            hash_fit(fit, h);
            h.write_u8(
                (*min_filter as u8) | ((*mag_filter as u8) << 1) | ((*downsample as u8) << 2),
            );
        }
        // The handle's `view_box` is baked and constant per `(set, icon)`, so identity plus rect,
        // fit and tint is all that can change; `IconSetId` carries a generation, so a reused slot
        // never names two artworks. The raster size is *not* hashed: it follows from the resolved
        // screen rect and needs a display scale the record lacks.
        ClosedPaint::Icon {
            handle,
            fit,
            tint,
            desaturate,
        } => {
            tint.hash(h);
            h.write_u32(handle.icon.set.bits());
            h.write_u16(handle.icon.icon.idx());
            h.write_u8((*fit as u8) | (u8::from(*desaturate) << 2));
        }
        // `content_hash` folds the effect and its parameter words, which live in the
        // `RecordStore`; the span is frame-local.
        ClosedPaint::Effect {
            content_hash,
            backdrop: _,
            outside: _,
            effect: _,
            params: _,
            images: _,
        } => content_hash.hash(h),
    }
}

/// Fold a lowered fill into the shape hash via [`LoweredBrush::hash_parts`], which
/// the chrome hash reads too.
fn hash_brush(fill: LoweredBrush, h: &mut Hasher) {
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

/// **The backstop behind the shape setters' checks**: `Shapes::push` debug-asserts this on each lowered record.
///
/// An assertion, not a check: by here a gradient sits behind a `GradientId` and a triangle's `radius` went through `radius.max(0.0)`, so a clean record proves nothing. Bulk inputs arrive as a `bbox` under the AABB NaN contract.
impl NanCheck for ShapeRecord {
    fn has_nan(&self) -> bool {
        match self {
            Self::Closed { geometry, paint } => geometry.has_nan() || paint.has_nan(),
            Self::Stroke { geometry, stroke } => geometry.has_nan() || stroke.has_nan(),
            Self::Text {
                local_origin,
                color,
                font,
                ..
            } => local_origin.has_nan() || color.has_nan() || font.has_nan(),
            Self::Mesh {
                placement,
                tint,
                bbox,
                ..
            } => placement.has_nan() || tint.has_nan() || bbox.has_nan(),
        }
    }
}

#[cfg(test)]
mod tests;
