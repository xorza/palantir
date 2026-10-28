//! [`PlaneCtx`]: one plane's encode walk, built per tree by [`Encoder::encode`](crate::renderer::frontend::encoder::Encoder::encode).

use crate::cascade::cascade_input_hash::CascadeInputHash;
use crate::damage::region::DamageRegion;
use crate::layout::plane_layout::PlaneLayout;
use crate::layout::text::text_runs::TextRuns;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::spacing::Spacing;
use crate::primitives::layout::clip_mode::ClipMode;
use crate::primitives::math::domain::is_invisible;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::image::{FitRect, ImageDownsample, ImageFilter, ImageFit};
use crate::primitives::paint::lut_row::LutRow;
use crate::renderer::frontend::encoder::gradient_resolver::GradientPass;
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::brush_source::BrushSource;
use crate::renderer::frontend::payload::draw_curve_payload::DrawCurvePayload;
use crate::renderer::frontend::payload::draw_icon_payload::DrawIconPayload;
use crate::renderer::frontend::payload::draw_image_payload::{
    DrawImagePayload, ImageDraw, ViewPaint,
};
use crate::renderer::frontend::payload::draw_mesh_payload::DrawMeshPayload;
use crate::renderer::frontend::payload::draw_polyline_payload::DrawPolylinePayload;
use crate::renderer::frontend::payload::draw_quad_payload::{
    DrawQuadPayload, RectQuad, TriangleQuad,
};
use crate::renderer::frontend::payload::draw_text_payload::DrawTextPayload;
use crate::renderer::frontend::payload::gpu_fill::GpuFill;
use crate::renderer::frontend::payload::push_clip_payload::PushClipPayload;
use crate::renderer::frontend::payload::spin::Spin;
use crate::renderer::frontend::payload::stroke_bounds::StrokeBounds;
use crate::renderer::gpu_paint::gpu_views::GpuViews;
use crate::renderer::render_buffer::image_flags::ImageFlags;
use crate::scene::tree::Tree;
use crate::scene::tree::iter::TreeItem;
use crate::scene::tree::node_id::NodeId;
use crate::scene::tree::paint_animations::PaintAnimationCursor;
use crate::shape::paint::curve_ramp::CurveRamp;
use crate::shape::paint::image_source::ImageSource;
use crate::shape::paint::quad_shape::QuadShape;
use crate::shape::paint::shape_brush::ShapeBrush;
use crate::shape::record::{ShapeRecord, TextAnchor};
use crate::shape::record_store::recorded_gradients::GradientId;
use crate::shape::rect::RectKind;
use crate::text::shaped_ref::ShapedTextRef;
use glam::Vec2;
use std::time::Duration;

/// One shape of a node's paint stream: its owner, its index in the owner's
/// shapes, and the record.
#[derive(Clone, Copy, Debug)]
struct ShapeItem<'s> {
    owner: NodeId,
    idx: u32,
    record: &'s ShapeRecord,
}

/// The fixed inputs one plane's encode walk reads.
#[derive(Debug)]
pub(super) struct PlaneCtx<'a, 'g> {
    pub(super) tree: &'a Tree,
    pub(super) layout: &'a PlaneLayout,
    pub(super) cascade_inputs: &'a [CascadeInputHash],
    pub(super) subtree_paint_rects: &'a [Rect],
    pub(super) gradients: &'a mut GradientPass<'g>,
    pub(super) paint_animation_cursor: PaintAnimationCursor<'a>,
    /// Live `GpuView`s by owner `WidgetId`, one map across layers; an `ImageSource::GpuView` carries only its epoch.
    pub(super) gpu_views: &'a GpuViews,
    pub(super) damage_filter: Option<&'a DamageRegion>,
    /// Logical-px inflation of each `subtree_paint_rect` before the damage cull, covering the AA pad the backend PreClears.
    pub(super) damage_cull_margin: f32,
    pub(super) viewport: Rect,
    pub(super) now: Duration,
}

impl PlaneCtx<'_, '_> {
    #[inline]
    fn brush_source(&mut self, brush: ShapeBrush) -> BrushSource {
        self.gradients.source(brush)
    }

    fn gradient_row(&mut self, id: GradientId) -> LutRow {
        self.gradients.resolve(id).lut_row
    }

