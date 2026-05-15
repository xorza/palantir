use super::cmd_buffer::{BrushSource, DrawMeshPayload, DrawPolylinePayload, RenderCmdBuffer};
use crate::common::frame_arena::FrameArena;
use crate::forest::shapes::record::{
    GradientPayload, LoweredShadow, ShadowGeom, ShapeBrush, ShapeRecord, shadow_paint_rect_local,
};
use crate::forest::tree::{NodeId, Tree, TreeItem};
use crate::layout::LayerLayout;
use crate::layout::types::{align::Align, align::HAlign, align::VAlign, clip_mode::ClipMode};
use crate::primitives::approx::noop_f32;
use crate::primitives::brush::FillAxis;
use crate::primitives::color::{Color, ColorF16};
use crate::primitives::stroke::Stroke;
use crate::primitives::{corners::Corners, rect::Rect, size::Size};
use crate::renderer::quad::FillKind;
use crate::shape::{ColorModeBits, LineCapBits, LineJoinBits};
use crate::ui::Ui;
use crate::ui::cascade::Cascade;
use crate::ui::damage::region::DamageRegion;
use crate::ui::frame_report::RenderPlan;
use std::time::Duration;

/// Always-on outline emitted over widgets whose explicit `WidgetId`
/// collided this frame. Magenta — distinct from the opt-in red
/// damage-rect overlay. Painted unclipped at the end of `encode`,
/// after every layer's regular paint.
const COLLISION_OVERLAY_STROKE: Stroke = Stroke::solid(Color::rgb(1.0, 0.0, 1.0), 3.0);

/// Build a `BrushSource` from a lowered `ShapeBrush` + the per-frame
/// gradient arena. `Solid` stays inline (no arena read at all);
/// `Gradient(id)` borrows the corresponding `GradientPayload` and
/// projects it to the matching `BrushSource::{Linear,Radial,Conic}`
/// variant. No `Brush` value is ever materialized — the solid hot
/// path writes 16 B of Color + 1 B tag instead of 88 B of enum.
#[inline]
fn shape_brush_source(gradients: &[GradientPayload], brush: ShapeBrush) -> BrushSource<'_> {
    match brush {
        ShapeBrush::Solid(c) => BrushSource::Solid(c),
        ShapeBrush::Gradient(id) => match &gradients[id as usize] {
            GradientPayload::Linear(g) => BrushSource::Linear(g),
            GradientPayload::Radial(g) => BrushSource::Radial(g),
            GradientPayload::Conic(g) => BrushSource::Conic(g),
        },
    }
}

