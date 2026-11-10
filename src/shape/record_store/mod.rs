//! Per-window store for retained record payloads, owned by [`Forest`]. Cleared
//! at record-pass start and retained across `PaintOnly` frames.
//!
//! One store rather than a copy chain: shape records, the encoder's paint
//! payloads and `MeshDraw` entries all carry spans into it directly.
//!
//! A shape's draw becomes its record here ([`RecordStore::record`]): each kind's `record_*`
//! method decides what to stage and stages it. A node's chrome lowers through
//! [`crate::shape::lower`], which also holds what both share.
//!
//! [`Forest`]: crate::scene::forest::Forest

pub(crate) mod recorded_gradient;
pub(crate) mod recorded_gradients;
pub(crate) mod text_store;

use crate::common::content_hash::ContentHash;
use crate::common::hash::Hasher;
use crate::common::span::Span;
use crate::effect::effect_registry::EffectRegistry;
use crate::image_registry::image_handle::ImageHandle;
use crate::primitives::effect::effect_kind::KindId;
use crate::primitives::geometry::mesh::{Mesh, MeshVertex};
use crate::primitives::geometry::rect::Rect;
use crate::primitives::identity::texture_id::TextureId;
use crate::primitives::math::domain::is_invisible;
use crate::primitives::math::float_hash::{self, FloatHash};
use crate::primitives::paint::color::RgbaF32;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::line_style::LineCap;
use crate::primitives::paint::material::reduced_backdrop::BackdropMode;
use crate::primitives::paint::paint::Paint;
use crate::primitives::paint::stroke::Stroke;
use crate::primitives::reduction::Reductions;
use crate::shape::curve::CurveShape;
use crate::shape::draw::Draw;
use crate::shape::effect_paint::ErasedEffectPaint;
use crate::shape::geometry::arc_geometry::ArcGeometry;
use crate::shape::geometry::bezier_geometry::BezierGeometry;
use crate::shape::geometry::box_geometry::BoxGeometry;
use crate::shape::geometry::closed_geometry::ClosedGeometry;
use crate::shape::geometry::triangle_geometry::{self, TriangleGeometry};
use crate::shape::icon_paint::IconPaint;
use crate::shape::image_paint::ImagePaint;
use crate::shape::lower;
use crate::shape::lower::LoweredPaint;
use crate::shape::mesh::MeshShape;
use crate::shape::paint::curve_ramp::CurveRamp;
use crate::shape::paint::image_source::ImageSource;
use crate::shape::paint::lowered_material::MaterialRead;
use crate::shape::paint::lowered_stroke::LoweredStroke;
use crate::shape::paint::quad_shape::QuadShape;
use crate::shape::polyline::{PolylineColors, PolylineShape};
use crate::shape::record::ShapeRecord;
use crate::shape::record::{ColorMode, EffectBackdrop};
use crate::shape::record_store::recorded_gradients::RecordedGradients;
use crate::shape::record_store::text_store::TextStore;
use crate::shape::rect::RectShape;
use crate::shape::text::TextShape;
use crate::shape::triangle::TriangleShape;
use crate::text::family_keys::FamilyKeys;
use crate::text::shaping_font::ShapingFont;
use glam::Vec2;
use std::hash::Hasher as _;
use std::slice;

/// The payload columns and the only API that appends to them. Every writer
/// holds `&mut Forest`, so the borrow checker excludes concurrent writes.
#[derive(Default, Debug)]
pub(crate) struct RecordStore {
    /// Vertex storage for `ShapeRecord::Mesh`, indexed by the record's `vertices`. Two columns,
    /// as the polyline payloads are kept, not an authoring `Mesh`, whose running hash and bounds
    /// the arena would leave stale.
    pub(crate) mesh_vertices: Vec<MeshVertex>,
    /// Index storage for `ShapeRecord::Mesh`, indexed by the record's `idxs`.
    pub(crate) mesh_idxs: Vec<u32>,
    /// Point storage for `ShapeRecord::Polyline`, indexed by the record's `points`.
    pub(crate) polyline_points: Vec<Vec2>,
    /// Color storage for `ShapeRecord::Polyline`: 1, `points.len()` or
    /// `points.len() - 1` per `ColorMode`, stored as `RgbaF16` (the `CurveInstance`
    /// lane form) so packing happens once at lowering.
    pub(crate) polyline_colors: Vec<RgbaF16>,
    /// Interned record-scoped gradient payloads, indexed by `LoweredBrush::Gradient`
    /// (set by `shape::lower::brush`). Cross-tree, so chrome and shape lowering
    /// share one pool and the encoder needs only this store.
    pub(crate) gradients: RecordedGradients,
    pub(crate) text: TextStore,
    /// The families of the shaper that shapes what this store records, keyed as a text shape
    /// lowers. `Default` gives a table of its own, for a store no shaper reads.
    families: FamilyKeys,
    /// Parameter words for `ShapeRecord::EffectPaint`, indexed by the record's `params`: each
    /// instance's `EffectParams` bytes, as the backend's storage buffer holds them.
    pub(crate) effect_params: Vec<u32>,
    /// Image storage for `ShapeRecord::EffectPaint`, indexed by the record's `images`: each
    /// instance's bound images, in slot order.
    pub(crate) effect_images: Vec<TextureId>,
}