    /// Emits one of a node's shapes; `runs` is that node's [`TextRuns`] cursor, advanced here.
    fn emit_one_shape(
        &mut self,
        item: ShapeItem<'_>,
        runs: &mut TextRuns,
        out: &mut impl PaintSink,
    ) {
        let ShapeItem {
            owner: id,
            idx: shape_idx,
            record: shape,
        } = item;
        // Shape setters check every input, so nothing non-finite reaches here; later non-finite checks guard the transform stack only.
        // No "not a no-op" assert: that needs resolved geometry, which first exists in `Draw*Payload::is_noop`.
        debug_assert!(
            !shape.has_nan(),
            "a NaN reached the encoder past the shape setters' checks: {shape:?}",
        );
        let shaped = runs.shaped(shape, self.layout);
        // Early-out so a shape animated to nothing doesn't pay for its geometry; `paint_mod.rotation` rides the stroke arms via `StrokeBounds`.
        let paint_mod = self.paint_animation_cursor.sample(shape_idx, self.now);
        if is_invisible(paint_mod.alpha) {
            return;
        }
        let alpha = paint_mod.alpha;
        let owner_rect = self.layout.rect[id.idx()];
        // The turn for payloads placed in the encoder's logical frame; a payload whose points are
        // relative to an origin of its own takes `Spin::about_owner` with that origin.
        let spin = Spin::about_owner(owner_rect, Vec2::ZERO, paint_mod.rotation);
        match shape {
            ShapeRecord::Quad(shape) => match shape {
                QuadShape::Rect {
                    kind,
                    local_rect,
                    corners,
                    fill,
                    border,
                    ..
                } => {
                    let rect = owner_rect.resolve_local(*local_rect);
                    let fill = self.brush_source(*fill);
                    out.draw_quad(
                        DrawQuadPayload::rect(RectQuad {
                            kind: *kind,
                            rect,
                            corners: *corners,
                            fill,
                            stroke: *border,
                        })
                        .turned(spin),
                        alpha,
                    );
                }
                QuadShape::Shadow {
                    local_rect,
                    corners,
                    shadow,
                } => {
                    // The composer grows a drop shadow from the source rect in physical pixels, so it lands on the same pixels as the fill.
                    let source = owner_rect.resolve_local(*local_rect);
                    if let Some(shadow) = DrawQuadPayload::shadow(source, *corners, *shadow) {
                        out.draw_quad(shadow.turned(spin), alpha);
                    }
                }
                QuadShape::Triangle {
                    a,
                    b,
                    c,
                    radius,
                    fill,
                    border,
                    bbox: _,
                } => {
                    // Owner-local corner points, turned here by a paint animation; the composer folds in `origin` and the transform. Solid fill only: the reused quad lanes have no room for a gradient.
                    let mut points = [*a, *b, *c];
                    if let Some(spin) =
                        Spin::about_owner(owner_rect, owner_rect.min, paint_mod.rotation)
                    {
                        let rotor = spin.rotor();
                        points = points.map(|q| rotor.apply(q));
                    }
                    out.draw_quad(
                        DrawQuadPayload::triangle(TriangleQuad {
                            origin: owner_rect.min,
                            points,
                            radius: *radius,
                            fill: *fill,
                            stroke: *border,
                        }),
                        alpha,
                    );
                }
            },
            ShapeRecord::Text {
                local_origin,
                text,
                color,
                align,
                ..
            } => {
                let shaped = shaped.expect("a text record always draws its run from the cursor");
                let Some(key) = shaped.key else {
                    tracing::trace!(?shape, "encoder: dropping text that shaped no buffer");
                    return;
                };
                // `local_rect: None`: the encoder places the bbox in the owner's padded inner rect via `Align::place_in`; `Some(origin)`: the widget places it at `owner.min + origin`. Shared placement, so the cascade's paint rect and damage agree with paint.
                let anchor = TextAnchor {
                    local_origin: *local_origin,
                    align: *align,
                };
                let local = anchor.bbox_local(
                    owner_rect.size,
                    self.tree.records.layout()[id.idx()].padding,
                    shaped.extent.size,
                );
                let rect = owner_rect.resolve_local(Some(local));
                out.draw_text(
                    DrawTextPayload {
                        rect,
                        ink: shaped.extent.ink,
                        color: *color,
                        text: ShapedTextRef::new(key, text),
                        spin,
                    },
                    alpha,
                );
            }
            ShapeRecord::Polyline {
                width,
                color_mode,
                cap,
                join,
                points,
                colors,
                bbox,
                content_hash: _,
            } => {
                out.draw_polyline(
                    DrawPolylinePayload {
                        bounds: StrokeBounds::new(owner_rect, *bbox, paint_mod.rotation),
                        origin: owner_rect.min,
                        width: *width,
                        points: *points,
                        colors: *colors,
                        color_mode: *color_mode,
                        cap: *cap,
                        join: *join,
                        alpha: 1.0,
                    },
                    alpha,
                );
            }
            ShapeRecord::Mesh {
                local_rect,
                tint,
                vertices,
                idxs,
                bbox,
                content_hash: _,
            } => {
                let origin = owner_rect.resolve_local(*local_rect).min;
                out.draw_mesh(
                    DrawMeshPayload {
                        bbox: *bbox,
                        origin,
                        tint: *tint,
                        vertices: *vertices,
                        idxs: *idxs,
                        spin: Spin::about_owner(owner_rect, origin, paint_mod.rotation),
                    },
                    alpha,
                );
            }
            ShapeRecord::Curve {
                cap,
                basis,
                stroke,
                bbox,
                ramp,
            } => {
                // Owner-local; the basis crosses verbatim so cull, spin and sub-instance sizing stay one code path.
                let ramp_row = match *ramp {
                    CurveRamp::None => None,
                    CurveRamp::Interned { id, hash: _ } => Some(self.gradient_row(id)),
                };
                out.draw_curve(
                    DrawCurvePayload {
                        basis: *basis,
                        bounds: StrokeBounds::new(owner_rect, *bbox, paint_mod.rotation),
                        origin: owner_rect.min,
                        fill: GpuFill::curve(stroke.color, ramp_row),
                        width: stroke.width,
                        cap: *cap,
                    },
                    alpha,
                );
            }
            ShapeRecord::Icon {
                local_rect,
                handle,
                fit,
                tint,
                desaturate,
            } => {
                let base = owner_rect.resolve_local(*local_rect);
                out.draw_icon(
                    DrawIconPayload {
                        rect: fit.resolve(base, handle.view_box()),
                        icon: handle.icon,
                        tint: *tint,
                        desaturate: *desaturate,
                        spin,
                    },
                    alpha,
                );
            }
            ShapeRecord::Image {
                local_rect,
                tint,
                source,
                fit,
                min_filter,
                mag_filter,
                downsample,
            } => {
                let base = owner_rect.resolve_local(*local_rect);
                // A registered image carries its id; a `GpuView` is looked up in `Ui::gpu_views` and its paint callback rides with the payload, with the epoch telling the backend whether the target is current.
                let (handle, view) = match source {
                    ImageSource::Texture { id, .. } => (*id, None),
                    ImageSource::GpuView { epoch } => {
                        let widget_id = self.tree.records.widget_id()[id.idx()];
                        let view = self.gpu_views.view(widget_id);
                        let paint = ViewPaint {
                            paint: &view.paint,
                            epoch: *epoch,
                        };
                        (view.texture_id, Some(paint))
                    }
                };
                let FitRect {
                    rect,
                    uv_min,
                    uv_size,
                } = fit.resolve(base, source.intrinsic());
                let mut flags = ImageFlags::NONE;
                if matches!(*fit, ImageFit::Tile { .. }) {
                    flags = flags.union(ImageFlags::TILED);
                }
                if *min_filter == ImageFilter::Nearest {
                    flags = flags.union(ImageFlags::MIN_NEAREST);
                }
                if *mag_filter == ImageFilter::Nearest {
                    flags = flags.union(ImageFlags::MAG_NEAREST);
                }
                flags = flags.union(match *downsample {
                    ImageDownsample::Single => ImageFlags::NONE,
                    ImageDownsample::Mean => ImageFlags::TAPS_MEAN,
                    ImageDownsample::Peak => ImageFlags::TAPS_PEAK,
                });
                out.draw_image(
                    ImageDraw {
                        payload: DrawImagePayload {
                            rect,
                            uv_min,
                            uv_size,
                            tint: *tint,
                            handle,
                            flags,
                            spin,
                        },
                        view,
                    },
                    alpha,
                );
            }
        }
    }

