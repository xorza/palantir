#[cfg(debug_assertions)]
mod collision_overlay;

use crate::layout::LayerLayout;
use crate::layout::types::align;
use crate::layout::types::clip_mode::ClipMode;
use crate::primitives::approx::noop_f32;
use crate::primitives::brush::gradient::FillAxis;
use crate::primitives::fill_wire::FillKind;
use crate::primitives::image::{ImageDownsample, ImageFilter, ImageFit};
use crate::primitives::nan::NanCheck;
use crate::primitives::widget_id::WidgetIdMap;
use crate::primitives::{corners::Corners, rect::Rect, size::Size};
use crate::renderer::frontend::FrameScene;
use crate::renderer::frontend::paint_sink::PaintSink;
use crate::renderer::frontend::payload::{
    BrushSource, DrawCurvePayload, DrawIconPayload, DrawImagePayload, DrawMeshPayload,
    DrawPolylinePayload, DrawQuadPayload, PushClipPayload, ResolvedGradient,
};
use crate::renderer::gpu_view::GpuViewEntry;
use crate::renderer::gradient_atlas::handle::SharedGradientAtlas;
use crate::renderer::plan::{RenderKind, RenderPlan, damage_cull_margin};
use crate::renderer::render_buffer::image::{
    IMG_FLAG_MAG_NEAREST, IMG_FLAG_MIN_NEAREST, IMG_FLAG_TAPS_MEAN, IMG_FLAG_TAPS_PEAK,
    IMG_FLAG_TILED,
};
use crate::scene::cascade::CascadeInputHash;
use crate::scene::damage::region::DamageRegion;
use crate::scene::record_store::RecordedGradient;
use crate::scene::shapes::paint::{
    ImageSource, LoweredShadow, QuadShape, ShadowGeom, ShapeBrush, shadow_paint_rect_local,
};
use crate::scene::shapes::record::ShapeRecord;
use crate::scene::tree::Tree;
use crate::scene::tree::iter::TreeItem;
use crate::scene::tree::paint_anims::PaintAnimCursor;
use crate::scene::tree::record::NodeId;
use crate::shape::icon::IconFit;
use crate::shape::rect::RectKind;
use crate::text::shaped_ref::ShapedTextRef;
use std::time::Duration;

/// Retained encoder state.
#[derive(Debug)]
pub(crate) struct Encoder {
    gradients: GradientResolver,
    gradient_atlas: SharedGradientAtlas,
}

/// Entries reset each encode because another window may have evicted their atlas rows.
#[derive(Debug, Default)]
struct GradientResolver {
    resolved: Vec<Option<ResolvedGradient>>,
}

impl GradientResolver {
    fn begin(&mut self, gradient_count: usize) {
        self.resolved.clear();
        self.resolved.resize(gradient_count, None);
    }

    fn source(
        &mut self,
        gradients: &[RecordedGradient],
        atlas: &SharedGradientAtlas,
        brush: ShapeBrush,
    ) -> BrushSource {
        let id = match brush {
            ShapeBrush::Solid(color) => return BrushSource::Solid(color),
            ShapeBrush::Gradient(id) => id,
        };
        let idx = id.0 as usize;
        if let Some(resolved) = self.resolved[idx] {
            return BrushSource::Gradient(resolved);
        }
        let gradient = &gradients[idx];
        let resolved = ResolvedGradient {
            axis: gradient.axis,
            row: atlas.register_stops(&gradient.stops, gradient.interp),
            kind: gradient.kind,
        };
        self.resolved[idx] = Some(resolved);
        BrushSource::Gradient(resolved)
    }
}

/// Resolve a shape's owner-relative `local_rect` against the owner's
/// arranged rect. `None` means "paint the owner's full rect"; `Some(lr)`
/// offsets `lr` by the owner's origin. Shared by the rectangle /
/// `Image` arms so the offset convention can't drift.
#[inline]
fn resolve_local_rect(owner_rect: Rect, local_rect: Option<Rect>) -> Rect {
    match local_rect {
        None => owner_rect,
        Some(lr) => Rect {
            min: owner_rect.min + lr.min,
            size: lr.size,
        },
    }
}