/// Where [`RecordStore::stage_mesh`] put one mesh's geometry.
#[derive(Clone, Copy, Debug)]
pub(crate) struct MeshSpans {
    pub(crate) vertices: Span,
    pub(crate) idxs: Span,
}

/// Where [`RecordStore::stage_polyline`] put one polyline's geometry.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PolylineSpans {
    pub(crate) points: Span,
    pub(crate) colors: Span,
}

/// Where a draw's effect names resolve, and what is reduced, as [`RecordStore::record`] records
/// it: the two travel together down to each effect and material the draw names.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Resolve<'a> {
    pub(crate) effects: &'a EffectRegistry,
    pub(crate) reductions: Reductions,
}

impl RecordStore {
    /// A store that records text for the shaper keying families in `families`.
    pub(crate) fn new(families: FamilyKeys) -> Self {
        Self {
            families,
            ..Self::default()
        }
    }

    /// The record of `draw`, its names resolved in `resolve` and its variable-length payloads
    /// copied into this store; `None` where it paints nothing. The one place a draw becomes a
    /// record: the record is a fixed-size row of indexes into this store, so a draw cannot become
    /// one before its payloads land here.
    ///
    /// # Panics
    ///
    /// When the draw names an effect nothing loaded, or one of other types.
    #[track_caller]
    pub(crate) fn record(&mut self, draw: Draw<'_>, resolve: Resolve<'_>) -> Option<ShapeRecord> {
        match draw {
            Draw::Box { geometry, paint } => self.record_box(geometry, &paint, resolve),
            Draw::Triangle { shape, paint } => self.record_triangle(shape, &paint, resolve),
            Draw::Curve { shape, stroke } => self.record_curve(shape, stroke),
            Draw::Arc { geometry, stroke } => self.record_arc(geometry, stroke),
            Draw::Polyline { shape, stroke } => self.record_polyline(shape, stroke),
            Draw::Text { shape, color } => self.record_text(shape, color),
            Draw::Mesh { shape, tint } => self.record_mesh(shape, tint),
            Draw::ImagePaint { geometry, paint } => self.record_image(geometry, paint),
            Draw::IconPaint { rect, paint } => self.record_icon(rect, paint),
            Draw::EffectPaint { geometry, paint } => self.record_effect(geometry, paint, resolve),
        }
    }

    /// The record of a box at `geometry` in `paint`, its material resolved in `resolve`; `None`
    /// where it paints nothing.
    #[track_caller]
    fn record_box(
        &mut self,
        geometry: BoxGeometry,
        paint: &Paint,
        resolve: Resolve<'_>,
    ) -> Option<ShapeRecord> {
        let read = paint.material.as_ref().and_then(|material| {
            MaterialRead::resolve(material, resolve.effects, resolve.reductions)
        });
        if geometry.local_rect.is_some_and(Rect::is_paint_empty)
            || (paint.fill.is_noop()
                && paint.border.is_noop()
                && paint.shadow.is_noop()
                && read.is_none())
        {
            return None;
        }
        let LoweredPaint {
            fill,
            border,
            shadow,
            material,
        } = lower::lower_paint(self, paint, read);
        Some(ShapeRecord::Quad(QuadShape::Box {
            geometry,
            fill,
            border,
            shadow,
            material,
        }))
    }