    /// Paints `id` and its subtree in paint order: the whole recursive walk, called once per root.
    pub(super) fn encode_node(&mut self, id: NodeId, out: &mut impl PaintSink) {
        if self.cascade_inputs[id.idx()].invisible() {
            return;
        }

        let subtree_paint_rect = self.subtree_paint_rects[id.idx()];
        if !subtree_paint_rect.intersects(self.viewport) {
            return;
        }

        // Damage cull, inflated by `damage_cull_margin` to cover the AA pad the backend PreClears, else a node in the pad ring is cleared but skipped.
        if let Some(region) = self.damage_filter
            && !region.any_intersects(subtree_paint_rect.inflated(self.damage_cull_margin))
        {
            return;
        }

        let rect = self.layout.rect[id.idx()];

        // Clip is in parent space (pre-transform); the transform applies inside it, to children only. Chrome paints before the clip is pushed: `Tree::open_node` folds its border into the padding that deflates the clip, and chrome self-clips via its SDF. Chrome is `None` only when every part is no-op.
        let mode = self.tree.records.attrs()[id.idx()].clip_mode();
        let clip = mode.is_clip();
        let chrome = self.tree.chrome(id);

        if let Some(chrome) = chrome {
            // Alpha `1.0`: chrome isn't a shape, so no paint animation. CSS Backgrounds 3 §7.1 order: drop shadow under the fill, inset over it.
            let fill = DrawQuadPayload::rect(RectQuad {
                kind: RectKind::Rounded,
                rect,
                corners: chrome.corners,
                fill: self.brush_source(chrome.fill),
                stroke: chrome.border,
            });
            if chrome.shadow.inset() {
                out.draw_quad(fill, 1.0);
                let width = chrome.border.width;
                let padding_box = Rect {
                    min: Vec2::ZERO,
                    size: rect.size,
                }
                .deflated(width);
                let source = rect.resolve_local(Some(padding_box));
                let corners = chrome
                    .corners
                    .inner(rect.size, Spacing::saturating([width; 4]));
                if let Some(shadow) = DrawQuadPayload::shadow(source, corners, chrome.shadow) {
                    out.draw_quad(shadow, 1.0);
                }
            } else {
                // The owner's rect, as `compute_paint_rect` mirrors.
                if let Some(shadow) = DrawQuadPayload::shadow(rect, chrome.corners, chrome.shadow) {
                    out.draw_quad(shadow, 1.0);
                }
                out.draw_quad(fill, 1.0);
            }
            if chrome.ring {
                let ring = DrawQuadPayload::rect(RectQuad {
                    kind: RectKind::Rounded,
                    rect,
                    corners: chrome.corners,
                    fill: self.brush_source(ShapeBrush::Solid(RgbaF16::TRANSPARENT)),
                    stroke: self.tree.focus_ring,
                });
                out.draw_quad(ring, 1.0);
            }
        }

        if clip {
            let layout = self.tree.records.layout()[id.idx()];
            let mask_rect = layout.inner_rect(rect);
            match mode {
                ClipMode::Rect => out.push_clip(PushClipPayload::rect(mask_rect)),
                ClipMode::Rounded => {
                    let painted = chrome.map(|row| row.corners).expect(
                        "ClipMode::Rounded without chrome row — open_node invariant violated",
                    );
                    out.push_clip(PushClipPayload {
                        rect: mask_rect,
                        corners: painted.inner(rect.size, layout.padding),
                    });
                }
                ClipMode::None => {}
            }
        }

        let transform = self.tree.anchored_transform(id, rect);

        // The body paints inside the node's transform; chrome stays in parent space, so a transform pans/zooms content while the chrome stays anchored.
        if let Some(t) = transform {
            out.push_transform(t);
        }
        let mut runs = TextRuns::new(self.layout.text_spans[id.idx()]);
        let tree = self.tree;
        for item in tree.tree_items(id) {
            match item {
                TreeItem::ShapeRecord(idx, record) => {
                    let item = ShapeItem {
                        owner: id,
                        idx,
                        record,
                    };
                    self.emit_one_shape(item, &mut runs, out);
                }
                TreeItem::Child(child) => {
                    self.encode_node(child.id, out);
                }
            }
        }
        debug_assert!(
            runs.is_drained(),
            "encoder text count differs from the node's shaped-text span",
        );
        if transform.is_some() {
            out.pop_transform();
        }

        if clip {
            out.pop_clip();
        }
    }
}