/// Payload bbox for a possibly-spinning stroke shape — **the producing
/// end of the spin pivot contract** (stated in the payload module doc).
///
/// A spun shape sweeps a disc about the owner-box centre `c`, so when
/// `rotation != 0` the lowered centerline bbox is replaced by the
/// smallest square centred on `c` that contains it: half-extent = max
/// distance from `c` to the bbox's corners. That square is what makes
/// `bbox.center() == c` hold downstream, which is the only way the
/// composer can recover the pivot.
fn spin_bbox(owner_rect: Rect, bbox: Rect, rotation: f32) -> Rect {
    if rotation == 0.0 {
        return bbox;
    }
    let c = glam::Vec2::new(owner_rect.size.w, owner_rect.size.h) * 0.5;
    let d = (bbox.min - c).abs().max((bbox.max() - c).abs());
    let r = d.length();
    Rect {
        min: c - glam::Vec2::splat(r),
        size: Size {
            w: 2.0 * r,
            h: 2.0 * r,
        },
    }
}

/// Walk every tree in the scene forest in paint order, emitting logical-px
/// paint commands into `out`. No GPU work, no scale/snap math — that
/// lives in the composer + backend. Per-tree layout rows come off
/// the scene layout, cascade rows off the scene cascade, keyed by layer.
///
/// `plan` is the paint plan for this frame:
/// - `RenderKind::Full` paints everything (first frame, surface change,
///   full-repaint fallback).
/// - `RenderKind::Partial { region }` runs damage-aware subtree
///   culling: a node whose `paint_rect` doesn't intersect any rect in
///   `region` short-circuits the whole subtree's recursion *and* its
///   Push/Pop emission. Caller's responsibility to skip the call
///   entirely when there's no damage to paint.
///
/// The sink arrives ready for a fresh frame — a `ComposeSession` from
/// `Composer::begin`, or an empty capturing sink.
impl Encoder {
    pub(crate) fn new(gradient_atlas: SharedGradientAtlas) -> Self {
        Self {
            gradients: GradientResolver::default(),
            gradient_atlas,
        }
    }

    /// Deliberately carries no profiling span: the sink composes
    /// inline, so this covers the same work as [`Frontend::build`] and a
    /// second span would only read as an encoder regression against a
    /// pre-fusion capture.
    ///
    /// [`Frontend::build`]: crate::renderer::frontend::Frontend::build
    pub(crate) fn encode(
        &mut self,
        scene: &FrameScene<'_>,
        plan: RenderPlan,
        out: &mut dyn PaintSink,
    ) {
        let Self {
            gradients: gradient_resolver,
            gradient_atlas,
        } = self;

        let damage_filter = match &plan.kind {
            RenderKind::Partial { region } => Some(region),
            RenderKind::Full => None,
        };

        let viewport = scene.display.logical_rect();
        let now = scene.time;
        let gradients = scene.payloads.gradients.records.as_slice();
        gradient_resolver.begin(gradients.len());
        // Matches the backend's padded physical scissor; both derive from
        // `renderer::plan::DAMAGE_AA_PADDING`.
        let damage_cull_margin = damage_cull_margin(scene.display.scale_factor);
        for (layer, tree) in scene.forest.trees.iter_paint_order() {
            let layer_cascades = &scene.cascade.layers[layer];
            let mut ctx = LayerCtx {
                tree,
                layout: &scene.layout[layer],
                cascade_inputs: layer_cascades.cascade_inputs.as_slice(),
                subtree_paint_rects: layer_cascades.subtree_paint_rects.as_slice(),
                gradients,
                gradient_atlas,
                gradient_resolver,
                paint_anim_cursor: tree.paint_anims.cursor(),
                gpu_views: scene.gpu_views,
                damage_filter,
                damage_cull_margin,
                viewport,
                now,
            };
            for root in &tree.roots {
                encode_node(&mut ctx, root.first_node, out);
            }
        }

        #[cfg(debug_assertions)]
        collision_overlay::emit(scene.forest, scene.layout, out);
    }
}