    /// The record of `shape` in `paint`, its material resolved in `resolve`; `None` where it
    /// paints nothing.
    ///
    /// The corners keep their placement, which resolves where the triangle is drawn
    /// ([`TriangleGeometry::place`]); a triangle of no area drops here, as no fit gives it one. A
    /// rect of no size keeps it: `Natural` only moves the corners by the rect's corner, and a
    /// scaling fit collapses it where it is placed.
    fn record_triangle(
        &mut self,
        shape: &TriangleShape,
        paint: &Paint,
        resolve: Resolve<'_>,
    ) -> Option<ShapeRecord> {
        let TriangleShape {
            corners,
            radius,
            placement,
        } = *shape;
        let read = paint.material.as_ref().and_then(|material| {
            MaterialRead::resolve(material, resolve.effects, resolve.reductions)
        });
        if (paint.fill.is_noop() && paint.border.is_noop() && read.is_none())
            || triangle_geometry::area_is_paint_empty(corners)
        {
            return None;
        }
        // `check` held the shadow a no-op.
        let LoweredPaint {
            fill,
            border,
            shadow: _,
            material,
        } = lower::lower_paint(self, paint, read);
        Some(ShapeRecord::Quad(QuadShape::Triangle {
            geometry: TriangleGeometry {
                corners,
                radius,
                placement,
            },
            fill,
            border,
            material,
        }))
    }

    /// The record of `shape` drawn with `stroke`, its ramp interned here; `None` where it paints
    /// nothing.
    ///
    /// The points keep their placement, which resolves where the curve is drawn
    /// ([`BezierGeometry::place`]): a curve of no length paints its cap there, as a fit can move
    /// its point. A butt cap on no length has no extent, so it drops here.
    fn record_curve(&mut self, shape: &CurveShape, stroke: &Stroke) -> Option<ShapeRecord> {
        let CurveShape { path, placement } = *shape;
        // The width and color enter through `LoweredStroke::lower`.
        let Stroke {
            width: _,
            color: _,
            ramp,
            cap,
            join: _,
        } = *stroke;
        if stroke.is_noop() || (cap == LineCap::Butt && path.point().is_some()) {
            return None;
        }
        Some(ShapeRecord::Curve {
            cap,
            geometry: BezierGeometry {
                bezier: path.bezier(),
                placement,
            },
            stroke: LoweredStroke::lower(*stroke),
            ramp: ramp.map_or(CurveRamp::None, |ramp| lower::ramp(self, &ramp)),
        })
    }

    /// The record of `arc` drawn with `stroke`; `None` where it paints nothing: an invisible
    /// stroke, or no sweep under a butt cap, which has no extent. An arc in a rect of no size keeps
    /// its record, which paints its cap as a dot where a round or square cap gives it one
    /// ([`ArcGeometry::place`]).
    fn record_arc(&mut self, arc: ArcGeometry, stroke: &Stroke) -> Option<ShapeRecord> {
        if stroke.is_noop() || (stroke.cap == LineCap::Butt && is_invisible(arc.sweep.abs())) {
            return None;
        }
        Some(ShapeRecord::Arc {
            cap: stroke.cap,
            geometry: arc,
            stroke: LoweredStroke::lower(*stroke),
            ramp: stroke.ramp.map_or(CurveRamp::None, |stroke_ramp| {
                lower::ramp(self, &stroke_ramp)
            }),
        })
    }

    /// The record of `shape` drawn with `stroke`, its points and colors copied here; `None` where
    /// it paints nothing.
    fn record_polyline(
        &mut self,
        shape: &PolylineShape<'_>,
        stroke: &Stroke,
    ) -> Option<ShapeRecord> {
        let PolylineShape {
            points,
            colors,
            bbox,
            placement,
        } = *shape;
        let Stroke {
            width,
            color,
            ramp: _,
            cap,
            join,
        } = *stroke;
        let invisible = match colors {
            PolylineColors::Single => false,
            PolylineColors::PerPoint(colors) | PolylineColors::PerSegment(colors) => {
                colors.iter().all(|color| color.is_noop())
            }
        };
        if stroke.is_noop() || points.len() < 2 || invisible {
            return None;
        }
        let (mode, color_slice): (ColorMode, &[RgbaF32]) = match &colors {
            PolylineColors::Single => (ColorMode::Single, slice::from_ref(&RgbaF32::WHITE)),
            PolylineColors::PerPoint(cs) => (ColorMode::PerPoint, cs),
            PolylineColors::PerSegment(cs) => (ColorMode::PerSegment, cs),
        };

        // Dropped above under two points, and the color setters checked the count.
        debug_assert!(
            points.len() >= 2,
            "polyline with < 2 points reached lowering"
        );
        debug_assert!(colors.matches(points.len()));
        let staged = self.stage_polyline(points, color_slice, color);
        let lowered_colors = &self.polyline_colors[staged.colors.range()];

        let mut h = Hasher::new();
        for &point in points {
            point.hash_visual(&mut h);
        }
        h.pod_slice(lowered_colors);
        let style = (u64::from(float_hash::canon_bits(width)) << 24)
            | ((mode as u64) << 16)
            | ((cap as u64) << 8)
            | (join as u64);
        h.write_u64(style);
        let content_hash = ContentHash::finish(&h);

        Some(ShapeRecord::Polyline {
            width,
            color_mode: mode,
            cap,
            join,
            points: staged.points,
            colors: staged.colors,
            bbox,
            placement,
            content_hash,
        })
    }

