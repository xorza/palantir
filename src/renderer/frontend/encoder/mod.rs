use crate::forest::frame_arena::FrameArenaInner;
use crate::forest::seen_ids::WidgetIdMap;
use crate::forest::shapes::record::{
    LoweredGradient, LoweredShadow, ShadowGeom, ShapeBrush, ShapeRecord, shadow_paint_rect_local,
    text_in_rect,
};
use crate::forest::tree::iter::TreeItem;
use crate::forest::tree::{NodeId, Tree};
use crate::layout::LayerLayout;
use crate::layout::types::clip_mode::ClipMode;
use crate::primitives::approx::noop_f32;
use crate::primitives::brush::FillAxis;
use crate::primitives::color::{Color, ColorF16};
use crate::primitives::image::{ImageFilter, ImageFit};
use crate::primitives::paint::FillKind;
use crate::primitives::stroke::Stroke;
use crate::primitives::{corners::Corners, rect::Rect, size::Size};
use crate::renderer::backend::viewport::damage_cull_margin;
use crate::renderer::frontend::cmd_buffer::{
    BrushSource, DrawArcPayload, DrawCurvePayload, DrawImagePayload, DrawMeshPayload,
    DrawPolylinePayload, RenderCmdBuffer,
};
use crate::renderer::gpu_view::GpuViewEntry;
use crate::renderer::render_buffer::{IMG_FLAG_NEAREST, IMG_FLAG_TILED};
use crate::shape::{ColorModeBits, LineCapBits, LineJoinBits};
use crate::ui::Ui;
use crate::ui::cascade::CascadeInputHash;
use crate::ui::damage::region::DamageRegion;
use crate::ui::frame_report::{RenderKind, RenderPlan};
use std::time::Duration;

/// Always-on outline emitted over widgets whose explicit `WidgetId`
/// collided this frame. Magenta — distinct from the opt-in red
/// damage-rect overlay. Painted unclipped at the end of `encode`,
/// after every layer's regular paint.
const COLLISION_OVERLAY_STROKE: Stroke = Stroke::solid(Color::rgb(1.0, 0.0, 1.0), 3.0);

/// Resolve a shape's owner-relative `local_rect` against the owner's
/// arranged rect. `None` means "paint the owner's full rect"; `Some(lr)`
/// offsets `lr` by the owner's origin. Shared by the `RoundedRect` /
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

/// Build a `BrushSource` from a lowered `ShapeBrush` + the per-frame
/// gradient arena. `Solid` stays inline; `Gradient(id)` reads the
/// pre-baked `LoweredGradient` (row + axis + kind already finalised
/// at shape-lowering time) — no per-encode dispatch.
#[inline]
fn shape_brush_source(gradients: &[LoweredGradient], brush: ShapeBrush) -> BrushSource {
    match brush {
        ShapeBrush::Solid(c) => BrushSource::Solid(c),
        ShapeBrush::Gradient(id) => BrushSource::Gradient(gradients[id as usize]),
    }
}

/// Payload bbox for a possibly-spinning stroke shape. A spun shape
/// sweeps a disc about the owner-box centre `c`, so when
/// `rotation != 0` the lowered bbox (which already carries the stroke
/// inflation) is replaced by the smallest square centred on `c` that
/// contains it: half-extent = max distance from `c` to the bbox's
/// corners. That bound is rotation-invariant about `c`, so the
/// composer's cull and overlap tracking stay correct at every angle —
/// and it keeps `bbox.center() == c`, the pivot contract the
/// composer's Spin arms rotate about (points for `DrawPolyline`,
/// control points for `DrawCurve`, center + angles for `DrawArc`).
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

/// Walk every tree in `ui.forest` in paint order, emitting logical-px
/// paint commands into `out`. No GPU work, no scale/snap math — that
/// lives in the composer + backend. Per-tree layout rows come off
/// `ui.layout`, cascade rows off `ui.cascades`, keyed by layer.
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
/// `out` is cleared at entry; capacity is retained across frames.
#[profiling::function]
pub(crate) fn encode(
    ui: &Ui,
    arena: &FrameArenaInner,
    plan: RenderPlan,
    out: &mut RenderCmdBuffer,
) {
    out.clear();

    let damage_filter = match &plan.kind {
        RenderKind::Partial { region } => Some(region),
        RenderKind::Full => None,
    };

    let viewport = ui.display.logical_rect();
    let now = ui.time;
    let gradients = arena.gradients.as_slice();
    // Matches the *padded* region the backend actually PreClears — the
    // pad + rounding-slack derivation lives next to the scissor math in
    // `viewport::damage_cull_margin` so the two can't drift.
    let damage_cull_margin = damage_cull_margin(ui.display.scale_factor);
    for (layer, tree) in ui.forest.iter_paint_order() {
        let layer_cascades = &ui.cascades.layers[layer];
        let ctx = LayerCtx {
            tree,
            layout: &ui.layout[layer],
            cascade_inputs: layer_cascades.cascade_inputs.as_slice(),
            subtree_paint_rects: layer_cascades.subtree_paint_rects.as_slice(),
            gradients,
            gpu_views: &ui.gpu_views,
            damage_filter,
            damage_cull_margin,
            viewport,
            now,
        };
        for root in &tree.roots {
            encode_node(&ctx, root.first_node, out);
        }
    }

    emit_collision_overlays(ui, out);
}