/// Per-layer encode context. Bundles fixed inputs so recursion threads one
/// `&mut ctx` instead of a long argument list.
#[derive(Debug)]
struct LayerCtx<'a> {
    tree: &'a Tree,
    layout: &'a LayerLayout,
    cascade_inputs: &'a [CascadeInputHash],
    subtree_paint_rects: &'a [Rect],
    gradients: &'a [RecordedGradient],
    gradient_atlas: &'a SharedGradientAtlas,
    gradient_resolver: &'a mut GradientResolver,
    paint_anim_cursor: PaintAnimCursor<'a>,
    /// Live `GpuView`s by `WidgetId` (one map across layers). A
    /// An `ImageSource::GpuView` carries only its epoch; the arm looks the
    /// view's stable `TextureId` + paint callback up here by the owner node's id.
    gpu_views: &'a WidgetIdMap<GpuViewEntry>,
    damage_filter: Option<&'a DamageRegion>,
    /// Logical-px inflation applied to each node's `subtree_paint_rect`
    /// before the damage-cull intersection test, so the cull covers the
    /// AA-padded region the backend PreClears (see [`Encoder::encode`]).
    damage_cull_margin: f32,
    viewport: Rect,
    now: Duration,
}

impl LayerCtx<'_> {
    #[inline]
    fn brush_source(&mut self, brush: ShapeBrush) -> BrushSource {
        self.gradient_resolver
            .source(self.gradients, self.gradient_atlas, brush)
    }
}