    /// The record of `shape` inked in `color`, its text recorded here; `None` where it shapes
    /// nothing.
    ///
    /// Empty text or an unusable face shapes nothing, as the two public text queries answer too.
    /// A transparent color does not drop the run: text is the one shape layout reads, so the run
    /// still sizes its node, and the emit gate drops its ink.
    fn record_text(&self, shape: &TextShape, color: RgbaF32) -> Option<ShapeRecord> {
        let TextShape {
            local_origin,
            text,
            font,
            wrap,
            align,
        } = *shape;
        if text.is_empty() || !font.metrics_valid() {
            return None;
        }
        Some(ShapeRecord::Text {
            local_origin,
            text: self.text.record(text),
            color: color.to_f16(),
            font: ShapingFont::new(font, self.families()),
            wrap,
            align,
        })
    }

    /// The record of `shape` under `tint`, its vertices and indices copied here, its bbox and
    /// content hash frozen; `None` where it paints nothing.
    fn record_mesh(&mut self, shape: &MeshShape<'_>, tint: RgbaF32) -> Option<ShapeRecord> {
        let MeshShape { mesh, placement } = *shape;
        if tint.is_noop() || mesh.is_noop() {
            return None;
        }
        let staged = self.stage_mesh(mesh);
        Some(ShapeRecord::Mesh {
            placement,
            tint: tint.to_f16(),
            vertices: staged.vertices,
            idxs: staged.idxs,
            bbox: mesh.bbox(),
            content_hash: mesh.content_hash(),
        })
    }

    /// The record of `paint` over `geometry`; `None` where it paints nothing: a box of no size, a
    /// triangle of no area, which no fit gives one, or an invisible tint.
    fn record_image(&self, geometry: ClosedGeometry, paint: &ImagePaint) -> Option<ShapeRecord> {
        let ImagePaint {
            handle,
            fit,
            min_filter,
            mag_filter,
            downsample,
            tint,
        } = paint;
        let (fit, min_filter, mag_filter, downsample, tint) =
            (*fit, *min_filter, *mag_filter, *downsample, *tint);
        if geometry.is_paint_empty() || tint.is_noop() {
            return None;
        }
        Some(ShapeRecord::ImagePaint {
            geometry,
            tint: tint.to_f16(),
            source: ImageSource::Texture {
                id: handle.id(),
                size: handle.size(),
                generation: handle.generation(),
            },
            fit,
            min_filter,
            mag_filter,
            downsample,
        })
    }

    /// The record of `paint` over `rect`; `None` where it paints nothing.
    fn record_icon(&self, rect: &RectShape, paint: &IconPaint) -> Option<ShapeRecord> {
        let RectShape {
            local_rect,
            corners: _,
            coverage: _,
        } = *rect;
        let IconPaint {
            handle,
            fit,
            tint,
            desaturate,
        } = *paint;
        if local_rect.is_some_and(Rect::is_paint_empty) || tint.is_noop() {
            return None;
        }
        Some(ShapeRecord::IconPaint {
            local_rect,
            handle,
            fit,
            tint: tint.to_f16(),
            desaturate,
        })
    }