/// Immutable per-layer encode context. Bundles the eight refs/scalars
/// that stay fixed across a layer's whole recursion so `encode_node`
/// and `emit_one_shape` thread one `&ctx` instead of a long argument
/// list. Built once per layer in [`encode`].
struct LayerCtx<'a> {
    tree: &'a Tree,
    layout: &'a LayerLayout,
    cascade_inputs: &'a [CascadeInputHash],
    subtree_paint_rects: &'a [Rect],
    gradients: &'a [LoweredGradient],
    /// Live `GpuView`s by `WidgetId` (one map across layers). A
    /// `ShapeRecord::GpuView` carries only its epoch; the arm looks the view's
    /// stable `TextureId` + paint callback up here by the owner node's id.
    gpu_views: &'a WidgetIdMap<GpuViewEntry>,
    damage_filter: Option<&'a DamageRegion>,
    /// Logical-px inflation applied to each node's `subtree_paint_rect`
    /// before the damage-cull intersection test, so the cull covers the
    /// AA-padded region the backend PreClears (see [`encode`]).
    damage_cull_margin: f32,
    viewport: Rect,
    now: Duration,
}

// Manual: `Tree` / `LayerLayout` don't implement `Debug`.
impl std::fmt::Debug for LayerCtx<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LayerCtx")
            .field("damage_cull_margin", &self.damage_cull_margin)
            .field("viewport", &self.viewport)
            .field("now", &self.now)
            .finish_non_exhaustive()
    }
}

/// Final pass: emit a magenta outline for each explicit-id collision
/// recorded this frame. Painted after the regular per-layer walk so
/// it sits on top of everything; emitted with no scissor push so it
/// ignores any clip context the colliding widgets sit under (scroll
/// viewports, clipped popups). Both `NodeId`s are precomputed at
/// recording time (`SeenIds.curr` hashmap lookup) — no tree scan.
fn emit_collision_overlays(ui: &Ui, out: &mut RenderCmdBuffer) {
    if ui.forest.collisions.is_empty() {
        return;
    }
    for record in &ui.forest.collisions {
        for ep in [record.first, record.second] {
            let rects = &ui.layout[ep.layer].rect;
            // Both endpoints come from `Forest::open_node`'s
            // `peek_next_id` and are always opened, so arrange produced
            // a rect for each — the index can't actually exceed `rects`.
            // Assert the invariant in dev; keep the skip as a release
            // safety net so a logic slip degrades to a missing overlay
            // (cosmetic) rather than a panic in the paint path.
            debug_assert!(
                ep.node.idx() < rects.len(),
                "collision endpoint {:?} out of bounds for layer rects len {}",
                ep.node,
                rects.len(),
            );
            if ep.node.idx() >= rects.len() {
                continue;
            }
            out.draw_rect(
                rects[ep.node.idx()],
                Corners::ZERO,
                BrushSource::Solid(ColorF16::TRANSPARENT),
                COLLISION_OVERLAY_STROKE.into(),
            );
        }
    }
}

