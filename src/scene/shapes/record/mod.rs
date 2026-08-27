//! The lowered form of one paint primitive — what a shape becomes once
//! authoring is done with it, and the only shape vocabulary the encoder
//! reads.

use crate::icons::icon_set::IconHandle;
use crate::layout::types::align::Align;
use crate::primitives::color::ColorF16;
use crate::primitives::image::{ImageDownsample, ImageFilter, ImageFit};
use crate::primitives::nan::NanCheck;
use crate::primitives::recorded_text::RecordedText;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::spacing::Spacing;
use crate::primitives::span::Span;
use crate::scene::shapes::paint::{CurveBasis, ImageSource, QuadShape, ShapeBrush};
use crate::shape::icon::IconFit;
use crate::shape::style::{LineCap, LineJoin};
use crate::text::glyph_font::GlyphFont;
use crate::text::wrap::TextWrap;
use glam::Vec2;

#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum ColorMode {
    /// One colour for the whole polyline — the default and the only
    /// mode whose colour span is a single entry.
    #[default]
    Single = 0,
    PerPoint = 1,
    PerSegment = 2,
}

/// `#[repr(u8)]` is for **layout**: it holds the discriminant to one
/// byte, which the `ShapeRecord` entry in `hot_struct_sizes` pins.
///
/// Reorder variants freely.
/// [`compute_record_hash`](crate::scene::shapes::hash::compute_record_hash)
/// hashes the discriminant via `mem::discriminant`, and the hashes it
/// produces are only ever compared against others from the same process
/// run — nothing persists them, so nothing depends on their values.
#[repr(u8)]
#[derive(Clone, Debug)]
pub(crate) enum ShapeRecord {
    /// Rounded rectangle, box-shadow, or rounded triangle, per
    /// [`QuadShape`]. One record kind, because everything outside the
    /// shape is shared: the same quad pipeline, the same `Quad`
    /// instance, and the same cull / group-flush / occlusion handling
    /// from the payload down.
    Quad(QuadShape),
    /// Stroked polyline. `points`/`colors` index into the
    /// `RecordPayloads`' `polyline_points` / `polyline_colors`. `colors`
    /// length depends on `color_mode`: 1 for `Single`,
    /// `points.len()` for `PerPoint`, `points.len() - 1` for
    /// `PerSegment`. `content_hash` summarizes points+colors+mode
    /// +cap+join bytes for cache identity. `bbox` is the centerline
    /// AABB of `points` in owner-relative coords; damage and composition
    /// apply the shared raster-aware stroke inflation. `cap` and `join`
    /// are user-picked stroke-style enums consumed by the composer and
    /// stroke shader.
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
    /// Shaped text run — *authoring inputs only*. Measured size and
    /// shaped-buffer key are layout outputs and live on
    /// `Layout.text_shapes`, not here. `wrap` selects between "shape
    /// once and freeze" (`Single`) and "reshape if the parent commits a
    /// narrower width than the natural unbroken line" (`Wrap`). `align`
    /// positions the glyph bbox inside the owner node's arranged rect (or
    /// `local_rect` if set) — the encoder reads it together with the
    /// shaped run's `measured` to shift the emitted `DrawText` rect.
    /// `HAlign::Auto`/`Stretch` and `VAlign::Auto`/`Stretch` collapse to
    /// top-left for text (glyphs don't stretch).
    ///
    /// `None` paints into the owner's arranged rect (deflated by the
    /// node's `padding`) and `align` positions the glyph bbox inside
    /// it. `Some(origin)` paints at `owner.min + origin` with the
    /// shaped measurement as the bbox — the encoder is a passthrough
    /// for positioning. Lets a widget shift the run by
    /// scroll/alignment offsets that depend on shaped-buffer state.
    Text {
        local_origin: Option<Vec2>,
        /// Lowered text storage and its pre-computed content hash. Arena-backed
        /// input is normalized into the active record store before this value
        /// is built, so its span and hash cannot belong to different passes.
        text: RecordedText,
        color: ColorF16,
        /// The face and metrics to shape in, the same named type
        /// [`TextShape`](crate::TextShape) authors and
        /// [`TextShapeKey`](crate::text::key::TextShapeKey) is minted from
        /// — so the mirror between the three is one field, not four kept
        /// in step by eye.
        ///
        /// `line_height_px` is a resolved logical-px leading, fed straight
        /// to the shaper's `Metrics::new`. Authoring-side widgets set it to
        /// `size_px * line_height_mult` where the multiplier defaults to
        /// [`LINE_HEIGHT_MULT`](crate::widgets::theme::text_style::LINE_HEIGHT_MULT)
        /// (1.2). Carrying the resolved px — instead of a multiplier the
        /// shaper would re-resolve — keeps widget conventions out of the
        /// shaper, and makes two runs at one font-size but different
        /// leading produce distinct cached buffers (via
        /// [`TextShapeKey::lh_q`](crate::text::key::TextShapeKey::lh_q)).
        font: GlyphFont,
        wrap: TextWrap,
        align: Align,
    },
    /// User-supplied colored triangle mesh. Vertex/index data lives on
    /// the `RecordPayloads`' `meshes` pool; these spans index into its
    /// vertex/index vecs. `content_hash` summarizes
    /// vertex+index bytes for cache identity — two frames with
    /// identical mesh content share a hash even though their span
    /// offsets differ.
    Mesh {
        local_rect: Option<Rect>,
        tint: ColorF16,
        vertices: Span,
        indices: Span,
        /// Owner-local AABB of the mesh's vertex positions. Snapshot of
        /// `Mesh::bbox()` at lowering — the user-side `Mesh` does the
        /// lazy compute (and caches it across frames for retained meshes),
        /// the record just freezes the value so encoder/composer don't
        /// re-scan.
        bbox: Rect,
        content_hash: u64,
    },
    /// Textured rectangle — a registered image or an app-rendered
    /// `GpuView`'s off-screen target, per [`ImageSource`]. One record
    /// kind, because everything outside `source` is shared: the same
    /// paint-rect resolution, fit, sampling, tint, and image pipeline.
    /// `local_rect = None` paints into the owner's full arranged rect;
    /// `Some(r)` paints `r` at owner-relative coords. `tint` multiplies
    /// sampled pixels in linear-RGB premultiplied space.
    Image {
        local_rect: Option<Rect>,
        tint: ColorF16,
        source: ImageSource,
        fit: ImageFit,
        min_filter: ImageFilter,
        mag_filter: ImageFilter,
        downsample: ImageDownsample,
    },
    /// A baked SVG icon, rasterized on demand at the exact physical pixel
    /// size it lands on and cached in the icon atlas. Carries the artwork's
    /// viewBox on the [`IconHandle`], so resolving `fit` needs no registry
    /// lookup on the encode path. `tint` multiplies a tintable icon whole and
    /// a colour icon's alpha only — see [`IconShape`](crate::IconShape).
    Icon {
        local_rect: Option<Rect>,
        handle: IconHandle,
        fit: IconFit,
        tint: ColorF16,
        /// Draw a colour icon as its own luminance — see
        /// [`IconShape::desaturate`](crate::IconShape::desaturate).
        desaturate: bool,
    },
    /// Native GPU stroke — a cubic Bézier or an exact circular arc, per
    /// [`CurveBasis`] (quadratics promote to cubic at lowering, lines
    /// degenerate to one — see `shapes::lower`). One record kind, because
    /// everything outside `basis` is shared: the same pipeline, cap model,
    /// gradient-along-`t` sampling, and deferred stroke bound. Stored
    /// owner-local; the composer adds the owner origin + active transform
    /// at compose time and uploads to a per-instance buffer. No joins
    /// (single-segment primitive); `fill` and `cap` are documented on
    /// their fields. `bbox` is the tight owner-local centerline AABB;
    /// damage and composition apply the shared raster-aware stroke
    /// inflation after transforms are known.
    Curve {
        basis: CurveBasis,
        width: f32,
        /// Lowered stroke fill. Solid colour stays inline; `Linear`
        /// gradient content rides as a `RecordedGradient` indexed by
        /// `ShapeBrush::Gradient` and resolves its atlas row on encode.
        /// The gradient is sampled in the shader along the curve
        /// parameter `t` — p0 → p3 for a cubic, a0 → a1 for an arc — so
        /// the `LinearGradient::angle` from authoring is intentionally
        /// ignored: the stroke carries its own 1-D parameter. The
        /// authoring type cannot contain radial or conic gradients.
        fill: ShapeBrush,
        /// Pre-computed content hash of `fill` when it's a gradient,
        /// `0` for solid — same context-free-hash trick as
        /// [`QuadShape::Rect`]'s own `fill_grad_hash`.
        fill_grad_hash: u64,
        /// End-cap style. Joins are absent (single-curve primitive,
        /// no interior). `Round`/`Square` extend the painted strip by
        /// `width/2` past each endpoint along the local tangent.
        cap: LineCap,
        bbox: Rect,
    },
}

