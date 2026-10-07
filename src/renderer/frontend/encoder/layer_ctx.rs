//! [`LayerCtx`]: one layer's encode walk, built per tree by [`Encoder::encode`](crate::renderer::frontend::encoder::Encoder::encode).

use crate::cascade::cascade_input_hash::CascadeInputHash;
use crate::damage::region::DamageRegion;
use crate::layout::layer_layout::LayerLayout;
use crate::layout::text::text_runs::TextRuns;
use crate::primitives::geometry::corners::Corners;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::layout::clip_mode::ClipMode;
use crate::primitives::math::domain::is_invisible;
use crate::primitives::math::nan::NanCheck;
use crate::primitives::packed::fill_axis::FillAxis;
use crate::primitives::packed::fill_kind::FillKind;
use crate::primitives::paint::color::rgba_f16::RgbaF16;
use crate::primitives::paint::image::{FitRect, ImageDownsample, ImageFilter, ImageFit};
use crate::primitives::paint::lut_row::LutRow;
use crate::renderer::frontend::encoder::GradientPass;
use crate::renderer::frontend::encoder::geometry;
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::brush_source::BrushSource;
use crate::renderer::frontend::payload::draw_curve_payload::DrawCurvePayload;
use crate::renderer::frontend::payload::draw_icon_payload::DrawIconPayload;
use crate::renderer::frontend::payload::draw_image_payload::{
    DrawImagePayload, ImageDraw, ViewPaint,
};
use crate::renderer::frontend::payload::draw_mesh_payload::DrawMeshPayload;
use crate::renderer::frontend::payload::draw_polyline_payload::DrawPolylinePayload;
use crate::renderer::frontend::payload::draw_quad_payload::DrawQuadPayload;
use crate::renderer::frontend::payload::draw_text_payload::DrawTextPayload;
use crate::renderer::frontend::payload::gpu_fill::GpuFill;
use crate::renderer::frontend::payload::push_clip_payload::PushClipPayload;
use crate::renderer::frontend::payload::stroke_bounds::StrokeBounds;
use crate::renderer::gpu_paint::gpu_views::GpuViews;
use crate::renderer::render_buffer::image_flags::ImageFlags;
use crate::scene::record_store::recorded_gradients::GradientId;
use crate::scene::tree::Tree;
use crate::scene::tree::iter::TreeItem;
use crate::scene::tree::node_id::NodeId;
use crate::scene::tree::paint_anims::PaintAnimCursor;
use crate::shape::paint::image_source::ImageSource;
use crate::shape::paint::lowered_shadow::LoweredShadow;
use crate::shape::paint::quad_shape::QuadShape;
use crate::shape::paint::shape_brush::CurveRamp;
use crate::shape::paint::shape_brush::ShapeBrush;
use crate::shape::record::{self, ShapeRecord};
use crate::text::shaped_ref::ShapedTextRef;
use glam::Vec2;
use std::time::Duration;

/// The fixed inputs one layer's encode walk reads.
#[derive(Debug)]
pub(super) struct LayerCtx<'a, 'g> {
    pub(super) tree: &'a Tree,
    pub(super) layout: &'a LayerLayout,
    pub(super) cascade_inputs: &'a [CascadeInputHash],
    pub(super) subtree_paint_rects: &'a [Rect],
    pub(super) gradients: &'a mut GradientPass<'g>,
    pub(super) paint_anim_cursor: PaintAnimCursor<'a>,
    /// Live `GpuView`s by owner `WidgetId`, one map across layers; an `ImageSource::GpuView` carries only its epoch.
    pub(super) gpu_views: &'a GpuViews,
    pub(super) damage_filter: Option<&'a DamageRegion>,
    /// Logical-px inflation of each `subtree_paint_rect` before the damage cull, covering the AA pad the backend PreClears.
    pub(super) damage_cull_margin: f32,
    pub(super) viewport: Rect,
    pub(super) now: Duration,
}