/// Emit one shape at `owner_rect`. Pulled out of `encode_node` so the
/// child-interleave loop can call it without duplicating the per-variant
/// match. `text_ordinal` is the within-node index of the next
/// `ShapeRecord::Text` to consume from `layout.text_spans[id]`; the caller
/// increments it after this function emits a text run.
fn emit_one_shape(
    ctx: &LayerCtx,
    id: NodeId,
    owner_rect: Rect,
    shape_idx: u32,
    shape: &ShapeRecord,
    text_ordinal: u32,
    out: &mut RenderCmdBuffer,
) {
    // Paint-anim gate. Today's only alpha source (`BlinkOpacity`) is
    // binary 0/1, so a "hidden" sample just skips emission;
    // fractional-alpha multiplication arrives with a future `Pulse`
    // variant. `Spin` rides `paint_mod.rotation`, consumed by the
    // stroke arms below.
    let paint_mod = ctx.tree.paint_anims.sample(shape_idx, ctx.now);
    if noop_f32(paint_mod.alpha) {
        return;
    }
    match shape {
        ShapeRecord::RoundedRect {
            local_rect,
            corners,
            fill,
            stroke,
            ..
        } => {
            let r = resolve_local_rect(owner_rect, *local_rect);
            let src = shape_brush_source(ctx.gradients, *fill);
            out.draw_rect(r, *corners, src, *stroke);
        }
        ShapeRecord::WindowedRect {
            local_rect,
            corners,
            fill,
            stroke,
            ..
        } => {
            let r = resolve_local_rect(owner_rect, *local_rect);
            let src = shape_brush_source(ctx.gradients, *fill);
            out.draw_rect_window(r, *corners, src, *stroke);
        }
        ShapeRecord::Text {
            local_origin,
            color,
            align,
            ..
        } => {
            let span = ctx.layout.text_spans[id.idx()];
            assert!(
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
            //   via `text_in_rect`.
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
                    text_in_rect(padded, shaped.measured, *align)
                }
                Some(origin) => Rect {
                    min: owner_rect.min + *origin,
                    size: shaped.measured,
                },
            };
            out.draw_text(rect, *color, shaped.key);
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
            // Points + colors live in the host's FrameArena; spans
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
                color_mode: ColorModeBits::new(*color_mode),
                cap: LineCapBits::new(*cap),
                join: LineJoinBits::new(*join),
                ..bytemuck::Zeroable::zeroed()
            });
        }
        ShapeRecord::Shadow {
            local_rect,
            corners,
            shadow,
        } => emit_shadow(out, owner_rect, *local_rect, *corners, shadow),
        ShapeRecord::Mesh {
            local_rect,
            tint,
            vertices,
            indices,
            bbox,
            content_hash: _,
        } => {
            // Verts live in the host's FrameArena owner-local;
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
                ..bytemuck::Zeroable::zeroed()
            });
        }
        ShapeRecord::Curve {
            p0,
            p1,
            p2,
            p3,
            width,
            fill,
            fill_grad_hash: _,
            cap,
            bbox,
        } => {
            // Curves are owner-local; composer adds `origin` + active
            // transform before scaling to physical px. Curves carry no
            // gradient axis, so `fill.axis` goes unread.
            let fill = shape_brush_source(ctx.gradients, *fill).to_gpu_fields();
            let rotation = paint_mod.rotation;
            out.draw_curve(DrawCurvePayload {
                bbox: spin_bbox(owner_rect, *bbox, rotation),
                origin: owner_rect.min,
                rotation,
                p0: *p0,
                p1: *p1,
                p2: *p2,
                p3: *p3,
                color: fill.color,
                width: *width,
                cap: *cap as u32,
                fill_kind: fill.kind,
                fill_lut_row: fill.lut_row,
                ..bytemuck::Zeroable::zeroed()
            });
        }
        ShapeRecord::Arc {
            center,
            radius,
            a0,
            a1,
            width,
            fill,
            fill_grad_hash: _,
            cap,
            bbox,
        } => {
            // Same owner-local convention as `Curve`; the composer
            // resolves center/radius to physical px.
            let fill = shape_brush_source(ctx.gradients, *fill).to_gpu_fields();
            let rotation = paint_mod.rotation;
            out.draw_arc(DrawArcPayload {
                bbox: spin_bbox(owner_rect, *bbox, rotation),
                origin: owner_rect.min,
                center: *center,
                radius: *radius,
                a0: *a0,
                a1: *a1,
                rotation,
                color: fill.color,
                width: *width,
                cap: *cap as u32,
                fill_kind: fill.kind,
                fill_lut_row: fill.lut_row,
                ..bytemuck::Zeroable::zeroed()
            });
        }
        ShapeRecord::Triangle {
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
            // (the cmd buffer is the single canonical correctness gate).
            out.draw_triangle(owner_rect.min, [*a, *b, *c], *fill, *radius, *stroke);
        }
        ShapeRecord::Image {
            local_rect,
            tint,
            id,
            size,
            fit,
            filter,
        } => {
            let base = resolve_local_rect(owner_rect, *local_rect);
            // Dims baked into the record — no registry borrow.
            // `size == ZERO` makes `resolve_fit` fall through to the base
            // rect + full UV.
            let Resolved {
                rect,
                uv_min,
                uv_size,
            } = resolve_fit(base, size.as_uvec2(), *fit);
            let mut flags = 0;
            if matches!(*fit, ImageFit::Tile { .. }) {
                flags |= IMG_FLAG_TILED;
            }
            if *filter == ImageFilter::Nearest {
                flags |= IMG_FLAG_NEAREST;
            }
            out.draw_image(DrawImagePayload::image(
                rect, uv_min, uv_size, *tint, *id, flags,
            ));
        }
        ShapeRecord::GpuView { epoch: _ } => {
            // A `GpuView` composites exactly like any image — full arranged
            // rect, untinted, full UV, sampling the view's stable `id` from
            // the shared texture cache (all encapsulated in `gpu_view`). The
            // view's `id` + app `paint` callback live in `Ui::gpu_views`, keyed
            // by the owner node's `WidgetId`; the callback then rides the cmd
            // buffer's own side channel, linked to this draw by index so the
            // composer can list the off-screen target in `frame_targets`.
            // `epoch` only affects the shape hash (damage), not the draw.
            let wid = ctx.tree.records.widget_id()[id.idx()];
            let view = &ctx.gpu_views[&wid];
            let paint_index = out.gpu_view_paints.len() as u32;
            out.gpu_view_paints.push(view.paint.clone());
            out.draw_image(DrawImagePayload::gpu_view(
                owner_rect,
                view.texture_id,
                paint_index,
            ));
        }
    }
}