impl ShapeRecord {
    /// Owner-local bbox used as the basis for cascade's screen-space paint
    /// bound. `Polyline` / `Curve` return their tight centerline
    /// bbox because stroke width and the physical-pixel AA fringe are applied
    /// after the bbox reaches screen space. `Quad` defers to
    /// [`QuadShape::bbox_local`], which is where the shadow halo is
    /// accounted for; the remaining shapes return their paint bbox
    /// directly. Does **not** handle `Text` — its bbox
    /// depends on the shaped extent from the layout pass and is computed by
    /// [`text_paint_bbox_local`], which cascade calls directly.
    #[inline]
    pub(crate) fn bbox_local(&self, owner_size: Size) -> Rect {
        match self {
            ShapeRecord::Quad(shape) => shape.bbox_local(owner_size),
            ShapeRecord::Polyline { bbox, .. } | ShapeRecord::Curve { bbox, .. } => *bbox,
            // A mesh's vertex hull can exceed the owner rect (rotated /
            // overflowing meshes), so it must report that hull — like
            // `Polyline` / `Curve` — or partial damage clips the overflow.
            // `local_rect` only *offsets* the mesh (its size is the vertex
            // hull, not `local_rect.size`), so translate the bbox by its min.
            ShapeRecord::Mesh {
                bbox, local_rect, ..
            } => {
                let origin = local_rect.map_or(Vec2::ZERO, |r| r.min);
                Rect {
                    min: bbox.min + origin,
                    size: bbox.size,
                }
            }
            ShapeRecord::Image { local_rect, .. } | ShapeRecord::Icon { local_rect, .. } => {
                local_rect.unwrap_or(Rect {
                    min: Vec2::ZERO,
                    size: owner_size,
                })
            }
            // Cascade dispatches Text to `text_paint_bbox_local` before
            // reaching this method — a direct call here would silently
            // lose the shaped extent. Unreachable by construction rather
            // than by convention: the cascade arm that calls this lists
            // its variants explicitly, so a new one is a compile error
            // there instead of a panic here.
            ShapeRecord::Text { .. } => {
                unreachable!("Text shapes resolve via text_paint_bbox_local in cascade")
            }
        }
    }
}