    /// The record of `paint` over `geometry`, its name resolved in `resolve`, its parameters and
    /// images copied here; `None` where it paints nothing.
    ///
    /// # Panics
    ///
    /// When no effect is loaded under the name, or one of other types, or the paint binds another
    /// count of images than the effect declares.
    #[track_caller]
    fn record_effect(
        &mut self,
        geometry: ClosedGeometry,
        paint: ErasedEffectPaint<'_>,
        resolve: Resolve<'_>,
    ) -> Option<ShapeRecord> {
        let ErasedEffectPaint {
            name,
            kind,
            params,
            capture,
            reduced_transparency,
            images,
        } = paint;
        // Resolved and checked in every mode and before the empty test, so a logic error panics
        // whatever is reduced and wherever it lies.
        let resolved = resolve.effects.resolve(name, resolve.reductions)?;
        let bound = images.iter().flatten().count();
        assert!(
            bound == usize::from(resolved.image_slots),
            "`{}` paints an effect that binds {} images, and the paint binds {bound}",
            name.name.as_str(),
            resolved.image_slots,
        );
        let backdrop = match kind {
            KindId::Color => None,
            KindId::Backdrop => Some(
                match reduced_transparency.mode(resolve.reductions.transparency)? {
                    BackdropMode::Capture => EffectBackdrop::Capture(capture),
                    BackdropMode::Ground => EffectBackdrop::Ground,
                },
            ),
        };
        if geometry.is_paint_empty() {
            return None;
        }
        let effect = resolved.id;
        let mut h = Hasher::new();
        h.write_u32(effect.0);
        h.pod_slice(params);
        // Each image by its id and generation, so an update repaints as an image's does.
        for image in images.iter().flatten() {
            h.write_u64(image.id().0);
            h.write_u32(image.generation());
        }
        match backdrop {
            Some(EffectBackdrop::Capture(capture)) => {
                h.write_u8(1);
                h.write_u32(capture.sigma.to_bits());
                h.write_u32(capture.reach.to_bits());
            }
            Some(EffectBackdrop::Ground) => h.write_u8(2),
            None => {}
        }
        Some(ShapeRecord::EffectPaint {
            backdrop,
            geometry,
            effect,
            params: self.stage_effect_params(params),
            images: self.stage_effect_images(images.iter().flatten()),
            content_hash: ContentHash::finish(&h),
        })
    }

    /// Stage one effect instance's parameter bytes as words. A loaded effect's parameters span a
    /// whole number of words, as its WGSL struct does.
    pub(crate) fn stage_effect_params(&mut self, bytes: &[u8]) -> Span {
        let (words, rest) = bytes.as_chunks::<4>();
        debug_assert!(rest.is_empty(), "effect parameters span whole words");
        let start = self.effect_params.len();
        self.effect_params
            .extend(words.iter().map(|word| u32::from_ne_bytes(*word)));
        Span::new(start as u32, (self.effect_params.len() - start) as u32)
    }

    /// Stage one effect instance's images, in slot order.
    pub(crate) fn stage_effect_images<'a>(
        &mut self,
        images: impl IntoIterator<Item = &'a ImageHandle>,
    ) -> Span {
        let start = self.effect_images.len();
        self.effect_images
            .extend(images.into_iter().map(ImageHandle::id));
        Span::new(start as u32, (self.effect_images.len() - start) as u32)
    }

    pub(crate) const fn families(&self) -> &FamilyKeys {
        &self.families
    }

    /// Drop every payload the last record pass appended, keeping capacity.
    /// PaintOnly skips this so the retained tree and storage stay valid together.
    pub(crate) fn clear(&mut self) {
        let Self {
            mesh_vertices,
            mesh_idxs,
            polyline_points,
            polyline_colors,
            gradients,
            text,
            effect_params,
            effect_images,
            families: _,
        } = self;
        mesh_vertices.clear();
        mesh_idxs.clear();
        polyline_points.clear();
        polyline_colors.clear();
        effect_params.clear();
        effect_images.clear();
        gradients.clear();
        text.clear();
    }

    /// Copy one mesh's vertices and indices in, returning the spans a
    /// `ShapeRecord::Mesh` carries.
    pub(crate) fn stage_mesh(&mut self, mesh: &Mesh) -> MeshSpans {
        let vertices = Span::new(
            self.mesh_vertices.len() as u32,
            mesh.vertices().len() as u32,
        );
        self.mesh_vertices.extend_from_slice(mesh.vertices());
        let idxs = Span::new(self.mesh_idxs.len() as u32, mesh.idxs().len() as u32);
        self.mesh_idxs.extend_from_slice(mesh.idxs());
        MeshSpans { vertices, idxs }
    }

    /// Copy one polyline's points and colors in, each color multiplied by `tint`
    /// and packed once here. Returns the spans a `ShapeRecord::Polyline` carries.
    pub(crate) fn stage_polyline(
        &mut self,
        points: &[Vec2],
        colors: &[RgbaF32],
        tint: RgbaF32,
    ) -> PolylineSpans {
        let staged_points = Span::new(self.polyline_points.len() as u32, points.len() as u32);
        self.polyline_points.extend_from_slice(points);
        let staged_colors = Span::new(self.polyline_colors.len() as u32, colors.len() as u32);
        self.polyline_colors
            .extend(colors.iter().map(|&c| c.tinted(tint).to_f16()));
        PolylineSpans {
            points: staged_points,
            colors: staged_colors,
        }
    }
}

#[cfg(test)]
mod tests;
