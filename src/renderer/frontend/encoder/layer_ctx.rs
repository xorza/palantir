//! [`LayerCtx`] — one layer's encode walk.
//!
//! [`Encoder::encode`](crate::renderer::frontend::encoder::Encoder::encode)
//! builds one of these per tree in the scene forest and hands it its roots;
//! everything from there down to the emitted paint commands is the recursion
//! below.

use crate::layout::LayerLayout;
use crate::layout::text_runs::TextRuns;
use crate::layout::types::clip_mode::ClipMode;
use crate::primitives::approx::noop_f32;
use crate::primitives::brush::gradient::FillAxis;
use crate::primitives::corners::Corners;
use crate::primitives::fill_kind::FillKind;
use crate::primitives::image::{ImageDownsample, ImageFilter, ImageFit};
use crate::primitives::nan::NanCheck;
use crate::primitives::rect::Rect;
use crate::renderer::frontend::encoder::GradientResolver;
use crate::renderer::frontend::encoder::geometry;
use crate::renderer::frontend::encoder::geometry::Resolved;
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::brush_source::BrushSource;
use crate::renderer::frontend::payload::draw_curve_payload::DrawCurvePayload;
use crate::renderer::frontend::payload::draw_icon_payload::DrawIconPayload;
use crate::renderer::frontend::payload::draw_image_payload::{DrawImagePayload, ImageDraw};
use crate::renderer::frontend::payload::draw_mesh_payload::DrawMeshPayload;
use crate::renderer::frontend::payload::draw_polyline_payload::DrawPolylinePayload;
use crate::renderer::frontend::payload::draw_quad_payload::DrawQuadPayload;
use crate::renderer::frontend::payload::draw_text_payload::DrawTextPayload;
use crate::renderer::frontend::payload::push_clip_payload::PushClipPayload;
use crate::renderer::frontend::payload::stroke_bounds::StrokeBounds;
use crate::renderer::gpu_paint::gpu_views::GpuViews;
use crate::renderer::gradient_atlas::shared_gradient_atlas::SharedGradientAtlas;
use crate::renderer::render_buffer::image::{
    IMG_FLAG_MAG_NEAREST, IMG_FLAG_MIN_NEAREST, IMG_FLAG_TAPS_MEAN, IMG_FLAG_TAPS_PEAK,
    IMG_FLAG_TILED,
};
use crate::scene::cascade::CascadeInputHash;
use crate::scene::damage::region::DamageRegion;
use crate::scene::record_store::recorded_gradient::RecordedGradient;
use crate::scene::shapes::paint::{ImageSource, LoweredShadow, QuadShape, ShadowGeom, ShapeBrush};
use crate::scene::shapes::record::{self, ShapeRecord};
use crate::scene::tree::Tree;
use crate::scene::tree::iter::TreeItem;
use crate::scene::tree::node_id::NodeId;
use crate::scene::tree::paint_anims::PaintAnimCursor;
use crate::shape::rect::RectKind;
use crate::text::shaped_ref::ShapedTextRef;
use glam::UVec2;
use std::time::Duration;

/// Per-layer encode context: the fixed inputs one layer's walk reads,
/// bundled so [`Self::encode_node`]'s recursion carries one `&mut self`
/// instead of a long argument list.
#[derive(Debug)]
pub(super) struct LayerCtx<'a> {
    pub(super) tree: &'a Tree,
    pub(super) layout: &'a LayerLayout,
    pub(super) cascade_inputs: &'a [CascadeInputHash],
    pub(super) subtree_paint_rects: &'a [Rect],
    pub(super) gradients: &'a [RecordedGradient],
    pub(super) gradient_atlas: &'a SharedGradientAtlas,
    pub(super) gradient_resolver: &'a mut GradientResolver,
    pub(super) paint_anim_cursor: PaintAnimCursor<'a>,
    /// Live `GpuView`s by `WidgetId`, one map across every layer. An
    /// `ImageSource::GpuView` carries only its epoch; the arm looks the
    /// view's stable `TextureId` + paint callback up here by the owner
    /// node's id.
    pub(super) gpu_views: &'a GpuViews,
    pub(super) damage_filter: Option<&'a DamageRegion>,
    /// Logical-px inflation applied to each node's `subtree_paint_rect`
    /// before the damage-cull intersection test, so the cull covers the
    /// AA-padded region the backend PreClears (see
    /// [`Encoder::encode`](crate::renderer::frontend::encoder::Encoder::encode)).
    pub(super) damage_cull_margin: f32,
    pub(super) viewport: Rect,
    pub(super) now: Duration,
}