/// Tight owner-local paint bbox of a [`ShapeRecord::Text`], using the
/// shaped extent the measure pass already computed (lives in
/// `LayerLayout::text_shapes`). The encoder applies the same formula
/// in screen space — [`Align::place_in`] is the sole source so cascade
/// damage rects and encoder draw rects can't drift.
///
/// **Damage inflation lives in cascade** (`scene::cascade`), not here —
/// the ladder-snap overshoot is in absolute screen pixels
/// (`measured × STEP/2`) regardless of the ancestor scale, so it must
/// be applied to the screen rect *after* `lift_to_screen` rather than
/// to the local rect *before* it. Inflating in local coords would
/// produce a screen pad of `measured × STEP/2 × cascade_scale`, which
/// underflows at `cascade_scale < 1` (zoomed-out content) and lets
/// long lines bleed past the damage region.
///
/// - `local_origin: Some(origin)` ⇒ widget owns positioning; rect is
///   `origin + measured`.
/// - `local_origin: None` ⇒ encoder owns positioning via
///   [`Align::place_in`] against the owner's padded inner
///   rect.
pub(crate) fn text_paint_bbox_local(
    local_origin: Option<Vec2>,
    align: Align,
    padding: Spacing,
    owner_size: Size,
    measured: Size,
) -> Rect {
    match local_origin {
        Some(origin) => Rect {
            min: origin,
            size: measured,
        },
        None => {
            let owner_local = Rect {
                min: Vec2::ZERO,
                size: owner_size,
            };
            align.place_in(owner_local.deflated_by(padding), measured)
        }
    }
}

/// **The single NaN gate's predicate.** `Shapes::add` runs this on the
/// lowered record — one place, one `O(1)` test per shape, in release as
/// well as debug.
///
/// Lowered is the right tier for it. Every bulk input (polyline points,
/// mesh vertices, curve control points) has by then been folded into a
/// `bbox` under the AABB NaN contract, so a NaN in any of them shows up
/// as a NaN bbox — which means one `Rect` test replaces an `O(n)` scan
/// of the data that produced it, and the check is cheap enough to keep
/// in release rather than compiling out with the assert.
///
/// The one input that does *not* survive lowering is a gradient's
/// geometry, which is interned into the record store behind a
/// `GradientId`. `lower::brush` screens it there instead.
impl NanCheck for ShapeRecord {
    fn has_nan(&self) -> bool {
        match self {
            ShapeRecord::Quad(shape) => shape.has_nan(),
            // `bbox` carries the points; `content_hash` and the spans
            // are frame-local indices and can't be NaN.
            ShapeRecord::Polyline { width, bbox, .. } => width.is_nan() || bbox.has_nan(),
            ShapeRecord::Text {
                local_origin,
                color,
                font,
                ..
            } => local_origin.has_nan() || color.has_nan() || font.has_nan(),
            ShapeRecord::Mesh {
                local_rect,
                tint,
                bbox,
                ..
            } => local_rect.has_nan() || tint.has_nan() || bbox.has_nan(),
            ShapeRecord::Image {
                local_rect,
                tint,
                fit,
                ..
            } => local_rect.has_nan() || tint.has_nan() || fit.has_nan(),
            // `fit` is a bare tag and the handle's viewBox comes from baked
            // data, so the rect and the tint are the whole surface.
            ShapeRecord::Icon {
                local_rect, tint, ..
            } => local_rect.has_nan() || tint.has_nan(),
            // `bbox` is derived from `basis`, so it stands in for the
            // control points / centre / radius / angles.
            ShapeRecord::Curve {
                width, fill, bbox, ..
            } => width.is_nan() || fill.has_nan() || bbox.has_nan(),
        }
    }
}

#[cfg(test)]
mod tests;