/// Walk the tree pre-order and emit logical-px paint commands. No GPU
/// work, no scale/snap math — that lives in the backend's process
/// step. Pure function over `(&Tree, &LayerLayout, &CascadesEngine)`, so
/// the same call works in unit tests with no device. Reads
/// invisibility cascade from `CascadesEngine` so encoder and hit-index
/// can't drift.
///
/// `damage_filter` enables damage-aware partial paint: when
/// `Some(region)`, leaf paint commands (`DrawRect`/`DrawText`) are
/// skipped for nodes whose arranged rect doesn't intersect any rect
/// in the region. Clip and transform push/pop pairs are *always*
/// emitted so descendant scissor state and group boundaries
/// (composer text↔quad split) stay correct. `None` paints
/// everything — used for the first frame and full-repaint fallback.
/// Encode every tree in `ui.forest` into `out` in paint order.
/// Per-tree layout and cascade rows are looked up by layer off
/// `ui.layout`. `damage` is the paint plan for this frame — `Full`
/// paints everything, `Partial(region)` filters leaves against the
/// region. The skip path is the caller's responsibility (`None`
/// damage ⇒ never call `encode`). `out` is cleared at entry and
/// keeps its capacity for the next frame.
#[profiling::function]
pub(crate) fn encode(ui: &Ui, arena: &FrameArena, plan: RenderPlan, out: &mut RenderCmdBuffer) {
    out.clear();

    let damage_filter = match &plan {
        RenderPlan::Partial { region, .. } => Some(region),
        RenderPlan::Full { .. } => None,
    };

    let viewport = ui.display.logical_rect();
    let now = ui.time;
    let gradients = arena.gradients.as_slice();
    for (layer, tree) in ui.forest.iter_paint_order() {
        let layout = &ui.layout[layer];
        let rows = ui.layout.cascades.rows_for(layer);
        for root in &tree.roots {
            encode_node(
                tree,
                layout,
                rows,
                gradients,
                damage_filter,
                viewport,
                NodeId(root.first_node),
                now,
                out,
            );
        }
    }

    emit_collision_overlays(ui, out);
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
        for (layer, node) in [record.first, record.second] {
            let rects = &ui.layout[layer].rect;
            if node.index() >= rects.len() {
                continue;
            }
            out.draw_rect(
                rects[node.index()],
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
#[allow(clippy::too_many_arguments)]
fn emit_one_shape(
    tree: &Tree,
    layout: &LayerLayout,
    id: NodeId,
    owner_rect: Rect,
    shape_idx: u32,
    shape: &ShapeRecord,
    gradients: &[GradientPayload],
    text_ordinal: u32,
    now: Duration,
    out: &mut RenderCmdBuffer,
) {
    // Paint-anim gate. Slice 1 ships only `BlinkOpacity`, whose
    // alpha is binary 0/1 — so we just skip emission when the
    // sample says "hidden". Fractional-alpha multiplication
    // arrives with the `Pulse` variant.
    let paint_mod = tree.paint_anims.sample(shape_idx, now);
    if noop_f32(paint_mod.alpha) {
        return;
    }
    match shape {
        ShapeRecord::RoundedRect {
            local_rect,
            radius,
            fill,
            stroke,
            ..
        } => {
            let r = match local_rect {
                None => owner_rect,
                Some(lr) => Rect {
                    min: owner_rect.min + lr.min,
                    size: lr.size,
                },
            };
            let src = shape_brush_source(gradients, *fill);
            out.draw_rect(r, *radius, src, *stroke);
        }
        ShapeRecord::Text {
            local_origin,
            color,
            align,
            ..
        } => {
            let span = layout.text_spans[id.index()];
            assert!(
                text_ordinal < span.len,
                "encoder text-shape ordinal {text_ordinal} out of bounds for span len {}",
                span.len,
            );
            let shaped = layout.text_shapes[(span.start + text_ordinal) as usize];
            if shaped.key.is_invalid() {
                tracing::trace!(?shape, "encoder: dropping text with invalid key");
                return;
            }
            // Two paths share the same `DrawText` payload:
            // - `local_rect: None` → encoder owns positioning. Place
            //   the shaped bbox inside the owner's padded inner rect
            //   via `align_text_in`.
            // - `local_rect: Some(origin)` → widget owns positioning.
            //   Origin is `owner.min + origin`; bbox size is the
            //   shaped measurement. `align`'s placement axes are
            //   ignored (only `align.halign()` matters here, and
            //   that's already baked into the shaped buffer's
            //   per-line glyph offsets).
            let rect = match local_origin {
                None => {
                    let padded = owner_rect.deflated_by(tree.records.layout()[id.index()].padding);
                    align_text_in(padded, shaped.measured, *align)
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
            // composer folds `origin` into the per-point transform
            // (no per-frame point copy any more).
            out.draw_polyline(DrawPolylinePayload {
                bbox: *bbox,
                origin: owner_rect.min,
                width: *width,
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
            radius,
            shadow,
        } => emit_shadow(out, owner_rect, *local_rect, *radius, shadow),
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
            let origin = match local_rect {
                None => owner_rect.min,
                Some(lr) => owner_rect.min + lr.min,
            };
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
    }
}

#[allow(clippy::too_many_arguments)]
fn encode_node(
    tree: &Tree,
    layout: &LayerLayout,
    rows: &[Cascade],
    gradients: &[GradientPayload],
    damage_filter: Option<&DamageRegion>,
    viewport: Rect,
    id: NodeId,
    now: Duration,
    out: &mut RenderCmdBuffer,
) {
    if rows[id.index()].cascade_input.invisible() {
        return;
    }

    // Off-screen subtree cull. Skips the whole subtree's recursion
    // when its paint bounds (layout rect inflated by shape overhang —
    // drop-shadow halos) don't intersect the viewport.
    if !rows[id.index()].paint_rect.intersects(viewport) {
        return;
    }

    // DamageEngine-aware subtree cull. Same shape as the viewport cull
    // above: if no damage rect intersects this subtree's paint bounds,
    // the whole subtree contributes nothing this frame — skip
    // recursion + Push/Pop emission entirely. **Soundness caveat:**
    // `Cascade.paint_rect` is the node's own paint bounds, not the
    // subtree bbox; descendants of Canvas / non-clipped / transformed
    // parents may overflow. The viewport cull already trusts this
    // assumption "by convention"; damage cull inherits the same. See
    // `docs/roadmap/damage.md`.
    if let Some(region) = damage_filter
        && !region.any_intersects(rows[id.index()].paint_rect)
    {
        return;
    }

    let rect = layout.rect[id.index()];

    // Order: clip is in parent-of-panel space (pre-transform); transform
    // applies inside the clip and only to children. The panel's own
    // background paints under the clip but BEFORE the transform — matching
    // WPF's `RenderTransform` convention.
    //
    // Exception: for `ClipMode::Rounded`, chrome paints BEFORE the clip
    // is pushed. The rounded mask is inset by the stroke width so
    // children can't overpaint the panel's stroke; that means chrome
    // pixels at the stroke region sit outside the mask. If chrome
    // painted under the mask too, its stroke would also be discarded.
    // Painting chrome unmasked (it self-clips via the SDF) keeps the
    // stroke visible while children stay clipped to the inset
    // interior.
    let mode = tree.records.attrs()[id.index()].clip_mode();
    let clip = mode.is_clip();
    let chrome = tree.chrome(id).copied();

    // Chrome paints BEFORE the clip is pushed. The clip rect is
    // deflated by the chrome's stroke width (so children don't paint
    // over the stroke), which means chrome's own stroke pixels would
    // also fall outside the deflated region and be clipped. Painting
    // chrome first leaves it unclipped (the panel's SDF self-clips
    // correctly), preserving the stroke ring.
    //
    // `Tree::open_node` drops chrome to `None` only when every
    // paintable part is no-op. Both `draw_rect` and `draw_shadow`
    // gate on their own `is_noop` internally, so a shadow-only or
    // fill-only background here emits exactly one command.
    if let Some(bg) = chrome {
        // Shadow paints UNDER the rect fill (CSS box-shadow order).
        // `local_rect = None` means the shadow follows the owner's
        // full arranged rect — `compute_paint_rect` mirrors this so
        // paint extent and damage extent stay in lockstep.
        emit_shadow(out, rect, None, bg.radius, &bg.shadow);
        let src = shape_brush_source(gradients, bg.fill);
        out.draw_rect(rect, bg.radius, src, bg.stroke);
    }

    if clip {
        // Clip deflates by `padding` only. Stroke is chrome — a visual
        // detail of this node's own background — not a layout offset
        // for its content. Children lay out at `rect - padding`, so
        // clipping to the same inset keeps a `margin(0)` child flush
        // with the clip edge. Glyphs / borders that happen to land on
        // the stroke ring are intentional; the stroke paints over them
        // in record order.
        let padding = tree.records.layout()[id.index()].padding;
        let mask_rect = rect.deflated_by(padding);
        match mode {
            ClipMode::Rect => out.push_clip(mask_rect),
            ClipMode::Rounded => {
                // Per-corner reduction by the larger of the two
                // adjacent edge insets so the mask curve stays inside
                // both adjacent edges; radius can't honor concentricity
                // with the painted stroke on both axes when padding is
                // asymmetric.
                let painted = tree
                    .chrome(id)
                    .map(|bg| bg.radius)
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
    let transform = tree.transform_of(id).filter(|t| !t.is_noop());

    // Interleave direct shapes with child recursion in record order.
    // Shapes paint *outside* the owner's pan transform so they stay
    // anchored to the owner regardless of scroll offset; transform is
    // pushed/popped per child accordingly.
    let mut text_ordinal: u32 = 0;
    for item in tree.tree_items(id) {
        match item {
            TreeItem::ShapeRecord(shape_idx, shape) => {
                emit_one_shape(
                    tree,
                    layout,
                    id,
                    rect,
                    shape_idx,
                    shape,
                    gradients,
                    text_ordinal,
                    now,
                    out,
                );
                if matches!(shape, ShapeRecord::Text { .. }) {
                    text_ordinal += 1;
                }
            }
            TreeItem::Child(child) => {
                if let Some(t) = transform {
                    out.push_transform(t);
                }
                encode_node(
                    tree,
                    layout,
                    rows,
                    gradients,
                    damage_filter,
                    viewport,
                    child.id,
                    now,
                    out,
                );
                if transform.is_some() {
                    out.pop_transform();
                }
            }
        }
    }

    if clip {
        out.pop_clip();
    }
}

/// Position a text run's bounding box inside a leaf's arranged rect per
/// `align`. Returns a rect with `min` shifted by the alignment offset
/// and `size` shrunk to the measured text bbox — composer takes
/// `min` as the glyph origin and `size` as the clip bounds. Glyphs
/// don't stretch, so `Auto`/`Stretch` collapse to start (top-left)
/// — matches `place_axis`'s behavior for non-stretchable content.
fn align_text_in(leaf: Rect, measured: Size, align: Align) -> Rect {
    let dx = match align.halign() {
        HAlign::Auto | HAlign::Left | HAlign::Stretch => 0.0,
        HAlign::Center => (leaf.size.w - measured.w) * 0.5,
        HAlign::Right => leaf.size.w - measured.w,
    };
    let dy = match align.valign() {
        VAlign::Auto | VAlign::Top | VAlign::Stretch => 0.0,
        VAlign::Center => (leaf.size.h - measured.h) * 0.5,
        VAlign::Bottom => leaf.size.h - measured.h,
    };
    Rect::new(
        leaf.min.x + dx.max(0.0),
        leaf.min.y + dy.max(0.0),
        measured.w,
        measured.h,
    )
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
    radius: Corners,
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
        radius,
        // LoweredShadow.color is `ColorF16` (the field); cmd-buffer
        // takes the packed form directly so the encoder doesn't
        // unpack-and-repack.
        shadow.color,
        kind,
        FillAxis::from_lanes(offset.x, offset.y, blur, axis_w),
    );
}

#[cfg(test)]
mod tests;