impl LayerCtx<'_> {
    #[inline]
    fn brush_source(&mut self, brush: ShapeBrush) -> BrushSource {
        self.gradient_resolver
            .source(self.gradients, self.gradient_atlas, brush)
    }

    /// Emit one of a node's shapes. Pulled out of `encode_node` so the
    /// child-interleave loop can call it without duplicating the per-variant
    /// match; `runs` is that node's [`TextRuns`] cursor, advanced here.
    fn emit_one_shape(
        &mut self,
        id: NodeId,
        shape_idx: u32,
        shape: &ShapeRecord,
        runs: &mut TextRuns,
        out: &mut impl PaintSink,
    ) {
        // **The lowered-shape invariant**, asserted at the one point every
        // lowered shape passes through. `Shapes::add` is the single gate
        // that decides NaN and it drops what it finds, so nothing carrying
        // one can arrive here.
        //
        // This is what lets the tiers below stop re-asking. The only
        // non-finite checks past this point guard a different source — the
        // *transform* stack, whose scale can overflow independently of any
        // shape input (see `urect_from_phys`) — not shape geometry.
        //
        // Deliberately not paired with a "not a no-op" assert: whether a
        // record paints is not answerable here. `local_rect: None` means
        // "the owner's arranged rect", and text's extent comes from the
        // shaped measure, so both are layout outputs the record doesn't
        // carry. That question belongs to `Draw*Payload::is_noop`, one tier
        // down, which is the first place the resolved geometry exists.
        debug_assert!(
            !shape.has_nan(),
            "a NaN reached the encoder — `Shapes::add`'s gate should have \
             dropped this shape: {shape:?}",
        );
        // Ahead of the two gates below, because the cursor counts *records*:
        // a run either of them drops still owns its slot in the node's span.
        let shaped = runs.shaped(shape, self.layout);
        // Every emit below passes `alpha`, and the sink's `draw_*` folds it
        // into whichever lane that payload carries its colour in. An
        // early-out rather than the gate: the sink drops a fully faded
        // payload anyway, but a shape animated to nothing should not pay
        // for its geometry first. `paint_mod.rotation` rides the stroke
        // arms instead, through `StrokeBounds`.
        let paint_mod = self.paint_anim_cursor.sample(shape_idx, self.now);
        if noop_f32(paint_mod.alpha) {
            return;
        }
        let alpha = paint_mod.alpha;
        // The node's arranged rect, which every shape resolves its geometry
        // against. Read here rather than passed in: it is `layout.rect[id]`,
        // the same column this function already indexes for padding and text
        // spans, so taking it as an argument only made two spellings of one
        // value that could disagree.
        let owner_rect = self.layout.rect[id.idx()];
        match shape {
            // The three quad-tier shapes resolve their geometry against the
            // owner rect and hand it to the one `PaintSink` quad path; from
            // the payload down they are a single draw.
            ShapeRecord::Quad(shape) => match shape {
                QuadShape::Rect {
                    kind,
                    local_rect,
                    corners,
                    fill,
                    stroke,
                    ..
                } => {
                    let r = geometry::resolve_local_rect(owner_rect, *local_rect);
                    let src = self.brush_source(*fill);
                    match kind {
                        RectKind::Rounded => {
                            out.draw_quad(DrawQuadPayload::rect(r, *corners, src, *stroke), alpha)
                        }
                        RectKind::Windowed => out.draw_quad(
                            DrawQuadPayload::rect_window(r, *corners, src, *stroke),
                            alpha,
                        ),
                    }
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
                    stroke,
                    bbox: _,
                } => {
                    // Corner points are owner-local; the composer folds `origin` +
                    // the active transform and derives the covering AABB. Solid
                    // fill only — the reused quad lanes have no room for a gradient.
                    // Stroke noop-normalization happens inside
                    // `DrawQuadPayload::triangle`, the single canonical gate.
                    out.draw_quad(
                        DrawQuadPayload::triangle(
                            owner_rect.min,
                            [*a, *b, *c],
                            *fill,
                            *radius,
                            *stroke,
                        ),
                        alpha,
                    )
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
                // Two paths share the same `DrawText` payload:
                // - `local_rect: None` → encoder owns positioning. Place
                //   the shaped bbox inside the owner's padded inner rect
                //   via `Align::place_in`.
                // - `local_rect: Some(origin)` → widget owns positioning.
                //   Origin is `owner.min + origin`; bbox size is the
                //   shaped measurement. `align`'s placement axes are
                //   ignored (only `align.halign()` matters here, and
                //   that's already baked into the shaped buffer's
                //   per-line glyph offsets).
                // Through the shared placement, then lifted — the
                // cascade's paint rect for this same run is built from
                // it, and the two have to name the same pixels or damage
                // and paint disagree about where the glyphs are.
                let local = record::text_paint_bbox_local(
                    *local_origin,
                    *align,
                    self.tree.records.layout()[id.idx()].padding,
                    owner_rect.size,
                    shaped.measured,
                );
                let rect = geometry::resolve_local_rect(owner_rect, Some(local));
                out.draw_text(
                    DrawTextPayload {
                        rect,
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
                // Points + colors live in the window's RecordStore; spans
                // are forwarded verbatim. Owner-local convention — the
                // composer folds `origin` into the per-point transform.
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
                        alpha: u8::MAX,
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
                // Verts live in the window's RecordStore owner-local;
                // composer folds `origin` into the per-instance translate.
                // No per-frame copy here.
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
                basis,
                width,
                fill,
                fill_grad_hash: _,
                cap,
                bbox,
            } => {
                // Curves are owner-local; composer adds `origin` + active
                // transform before scaling to physical px. Curves carry no
                // gradient axis, so `fill.axis` goes unread. The basis
                // crosses verbatim — record and payload share the type, so
                // both bases' cull, spin, and sub-instance sizing stay one
                // code path from here through the composer.
                out.draw_curve(
                    DrawCurvePayload {
                        basis: *basis,
                        bounds: StrokeBounds::new(owner_rect, *bbox, paint_mod.rotation),
                        origin: owner_rect.min,
                        fill: self.brush_source(*fill).gpu_fill(),
                        width: *width,
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
                        rect: geometry::resolve_icon_fit(base, handle.view_box, *fit),
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
                // The one thing the two sources don't share: where the
                // texture comes from. A registered image carries its id +
                // intrinsic dims inline (no registry borrow); a `GpuView`
                // looks its stable target up in `Ui::gpu_views` by the owner
                // node's `WidgetId` and hands back the app paint callback,
                // which rides alongside the payload so the sink can list the
                // off-screen target in `frame_targets`. A view reports an
                // all-zero intrinsic size, which makes `resolve_fit` fall
                // through to the base rect + full UV — the full-rect,
                // untinted composite a view has always emitted.
                // `epoch` only affects the shape hash (damage), not the draw.
                let (handle, size, paint) = match source {
                    ImageSource::Texture { id, size, .. } => (*id, *size, None),
                    ImageSource::GpuView { epoch: _ } => {
                        let wid = self.tree.records.widget_id()[id.idx()];
                        let view = self.gpu_views.view(wid);
                        (view.texture_id, UVec2::ZERO, Some(&view.paint))
                    }
                };
                let Resolved {
                    rect,
                    uv_min,
                    uv_size,
                } = geometry::resolve_fit(base, size.as_vec2(), *fit);
                let mut flags = 0;
                if matches!(*fit, ImageFit::Tile { .. }) {
                    flags |= IMG_FLAG_TILED;
                }
                if *min_filter == ImageFilter::Nearest {
                    flags |= IMG_FLAG_MIN_NEAREST;
                }
                if *mag_filter == ImageFilter::Nearest {
                    flags |= IMG_FLAG_MAG_NEAREST;
                }
                // At most one tap bit: the shader reads them as a mode, not a set.
                match *downsample {
                    ImageDownsample::Single => {}
                    ImageDownsample::Mean => flags |= IMG_FLAG_TAPS_MEAN,
                    ImageDownsample::Peak => flags |= IMG_FLAG_TAPS_PEAK,
                }
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
                        paint,
                    },
                    alpha,
                );
            }
        }
    }

    /// Paint `id` and its subtree, in paint order.
    ///
    /// Recursive, and the whole walk: the invisible and damage-cull gates,
    /// chrome, the clip push/pop pair, and the interleave of a node's own
    /// shapes with its children all happen here. Called once per root by
    /// [`Encoder::encode`](crate::renderer::frontend::encoder::Encoder::encode).
    pub(super) fn encode_node(&mut self, id: NodeId, out: &mut impl PaintSink) {
        if self.cascade_inputs[id.idx()].invisible() {
            return;
        }

        // Off-screen subtree cull. Reads `Cascade::subtree_paint_rects`
        // — the rolled-up paint bound that includes every descendant —
        // so a Canvas-positioned child overflowing its parent's `Fixed`
        // bound (or a shape with negative-margin overhang) doesn't get
        // killed when the parent's own rect lies just outside the
        // viewport. The parallel column is owner-local to this layer.
        let subtree_paint_rect = self.subtree_paint_rects[id.idx()];
        if !subtree_paint_rect.intersects(self.viewport) {
            return;
        }

        // DamageEngine-aware subtree cull. Same shape as the viewport
        // cull: if no damage rect intersects the subtree paint bound,
        // the whole subtree contributes nothing this frame — skip
        // recursion + Push/Pop emission entirely. `subtree_paint_rect`
        // covers descendants too, so a horizontal pan that translates
        // an overhanging port circle into the damage region still
        // recurses through the (potentially own-rect-tight) ancestor.
        //
        // Inflate by `damage_cull_margin` so the cull covers the AA-padded
        // region the backend PreClears, not just the raw damage rect. A
        // node whose paint bound lands in that pad ring (near a moving
        // shape's bbox edge — e.g. a bezier wire dragged past a node border
        // or port circle) would otherwise be cleared but skipped here,
        // leaving a hard cut along the wire's bbox boundary.
        if let Some(region) = self.damage_filter
            && !region.any_intersects(subtree_paint_rect.inflated(self.damage_cull_margin))
        {
            return;
        }

        let rect = self.layout.rect[id.idx()];

        // Order: clip is in parent-of-panel space (pre-transform); transform
        // applies inside the clip and only to children. The panel's own
        // background paints under the clip but BEFORE the transform — matching
        // WPF's `RenderTransform` convention.
        //
        // Chrome paints BEFORE the clip is pushed: `Tree::open_node` folds
        // the chrome's stroke width into the padding that deflates the clip
        // (and, for `ClipMode::Rounded`, insets the mask), so chrome's own
        // stroke pixels sit outside the mask. Painting chrome first leaves it
        // unclipped — it self-clips via its SDF — which preserves the stroke
        // ring while children stay clipped to the inset interior.
        //
        // `Tree::open_node` drops chrome to `None` only when every paintable
        // part is no-op. Both `DrawQuadPayload::rect` and
        // `DrawQuadPayload::shadow` gate on their own `is_noop` internally,
        // so a shadow-only or fill-only background here emits exactly one
        // command.
        let mode = self.tree.records.attrs()[id.idx()].clip_mode();
        let clip = mode.is_clip();
        // Borrowed, not copied: `LayerCtx::tree` is a shared reference, so this
        // borrows the `Tree` rather than `self` and does not collide with the
        // `&mut self` that `brush_source` needs below. Copying instead cost a
        // 64-byte `ChromeRow` per chromed node — most nodes in a real UI.
        let chrome = self.tree.chrome(id);

        if let Some(bg) = chrome {
            // Both draws pass alpha `1.0`: a paint animation is registered
            // against a shape, and chrome is the node's own, not one of them.
            //
            // Shadow paints UNDER the rect fill (CSS box-shadow order).
            // `local_rect = None` means the shadow follows the owner's
            // full arranged rect — `compute_paint_rect` mirrors this so
            // paint extent and damage extent stay in lockstep.
            emit_shadow(out, rect, None, bg.corners, &bg.shadow, 1.0);
            let src = self.brush_source(bg.fill);
            out.draw_quad(DrawQuadPayload::rect(rect, bg.corners, src, bg.stroke), 1.0);
        }

        if clip {
            let layout = self.tree.records.layout()[id.idx()];
            let mask_rect = layout.inner_rect(rect);
            match mode {
                ClipMode::Rect => out.push_clip(PushClipPayload::rect(mask_rect)),
                ClipMode::Rounded => {
                    // Per-corner reduction by the larger of the two
                    // adjacent edge insets so the mask curve stays inside
                    // both adjacent edges; radius can't honor concentricity
                    // with the painted stroke on both axes when padding is
                    // asymmetric.
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
                // Unreachable under the gate above. Spelled out so a new
                // `ClipMode` is a compile error here rather than a variant
                // that silently clips nothing.
                ClipMode::None => {}
            }
        }

        // Clip culling (skipping leaves outside the active ancestor
        // clip) intentionally does NOT live in the encoder: the scissor
        // exists only in physical space on the composer, which culls each
        // call as it arrives. Damage filtering happens at subtree
        // granularity above (early
        // return when no rect intersects this node's screen rect); leaves
        // emit unconditionally once we're past that gate.

        // `None` for an identity transform, which is why nothing below
        // emits the Push/PopTransform pair for one: composing identity is
        // a no-op, so the pair would waste two sink calls and a
        // transform-stack push/pop in the composer.
        let transform = self.tree.anchored_transform(id, rect);

        // Body (direct shapes + child subtrees) paints inside the node's
        // own transform — chrome (drawn above this point) is the only
        // thing that stays in parent space, so a panel's `transform` acts
        // as a pure inner-content pan/zoom while its background remains
        // anchored. Single push/pop wraps the whole body; the composer
        // handles per-call transform composition.
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

/// Shared shadow emit. Chrome branch (`Background::shadow`,
/// `local_rect = None`) and shape-buffer branch (`QuadShape::Shadow`,
/// owner-relative `local_rect`) both route here so the
/// `LoweredShadow::paint_rect_local` translation + fill-axis packing
/// can't drift between the two views.
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
    let paint_local = shadow.paint_rect_local(local_rect, owner_rect.size);
    let paint_rect = Rect {
        min: owner_rect.min + paint_local.min,
        size: paint_local.size,
    };
    let (kind, fill_axis) = if shadow.inset() {
        // The inset axis *is* the stored geometry, so it travels as the
        // packed word — unpacking it to f32 and repacking would be an
        // f16 round trip of identical bytes.
        (FillKind::SHADOW_INSET, FillAxis::from(shadow.geom_f16))
    } else {
        // A drop shadow zeroes the offset lanes: the halo is already
        // folded into `paint_rect`, so the shader must not shift again.
        let ShadowGeom { blur, spread, .. } = shadow.geom();
        (
            FillKind::SHADOW_DROP,
            FillAxis::from_lanes(0.0, 0.0, blur, spread),
        )
    };
    out.draw_quad(
        DrawQuadPayload::shadow(
            paint_rect,
            corners,
            // LoweredShadow.color is `RgbaF16` (the field); the payload
            // takes the packed form directly so the encoder doesn't
            // unpack-and-repack.
            shadow.color,
            kind,
            fill_axis,
        ),
        alpha,
    );
}