impl LayerCtx<'_, '_> {
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
        id: NodeId,
        shape_idx: u32,
        shape: &ShapeRecord,
        runs: &mut TextRuns,
        out: &mut impl PaintSink,
    ) {
        // `Shapes::add` is the single NaN gate, so nothing non-finite reaches here; later non-finite checks guard the transform stack only.
        // No "not a no-op" assert: that needs resolved geometry, which first exists in `Draw*Payload::is_noop`.
        debug_assert!(
            !shape.has_nan(),
            "a NaN reached the encoder — `Shapes::add`'s gate should have \
             dropped this shape: {shape:?}",
        );
        let shaped = runs.shaped(shape, self.layout);
        // Early-out so a shape animated to nothing doesn't pay for its geometry; `paint_mod.rotation` rides the stroke arms via `StrokeBounds`.
        let paint_mod = self.paint_anim_cursor.sample(shape_idx, self.now);
        if is_invisible(paint_mod.alpha) {
            return;
        }
        let alpha = paint_mod.alpha;
        let owner_rect = self.layout.rect[id.idx()];
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
                    let r = geometry::resolve_local_rect(owner_rect, *local_rect);
                    let src = self.brush_source(*fill);
                    out.draw_quad(
                        DrawQuadPayload::rect_of_kind(*kind, r, *corners, src, *border),
                        alpha,
                    );
                }
                QuadShape::Shadow {
                    local_rect,
                    corners,
                    shadow,
                } => emit_shadow(out, owner_rect, *local_rect, *corners, shadow, alpha),
                QuadShape::Triangle {
                    a,
                    b,
                    c,
                    radius,
                    fill,
                    border,
                    bbox: _,
                } => {
                    // Owner-local corner points; the composer folds in `origin` and the transform. Solid fill only: the reused quad lanes have no room for a gradient.
                    out.draw_quad(
                        DrawQuadPayload::triangle(
                            owner_rect.min,
                            [*a, *b, *c],
                            *fill,
                            *radius,
                            *border,
                        ),
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
                let local = record::text_paint_bbox_local(
                    *local_origin,
                    *align,
                    self.tree.records.layout()[id.idx()].padding,
                    owner_rect.size,
                    shaped.extent.size,
                );
                let rect = geometry::resolve_local_rect(owner_rect, Some(local));
                out.draw_text(
                    DrawTextPayload {
                        rect,
                        ink: shaped.extent.ink,
                        color: *color,
                        text: ShapedTextRef::new(key, text),
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
                        points_start: points.start,
                        points_len: points.len,
                        colors_start: colors.start,
                        colors_len: colors.len,
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
                indices,
                bbox,
                content_hash: _,
            } => {
                let origin = geometry::resolve_local_rect(owner_rect, *local_rect).min;
                out.draw_mesh(
                    DrawMeshPayload {
                        bbox: *bbox,
                        origin,
                        tint: *tint,
                        v_start: vertices.start,
                        v_len: vertices.len,
                        i_start: indices.start,
                        i_len: indices.len,
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
                let base = geometry::resolve_local_rect(owner_rect, *local_rect);
                out.draw_icon(
                    DrawIconPayload {
                        rect: fit.resolve(base, handle.view_box()),
                        icon: handle.icon,
                        tint: *tint,
                        desaturate: *desaturate,
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
                let base = geometry::resolve_local_rect(owner_rect, *local_rect);
                // A registered image carries its id; a `GpuView` is looked up in `Ui::gpu_views` and its paint callback rides with the payload, with the epoch telling the backend whether the target is current.
                let (handle, view) = match source {
                    ImageSource::Texture { id, .. } => (*id, None),
                    ImageSource::GpuView { epoch } => {
                        let wid = self.tree.records.widget_id()[id.idx()];
                        let view = self.gpu_views.view(wid);
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

        if let Some(bg) = chrome {
            // Alpha `1.0`: chrome isn't a shape, so no paint animation. CSS Backgrounds 3 §7.1 order: drop shadow under the fill, inset over it.
            let src = self.brush_source(bg.fill);
            let fill = DrawQuadPayload::rect(rect, bg.corners, src, bg.border);
            if bg.shadow.inset() {
                out.draw_quad(fill, 1.0);
                let width = bg.border.width;
                let padding_box = Rect {
                    min: Vec2::ZERO,
                    size: rect.size,
                }
                .deflated(width);
                let corners = bg.corners.deflated(rect.size, width);
                emit_shadow(out, rect, Some(padding_box), corners, &bg.shadow, 1.0);
            } else {
                // `local_rect = None`: the owner's rect, as `compute_paint_rect` mirrors.
                emit_shadow(out, rect, None, bg.corners, &bg.shadow, 1.0);
                out.draw_quad(fill, 1.0);
            }
            if bg.ring {
                let clear = self.brush_source(ShapeBrush::Solid(RgbaF16::TRANSPARENT));
                let ring = self.tree.focus_ring;
                out.draw_quad(DrawQuadPayload::rect(rect, bg.corners, clear, ring), 1.0);
            }
        }

        if clip {
            let layout = self.tree.records.layout()[id.idx()];
            let mask_rect = layout.inner_rect(rect);
            match mode {
                ClipMode::Rect => out.push_clip(PushClipPayload::rect(mask_rect)),
                ClipMode::Rounded => {
                    // Per-corner reduction by the larger adjacent edge inset, keeping the mask curve inside both edges.
                    let painted = chrome.map(|bg| bg.corners).expect(
                        "ClipMode::Rounded without chrome row — open_node invariant violated",
                    );
                    let [ptl, ptr_, pbr, pbl] = painted.as_array();
                    let [pl, pt, pr, pb] = layout.padding.as_array();
                    let mask_radius = Corners::new(
                        (ptl - pt.max(pl)).max(0.0),
                        (ptr_ - pt.max(pr)).max(0.0),
                        (pbr - pb.max(pr)).max(0.0),
                        (pbl - pb.max(pl)).max(0.0),
                    );
                    out.push_clip(PushClipPayload {
                        rect: mask_rect,
                        corners: mask_radius,
                    });
                }
                ClipMode::None => {}
            }
        }

        let transform = self.tree.anchored_transform(id, rect);

        // The body paints inside the node's transform; chrome stays in parent space, so a transform pans/zooms content while the background stays anchored.
        if let Some(t) = transform {
            out.push_transform(t);
        }
        let mut runs = TextRuns::new(self.layout.text_spans[id.idx()]);
        let tree = self.tree;
        for item in tree.tree_items(id) {
            match item {
                TreeItem::ShapeRecord(shape_idx, shape) => {
                    self.emit_one_shape(id, shape_idx, shape, &mut runs, out);
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

/// Shared shadow emit. The composer grows a drop shadow from the source rect in physical pixels, so it lands on the same pixels as the fill.
fn emit_shadow(
    out: &mut impl PaintSink,
    owner_rect: Rect,
    local_rect: Option<Rect>,
    corners: Corners,
    shadow: &LoweredShadow,
    alpha: f32,
) {
    if shadow.is_noop() {
        return;
    }
    let source = match local_rect {
        Some(local) => Rect {
            min: owner_rect.min + local.min,
            size: local.size,
        },
        None => owner_rect,
    };
    let kind = if shadow.inset() {
        FillKind::SHADOW_INSET
    } else {
        FillKind::SHADOW_DROP
    };
    // The axis travels as the packed word; unpacking would round-trip identical bytes through f16.
    let fill_axis = FillAxis::from(shadow.geom_f16);
    out.draw_quad(
        DrawQuadPayload::shadow(source, corners, shadow.color, kind, fill_axis),
        alpha,
    );
}
