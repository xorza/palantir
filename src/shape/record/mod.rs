//! The lowered form of one paint primitive, and the only shape vocabulary the encoder reads.

use crate::common::content_hash::ContentHash;
use crate::common::hash::Hasher;
use crate::common::span::Span;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::cubic_bezier::CubicBezier;
use crate::primitives::geometry::fit::Fit;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::identity::effect_id::EffectId;
use crate::primitives::identity::icon_handle::IconHandle;
use crate::primitives::layout::align::Align;
use crate::primitives::math::float_hash::FloatHash;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::image::{ImageDownsample, ImageFilter, ImageFit};
use crate::primitives::paint::line_style::{LineCap, LineJoin};
use crate::primitives::text::recorded_text::RecordedText;
use crate::shape::paint::arc_geometry::ArcGeometry;
use crate::shape::paint::bezier_geometry::BezierGeometry;
use crate::shape::paint::curve_ramp::CurveRamp;
use crate::shape::paint::image_source::ImageSource;
use crate::shape::paint::lowered_brush::{BrushHash, LoweredBrush};
use crate::shape::paint::lowered_material::LoweredMaterial;
use crate::shape::paint::lowered_stroke::LoweredStroke;
use crate::shape::paint::point_placement::PointPlacement;
use crate::shape::paint::quad_shape::QuadShape;
use crate::text::shaping_font::ShapingFont;
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

/// What a backdrop effect reads under it: what is painted there, captured, or the window's
/// ground while transparency is reduced, which takes no capture and makes no pane.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum EffectBackdrop {
    Capture(EffectPane),
    Ground,
}

/// What a backdrop effect captures under it, in logical px: the σ of its blur and how far past its
/// rect it reads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct EffectPane {
    pub(crate) sigma: f32,
    pub(crate) reach: f32,
}

impl EffectPane {
    /// Reads under its rect alone, sharp.
    pub(crate) const ZERO: Self = Self {
        sigma: 0.0,
        reach: 0.0,
    };
}

/// What a pane reads, as the cascade plans its capture: a box's material or a backdrop effect.
#[derive(Clone, Copy, Debug)]
pub(crate) enum PaneRead {
    Material(LoweredMaterial),
    Effect(EffectPane),
}

impl PaneRead {
    /// The blur's σ in logical px.
    pub(crate) fn sigma(self) -> f32 {
        match self {
            Self::Material(material) => material.sigma(),
            Self::Effect(pane) => pane.sigma,
        }
    }

    /// How far past the pane, in physical px under `scale` physical px per logical px, its
    /// composite reads: a lens's margin ([`LoweredMaterial::lens_margin`], `narrowest` its pane's
    /// narrowest side), or an effect's reach rounded out.
    #[expect(
        clippy::cast_sign_loss,
        reason = "a reach is a length, which `EffectPaint::reach` checks, and a scale is positive"
    )]
    pub(crate) fn margin(self, scale: f32, narrowest: Option<f32>) -> u32 {
        match self {
            Self::Material(material) => material.lens_margin(scale, narrowest),
            Self::Effect(pane) => (pane.reach * scale).ceil() as u32,
        }
    }
}