/// Emit one shape at `owner_rect`. Pulled out of `encode_node` so the
/// child-interleave loop can call it without duplicating the per-variant
/// match. `text_ordinal` is the within-node index of the next
/// `ShapeRecord::Text` to consume from `layout.text_spans[id]`; the caller
/// increments it after this function emits a text run.
fn emit_one_shape(
    ctx: &mut LayerCtx<'_>,
    id: NodeId,
    owner_rect: Rect,
    shape_idx: u32,
    shape: &ShapeRecord,
    text_ordinal: u32,
    out: &mut dyn PaintSink,
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
    // Paint-anim gate. Today's only alpha source (`BlinkOpacity`) is
    // binary 0/1, so a "hidden" sample just skips emission;
    // fractional-alpha multiplication arrives with a future `Pulse`
    // variant. `Spin` rides `paint_mod.rotation`, consumed by the
    // stroke arms below.
    let paint_mod = ctx.paint_anim_cursor.sample(shape_idx, ctx.now);
    if noop_f32(paint_mod.alpha) {
        return;
    }
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
                let r = resolve_local_rect(owner_rect, *local_rect);
                let src = ctx.brush_source(*fill);
                match kind {
                    RectKind::Rounded => {
                        out.draw_quad(DrawQuadPayload::rect(r, *corners, src, *stroke))
                    }
                    RectKind::Windowed => {
                        out.draw_quad(DrawQuadPayload::rect_window(r, *corners, src, *stroke))
                    }
                }
            }
            QuadShape::Shadow {
                local_rect,
                corners,
                shadow,
            } => emit_shadow(out, owner_rect, *local_rect, *corners, shadow),
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
                // Stroke noop-normalization happens inside `draw_triangle`
                // (`PaintSink`'s provided half is the single canonical gate).
                out.draw_quad(DrawQuadPayload::triangle(
                    owner_rect.min,
                    [*a, *b, *c],
                    *fill,
                    *radius,
                    *stroke,
                ))
            }
        },
        ShapeRecord::Text {
            local_origin,
            text,
            color,
            align,
            ..
        } => {
            let span = ctx.layout.text_spans[id.idx()];
            debug_assert!(
                text_ordinal < span.len,
                "encoder text-shape ordinal {text_ordinal} out of bounds for span len {}",
                span.len,
            );
            let shaped = ctx.layout.text_shapes[(span.start + text_ordinal) as usize];
            if shaped.key.is_invalid() {
                tracing::trace!(?shape, "encoder: dropping text with invalid key");
                return;
            }
            // Two paths share the same `DrawText` payload:
            // - `local_rect: None` → encoder owns positioning. Place
            //   the shaped bbox inside the owner's padded inner rect
            //   via `align_in_rect`.
            // - `local_rect: Some(origin)` → widget owns positioning.
            //   Origin is `owner.min + origin`; bbox size is the
            //   shaped measurement. `align`'s placement axes are
            //   ignored (only `align.halign()` matters here, and
            //   that's already baked into the shaped buffer's
            //   per-line glyph offsets).
            let rect = match local_origin {
                None => {
                    let padded =
                        owner_rect.deflated_by(ctx.tree.records.layout()[id.idx()].padding);
                    align::align_in_rect(padded, shaped.measured, *align)
                }
                Some(origin) => Rect {
                    min: owner_rect.min + *origin,
                    size: shaped.measured,
                },
            };
            out.draw_text(rect, *color, ShapedTextRef::new(shaped.key, text));
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
            let rotation = paint_mod.rotation;
            let bbox = spin_bbox(owner_rect, *bbox, rotation);
            out.draw_polyline(DrawPolylinePayload {
                bbox,
                origin: owner_rect.min,
                width: *width,
                rotation,
                points_start: points.start,
                points_len: points.len,
                colors_start: colors.start,
                colors_len: colors.len,
                color_mode: *color_mode,
                cap: *cap,
                join: *join,
            });
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
            let origin = resolve_local_rect(owner_rect, *local_rect).min;
            out.draw_mesh(DrawMeshPayload {
                bbox: *bbox,
                origin,
                tint: *tint,
                v_start: vertices.start,
                v_len: vertices.len,
                i_start: indices.start,
                i_len: indices.len,
            });
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
            let fill = ctx.brush_source(*fill).to_gpu_fields();
            let rotation = paint_mod.rotation;
            out.draw_curve(DrawCurvePayload {
                basis: *basis,
                bbox: spin_bbox(owner_rect, *bbox, rotation),
                origin: owner_rect.min,
                rotation,
                color: fill.color,
                width: *width,
                cap: *cap,
                fill_kind: fill.kind,
                fill_lut_row: fill.lut_row,
            });
        }
        ShapeRecord::Icon {
            local_rect,
            handle,
            fit,
            tint,
        } => {
            let base = resolve_local_rect(owner_rect, *local_rect);
            out.draw_icon(DrawIconPayload {
                rect: resolve_icon_fit(base, handle.view_box, *fit),
                icon: handle.icon,
                tint: *tint,
            });
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
            let base = resolve_local_rect(owner_rect, *local_rect);
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
                ImageSource::Texture { id, size } => (*id, *size, None),
                ImageSource::GpuView { epoch: _ } => {
                    let wid = ctx.tree.records.widget_id()[id.idx()];
                    let view = &ctx.gpu_views[&wid];
                    (view.texture_id, glam::UVec2::ZERO, Some(&view.paint))
                }
            };
            let Resolved {
                rect,
                uv_min,
                uv_size,
            } = resolve_fit(base, size, *fit);
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
                DrawImagePayload::image(rect, uv_min, uv_size, *tint, handle, flags),
                paint,
            );
        }
    }
}