fn encode_node(ctx: &LayerCtx, id: NodeId, out: &mut RenderCmdBuffer) {
    if ctx.cascade_inputs[id.idx()].invisible() {
        return;
    }

    // Off-screen subtree cull. Reads `Cascades::subtree_paint_rects`
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
    let chrome = ctx.tree.chrome(id);

    if let Some(bg) = chrome {
        // Shadow paints UNDER the rect fill (CSS box-shadow order).
        // `local_rect = None` means the shadow follows the owner's
        // full arranged rect — `compute_paint_rect` mirrors this so
        // paint extent and damage extent stay in lockstep.
        emit_shadow(out, rect, None, bg.corners, &bg.shadow);
        let src = shape_brush_source(ctx.gradients, bg.fill);
        out.draw_rect(rect, bg.corners, src, bg.stroke);
    }

    if clip {
        // Clip deflates by `padding`. `Tree::open_node` folds the
        // chrome's stroke width into padding so the mask automatically
        // sits inside the painted stroke ring — children clipped here
        // can't overpaint the stroke.
        let padding = ctx.tree.records.layout()[id.idx()].padding;
        let mask_rect = rect.deflated_by(padding);
        match mode {
            ClipMode::Rect => out.push_clip(mask_rect),
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
                out.push_clip_rounded(mask_rect, mask_radius);
            }
            ClipMode::None => {}
        }
    }

    // Clip culling (skipping leaves outside the active ancestor
    // clip) intentionally does NOT live in the encoder: cmd shape
    // would depend on screen position, complicating downstream
    // walks. The composer culls per-cmd at compose time instead.
    // Damage filtering happens at subtree granularity above (early
    // return when no rect intersects this node's screen rect); leaves
    // emit unconditionally once we're past that gate.

    // Skip Push/PopTransform when the transform is identity —
    // composing identity is a no-op, so emitting the pair just
    // wastes two cmd slots and a `transform_stack` push/pop in the
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
    // handles per-cmd transform composition.
    if let Some(t) = transform {
        out.push_transform(t);
    }
    let mut text_ordinal: u32 = 0;
    for item in ctx.tree.tree_items(id) {
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
    if transform.is_some() {
        out.pop_transform();
    }

    if clip {
        out.pop_clip();
    }
}

/// Shared shadow emit. Chrome branch (`Background::shadow`,
/// `local_rect = None`) and shape-buffer branch (`ShapeRecord::Shadow`,
/// owner-relative `local_rect`) both route here so the
/// `shadow_paint_rect_local` translation + `(kind, axis_w)` packing
/// can't drift between the two views.
fn emit_shadow(
    out: &mut RenderCmdBuffer,
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
    let (kind, axis_w) = if inset {
        (FillKind::SHADOW_INSET, spread.max(0.0))
    } else {
        (FillKind::SHADOW_DROP, 0.0)
    };
    out.draw_shadow(
        paint_rect,
        corners,
        // LoweredShadow.color is `ColorF16` (the field); cmd-buffer
        // takes the packed form directly so the encoder doesn't
        // unpack-and-repack.
        shadow.color,
        kind,
        FillAxis::from_lanes(offset.x, offset.y, blur, axis_w),
    );
}

/// Output of [`resolve_fit`]: the final paint rect + UV crop the
/// encoder hands to the cmd buffer.
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
            let w = iw * scale;
            let h = ih * scale;
            let dx = (bw - w) * 0.5;
            let dy = (bh - h) * 0.5;
            Resolved {
                rect: Rect {
                    min: base.min + glam::Vec2::new(dx, dy),
                    size: Size { w, h },
                },
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
            let dx = (bw - iw) * 0.5;
            let dy = (bh - ih) * 0.5;
            Resolved {
                rect: Rect {
                    min: base.min + glam::Vec2::new(dx, dy),
                    size: Size { w: iw, h: ih },
                },
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
mod tests;