/// `#[repr(u8)]` pins the discriminant to one byte (`hot_struct_sizes`). Reorder variants freely: hashes use `mem::discriminant` and are only compared within a process run.
#[repr(u8)]
#[derive(Clone, Debug)]
pub(crate) enum ShapeRecord {
    /// A box or a rounded triangle, per [`QuadShape`].
    Quad(QuadShape),
    /// Stroked polyline. `points`/`colors` index the `RecordStore`; `colors` has 1 entry for `Single`, `points.len()` for `PerPoint`, `points.len() - 1` for `PerSegment`. `bbox` is the points' AABB as authored, the extent `placement` fits into its rect where the polyline is drawn; damage and composition apply the stroke inflation.
    Polyline {
        width: f32,
        color_mode: ColorMode,
        cap: LineCap,
        join: LineJoin,
        points: Span,
        colors: Span,
        bbox: Rect,
        placement: PointPlacement,
        content_hash: ContentHash,
    },
    /// Shaped text run, *authoring inputs only*: measured size and the shaped-buffer key are layout outputs in `Layout.text_shapes`. `wrap` selects shape-once (`Single`) or reshape under a narrower committed width (`Wrap`); `align` positions the glyph bbox (`Auto`/`Stretch` collapse to top-left).
    ///
    /// `None` paints into the owner's rect deflated by `padding`. `Some(origin)` paints at `owner.min + origin` with the shaped measurement as the bbox, so a widget can apply offsets that depend on shaped-buffer state.
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
    /// User-supplied colored triangle mesh. Spans index the `RecordStore`'s `meshes` pool; `content_hash` covers vertex+index bytes.
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
    /// A baked SVG icon, rasterized at the exact physical size into the icon atlas. The [`IconHandle`] carries the viewBox, so `fit` needs no registry lookup. `tint` multiplies a tintable icon whole and a color icon's alpha only, see [`IconPaint`](crate::widget::IconPaint).
    Icon {
        local_rect: Option<Rect>,
        handle: IconHandle,
        fit: Fit,
        tint: RgbaF16,
        /// Draws a color icon as its own luminance, see [`IconPaint::desaturate`](crate::widget::IconPaint::desaturate).
        desaturate: bool,
    },
    /// An app-written effect over a rounded rect: `local_rect = None` paints the owner's rect,
    /// `Some(r)` paints `r` owner-relative, which may exceed the owner. `params` indexes the
    /// `RecordStore`'s parameter words; `content_hash` covers the effect, those words, its images
    /// and its pane.
    Effect {
        /// What a backdrop effect reads under it; `None` for a color effect.
        backdrop: Option<EffectBackdrop>,
        local_rect: Option<Rect>,
        corners: Corners,
        effect: EffectId,
        params: Span,
        /// Into `RecordStore::effect_images`.
        images: Span,
        content_hash: ContentHash,
    },
    /// Native GPU stroke of a cubic Bézier (quadratics promote to cubic, lines degenerate; see
    /// `CurveShape`'s lowering), fit into its owner where it is drawn
    /// ([`BezierGeometry::place`]). No joins.
    Curve {
        /// End-cap style; `Round`/`Square` extend the strip by `width/2` past each endpoint.
        cap: LineCap,
        geometry: BezierGeometry,
        stroke: LoweredStroke,
        /// The ramp the stroke color multiplies, sampled along `t`, p0 to p3.
        ramp: CurveRamp,
    },
    /// Native GPU stroke of an [`ArcGeometry`], whose circle resolves against the owner where it
    /// is drawn ([`ArcGeometry::place`]).
    Arc {
        /// End-cap style, as a curve's.
        cap: LineCap,
        geometry: ArcGeometry,
        stroke: LoweredStroke,
        /// The ramp the stroke color multiplies, sampled along the sweep.
        ramp: CurveRamp,
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
    /// What the record reads as a pane over the pixels before it: a rect's material or a backdrop
    /// effect's, each where it captures; `None` for every other record.
    pub(crate) const fn pane(&self) -> Option<PaneRead> {
        match self {
            Self::Quad(QuadShape::Box {
                material: Some(material),
                ..
            }) if material.captures() => Some(PaneRead::Material(*material)),
            Self::Effect {
                backdrop: Some(EffectBackdrop::Capture(pane)),
                ..
            } => Some(PaneRead::Effect(*pane)),
            _ => None,
        }
    }

    /// Whether the record is a pane over the pixels before it.
    pub(crate) const fn is_pane(&self) -> bool {
        self.pane().is_some()
    }

    /// The record's canonical content hash, filling the parallel `Shapes::hashes` arena at `Shapes::add`; `Tree::compute_rollups` and damage diff read it, and production code never rehashes a record.
    ///
    /// The schedule is `discriminant → per-variant fields` at every nesting level (`mem::discriminant`, so it cannot drift from the enum). Nothing is persisted, so variants can be added or reordered freely. `Polyline` and `Mesh` hand over a `content_hash` their lowering computed, as their payload bytes live in the `RecordStore` this never sees.
    pub(crate) fn content_hash(&self) -> ContentHash {
        let mut h = Hasher::new();
        mem::discriminant(self).hash(&mut h);
        match self {
            // Both shapes share this record's discriminant, so `QuadShape`'s goes in first
            // to keep a box and a triangle apart.
            Self::Quad(shape) => {
                mem::discriminant(shape).hash(&mut h);
                match shape {
                    QuadShape::Box {
                        geometry,
                        fill,
                        border,
                        shadow,
                        material,
                    } => {
                        geometry.hash_into(&mut h);
                        hash_brush(*fill, &mut h);
                        border.hash_into(&mut h);
                        shadow.hash(&mut h);
                        material.hash(&mut h);
                    }
                    QuadShape::Triangle {
                        geometry,
                        fill,
                        border,
                    } => {
                        geometry.hash_into(&mut h);
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
                placement,
                width: _,
                color_mode: _,
                cap: _,
                join: _,
                points: _,
                colors: _,
                bbox: _,
            } => {
                content_hash.hash(&mut h);
                placement.hash_into(&mut h);
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
                    // see; a new paint takes a new target `id`, which it does.
                    ImageSource::GpuView { id, epoch } => {
                        h.write_u64(id.0);
                        h.write_u64(*epoch);
                    }
                }
                // The fit (including `Tile`'s UV transform, which changes every
                // pan/zoom placement), both sampling filters and the minification tap mode:
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
                h.write_u16(handle.icon.icon.idx());
                h.write_u8((*fit as u8) | (u8::from(*desaturate) << 2));
            }
            // `content_hash` folds the effect and its parameter words, which live in the
            // `RecordStore`; the span is frame-local.
            Self::Effect {
                local_rect,
                corners,
                content_hash,
                backdrop: _,
                effect: _,
                params: _,
                images: _,
            } => {
                hash_optional_rect(*local_rect, &mut h);
                corners.hash(&mut h);
                content_hash.hash(&mut h);
            }
            // Geometry + style are hashed inline (every input lives on the record).
            // `bbox` is excluded. The ramp goes in by content hash so equal geometry
            // with different ramps differs.
            Self::Curve {
                cap,
                geometry:
                    BezierGeometry {
                        bezier: CubicBezier { p0, p1, p2, p3 },
                        placement,
                    },
                stroke,
                ramp,
            } => {
                for point in [p0, p1, p2, p3] {
                    point.hash_visual(&mut h);
                }
                placement.hash_into(&mut h);
                hash_stroke(*stroke, *cap, *ramp, &mut h);
            }
            Self::Arc {
                cap,
                geometry:
                    ArcGeometry {
                        local_rect,
                        start_angle,
                        sweep,
                    },
                stroke,
                ramp,
            } => {
                hash_optional_rect(*local_rect, &mut h);
                start_angle.hash_visual(&mut h);
                sweep.hash_visual(&mut h);
                hash_stroke(*stroke, *cap, *ramp, &mut h);
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

fn hash_optional_rect(rect: Option<Rect>, h: &mut Hasher) {
    match rect {
        None => h.write_u8(0),
        Some(rect) => {
            h.write_u8(1);
            rect.hash_visual(h);
        }
    }
}

/// Fold a native stroke's paint into the shape hash: its width and color, its cap, and its ramp
/// by content hash.
fn hash_stroke(stroke: LoweredStroke, cap: LineCap, ramp: CurveRamp, h: &mut Hasher) {
    stroke.hash_into(h);
    match ramp {
        CurveRamp::None => h.write_u8(cap as u8),
        CurveRamp::Interned { id: _, hash } => {
            h.write_u8(cap as u8 | 0x80);
            hash.hash(h);
        }
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

/// **The backstop behind the shape setters' checks**: `Shapes::add` debug-asserts this on each lowered record.
///
/// An assertion, not a check: by here a gradient sits behind a `GradientId` and a triangle's `radius` went through `radius.max(0.0)`, so a clean record proves nothing. Bulk inputs arrive as a `bbox` under the AABB NaN contract.
impl NanCheck for ShapeRecord {
    fn has_nan(&self) -> bool {
        match self {
            Self::Quad(shape) => shape.has_nan(),
            Self::Polyline {
                width,
                bbox,
                placement,
                ..
            } => width.is_nan() || bbox.has_nan() || placement.has_nan(),
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
            Self::Image {
                local_rect,
                tint,
                fit,
                ..
            } => local_rect.has_nan() || tint.has_nan() || fit.has_nan(),
            Self::Icon {
                local_rect, tint, ..
            } => local_rect.has_nan() || tint.has_nan(),
            Self::Curve {
                geometry: BezierGeometry { bezier, placement },
                stroke,
                ..
            } => {
                let CubicBezier { p0, p1, p2, p3 } = bezier;
                [p0, p1, p2, p3].iter().any(|p| p.is_nan())
                    || placement.has_nan()
                    || stroke.has_nan()
            }
            Self::Arc {
                geometry:
                    ArcGeometry {
                        local_rect,
                        start_angle,
                        sweep,
                    },
                stroke,
                ..
            } => local_rect.has_nan() || start_angle.is_nan() || sweep.is_nan() || stroke.has_nan(),
            Self::Effect { local_rect, .. } => local_rect.has_nan(),
        }
    }
}

#[cfg(test)]
mod tests;