fn encode_node(ctx: &mut LayerCtx<'_>, id: NodeId, out: &mut dyn PaintSink) {
    if ctx.cascade_inputs[id.idx()].invisible() {
        return;
    }

    // Off-screen subtree cull. Reads `Cascade::subtree_paint_rects`
    // — the rolled-up paint bound that includes every descendant —
    // so a Canvas-positioned child overflowing its parent's `Fixed`
    // bound (or a shape with negative-margin overhang) doesn't get
    // killed when the parent's own rect lies just outside the
    // viewport. The parallel column is owner-local to this layer.
    let subtree_paint_rect = ctx.subtree_paint_rects[id.idx()];
    if !subtree_paint_rect.intersects(ctx.viewport) {
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
    if let Some(region) = ctx.damage_filter
        && !region.any_intersects(subtree_paint_rect.inflated(ctx.damage_cull_margin))
    {
        return;
    }

    let rect = ctx.layout.rect[id.idx()];

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
    // part is no-op. Both `draw_rect` and `draw_shadow` gate on their own
    // `is_noop` internally, so a shadow-only or fill-only background here
    // emits exactly one command.
    let mode = ctx.tree.records.attrs()[id.idx()].clip_mode();
    let clip = mode.is_clip();
    // Borrowed, not copied: `LayerCtx::tree` is a shared reference, so this
    // borrows the `Tree` rather than `ctx` and does not collide with the
    // `&mut ctx` that `brush_source` needs below. Copying instead cost a
    // 64-byte `ChromeRow` per chromed node — most nodes in a real UI.
    let chrome = ctx.tree.chrome(id);

    if let Some(bg) = chrome {
        // Shadow paints UNDER the rect fill (CSS box-shadow order).
        // `local_rect = None` means the shadow follows the owner's
        // full arranged rect — `compute_paint_rect` mirrors this so
        // paint extent and damage extent stay in lockstep.
        emit_shadow(out, rect, None, bg.corners, &bg.shadow);
        let src = ctx.brush_source(bg.fill);
        out.draw_quad(DrawQuadPayload::rect(rect, bg.corners, src, bg.stroke));
    }

    if clip {
        // Clip deflates by `padding`. `Tree::open_node` folds the
        // chrome's stroke width into padding so the mask automatically
        // sits inside the painted stroke ring — children clipped here
        // can't overpaint the stroke.
        let padding = ctx.tree.records.layout()[id.idx()].padding;
        let mask_rect = rect.deflated_by(padding);
        match mode {
            ClipMode::Rect => out.clip(PushClipPayload::rect(mask_rect)),
            ClipMode::Rounded => {
                // Per-corner reduction by the larger of the two
                // adjacent edge insets so the mask curve stays inside
                // both adjacent edges; radius can't honor concentricity
                // with the painted stroke on both axes when padding is
                // asymmetric.
                let painted = chrome
                    .map(|bg| bg.corners)
                    .expect("ClipMode::Rounded without chrome row — open_node invariant violated");
                let [ptl, ptr_, pbr, pbl] = painted.as_array();
                let [pl, pt, pr, pb] = padding.as_array();
                let mask_radius = Corners::new(
                    (ptl - pt.max(pl)).max(0.0),
                    (ptr_ - pt.max(pr)).max(0.0),
                    (pbr - pb.max(pr)).max(0.0),
                    (pbl - pb.max(pl)).max(0.0),
                );
                out.clip(PushClipPayload::rounded(mask_rect, mask_radius));
            }
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

    // Skip Push/PopTransform when the transform is identity —
    // composing identity is a no-op, so emitting the pair just
    // wastes two sink calls and a `transform_stack` push/pop in the
    // composer.
    //
    // Anchor the raw transform at the node's own `layout_rect.min`
    // so its scale pivots about the panel's origin (see
    // `TranslateScale::anchored_at`). Cascade and `compute_paint_rect`
    // apply the same anchoring; pushing the un-anchored form here
    // would visibly shift the body relative to its damage rect.
    let transform = ctx
        .tree
        .transform_of(id)
        .map(|t| t.anchored_at(rect.min))
        .filter(|t| !t.is_noop());

    // Body (direct shapes + child subtrees) paints inside the node's
    // own transform — chrome (drawn above this point) is the only
    // thing that stays in parent space, so a panel's `transform` acts
    // as a pure inner-content pan/zoom while its background remains
    // anchored. Single push/pop wraps the whole body; the composer
    // handles per-call transform composition.
    if let Some(t) = transform {
        out.push_transform(t);
    }
    let mut text_ordinal: u32 = 0;
    let tree = ctx.tree;
    for item in tree.tree_items(id) {
        match item {
            TreeItem::ShapeRecord(shape_idx, shape) => {
                emit_one_shape(ctx, id, rect, shape_idx, shape, text_ordinal, out);
                if matches!(shape, ShapeRecord::Text { .. }) {
                    text_ordinal += 1;
                }
            }
            TreeItem::Child(child) => {
                encode_node(ctx, child.id, out);
            }
        }
    }
    debug_assert_eq!(
        text_ordinal,
        ctx.layout.text_spans[id.idx()].len,
        "encoder text count differs from the node's shaped-text span",
    );
    if transform.is_some() {
        out.pop_transform();
    }

    if clip {
        out.pop_clip();
    }
}

/// Shared shadow emit. Chrome branch (`Background::shadow`,
/// `local_rect = None`) and shape-buffer branch (`QuadShape::Shadow`,
/// owner-relative `local_rect`) both route here so the
/// `shadow_paint_rect_local` translation + fill-axis packing
/// can't drift between the two views.
fn emit_shadow(
    out: &mut dyn PaintSink,
    owner_rect: Rect,
    local_rect: Option<Rect>,
    corners: Corners,
    shadow: &LoweredShadow,
) {
    if shadow.is_noop() {
        return;
    }
    // Unpack all four f16 geom lanes in one batched SIMD call.
    let ShadowGeom {
        offset,
        blur,
        spread,
    } = shadow.geom();
    let inset = shadow.inset();
    let paint_local =
        shadow_paint_rect_local(local_rect, owner_rect.size, offset, blur, spread, inset);
    let paint_rect = Rect {
        min: owner_rect.min + paint_local.min,
        size: paint_local.size,
    };
    let (kind, fill_axis) = if inset {
        (
            FillKind::SHADOW_INSET,
            FillAxis::from_lanes(offset.x, offset.y, blur, spread),
        )
    } else {
        (
            FillKind::SHADOW_DROP,
            FillAxis::from_lanes(0.0, 0.0, blur, spread),
        )
    };
    out.draw_quad(DrawQuadPayload::shadow(
        paint_rect,
        corners,
        // LoweredShadow.color is `ColorF16` (the field); the payload
        // takes the packed form directly so the encoder doesn't
        // unpack-and-repack.
        shadow.color,
        kind,
        fill_axis,
    ));
}

/// Map an icon's `(base, view_box, fit)` onto the logical-px rect it paints.
///
/// Sibling of [`resolve_fit`] and deliberately smaller: an icon has no UV to
/// crop, so every mode is a rect and nothing else. A degenerate viewBox falls
/// through to the base rect — the same fail-safe the image path takes for a
/// missing intrinsic size.
pub(super) fn resolve_icon_fit(base: Rect, view_box: glam::Vec2, fit: IconFit) -> Rect {
    let (iw, ih) = (view_box.x, view_box.y);
    let (bw, bh) = (base.size.w, base.size.h);
    if iw <= 0.0 || ih <= 0.0 || bw <= 0.0 || bh <= 0.0 {
        return base;
    }
    match fit {
        IconFit::Fill => base,
        IconFit::Contain => {
            // Preserve aspect; the tighter axis ratio decides the scale.
            let scale = (bw / iw).min(bh / ih);
            centered_in(base, iw * scale, ih * scale)
        }
        // Intrinsic logical px, centred. Overflows a smaller rect, exactly as
        // `ImageFit::None` does.
        IconFit::None => centered_in(base, iw, ih),
    }
}

/// A `w`x`h` box centred inside `base`. Every aspect-preserving fit resolves
/// through here — icon and image alike — so the three of them cannot disagree
/// about where the leftover space goes.
fn centered_in(base: Rect, w: f32, h: f32) -> Rect {
    Rect {
        min: base.min + glam::Vec2::new((base.size.w - w) * 0.5, (base.size.h - h) * 0.5),
        size: Size { w, h },
    }
}

/// Output of [`resolve_fit`]: the final paint rect + UV crop the
/// encoder hands to the sink.
#[derive(Debug)]
struct Resolved {
    rect: Rect,
    uv_min: glam::Vec2,
    uv_size: glam::Vec2,
}

const FULL_UV_MIN: glam::Vec2 = glam::Vec2::ZERO;
const FULL_UV_SIZE: glam::Vec2 = glam::Vec2::ONE;

/// Map `(base, image_size, fit)` → `(paint_rect, uv_crop)`. `base` is
/// the encoder-resolved paint rect (owner rect or local override).
/// `image_size = UVec2::ZERO` (missing registry entry at lowering time)
/// falls through to the base rect with full UV — the backend's
/// lookup-miss branch then skips the actual draw.
fn resolve_fit(base: Rect, image_size: glam::UVec2, fit: ImageFit) -> Resolved {
    let iw = image_size.x as f32;
    let ih = image_size.y as f32;
    let bw = base.size.w;
    let bh = base.size.h;
    if iw <= 0.0 || ih <= 0.0 || bw <= 0.0 || bh <= 0.0 {
        return Resolved {
            rect: base,
            uv_min: FULL_UV_MIN,
            uv_size: FULL_UV_SIZE,
        };
    }
    match fit {
        ImageFit::Fill => Resolved {
            rect: base,
            uv_min: FULL_UV_MIN,
            uv_size: FULL_UV_SIZE,
        },
        ImageFit::Contain => {
            // Preserve aspect; the smaller axis ratio decides scale.
            let scale = (bw / iw).min(bh / ih);
            Resolved {
                rect: centered_in(base, iw * scale, ih * scale),
                uv_min: FULL_UV_MIN,
                uv_size: FULL_UV_SIZE,
            }
        }
        ImageFit::Cover => {
            // Preserve aspect; the larger axis ratio decides scale —
            // image overhangs the rect. Crop the overhang via UV
            // (centered, so visible texels match `Contain`'s axis).
            let scale = (bw / iw).max(bh / ih);
            let w_phys = iw * scale; // >= bw
            let h_phys = ih * scale; // >= bh
            let uv_w = bw / w_phys; // <= 1
            let uv_h = bh / h_phys; // <= 1
            Resolved {
                rect: base,
                uv_min: glam::Vec2::new((1.0 - uv_w) * 0.5, (1.0 - uv_h) * 0.5),
                uv_size: glam::Vec2::new(uv_w, uv_h),
            }
        }
        ImageFit::None => {
            // Paint at intrinsic px, centered. Image may exceed `base`
            // — currently uncropped; future slice can add a scissor.
            Resolved {
                rect: centered_in(base, iw, ih),
                uv_min: FULL_UV_MIN,
                uv_size: FULL_UV_SIZE,
            }
        }
        // Raw caller-driven UV; the shader wraps with `fract`. The
        // intrinsic image size is irrelevant — `scale`/`offset` already
        // express the repeat count and phase against the full rect.
        ImageFit::Tile { offset, scale } => Resolved {
            rect: base,
            uv_min: offset,
            uv_size: scale,
        },
    }
}

#[cfg(test)]
pub(crate) mod internals {
    use crate::renderer::frontend::FrameScene;
    use crate::renderer::frontend::capture::PaintCapture;
    use crate::renderer::frontend::encoder::Encoder;
    use crate::renderer::gradient_atlas::handle::SharedGradientAtlas;
    use crate::renderer::plan::RenderPlan;

    pub(crate) fn encode(
        scene: FrameScene<'_>,
        gradient_atlas: &SharedGradientAtlas,
        plan: RenderPlan,
    ) -> PaintCapture {
        let mut encoder = Encoder::new(gradient_atlas.clone());
        let mut recorded = PaintCapture::default();
        encoder.encode(&scene, plan, &mut recorded);
        recorded
    }
}

#[cfg(test)]
mod tests;
