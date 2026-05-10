use super::cmd_buffer::RenderCmdBuffer;
use crate::forest::Forest;
use crate::forest::tree::{NodeId, Tree, TreeItem};
use crate::layout::result::{LayerResult, LayoutResult};
use crate::layout::types::{align::Align, align::HAlign, align::VAlign, clip_mode::ClipMode};
use crate::primitives::{
    corners::Corners, rect::Rect, size::Size, spacing::Spacing, transform::TranslateScale,
};
use crate::shape::Shape;
use crate::ui::cascade::{Cascade, CascadeResult};
use crate::ui::damage::region::DamageRegion;

/// Walk the tree pre-order and emit logical-px paint commands. No GPU
/// work, no scale/snap math — that lives in the backend's process
/// step. Pure function over `(&Tree, &LayerResult, &Cascades)`, so
/// the same call works in unit tests with no device. Reads
/// invisibility cascade from `Cascades` so encoder and hit-index
/// can't drift.
///
/// `damage_filter` enables damage-aware partial paint: when
/// `Some(region)`, leaf paint commands (`DrawRect`/`DrawText`) are
/// skipped for nodes whose arranged rect doesn't intersect any rect
/// in the region. Clip and transform push/pop pairs are *always*
/// emitted so descendant scissor state and group boundaries
/// (composer text↔quad split) stay correct. `None` paints
/// everything — used for the first frame and full-repaint fallback.
#[derive(Default)]
pub(crate) struct Encoder {
    pub(crate) cmds: RenderCmdBuffer,
}

impl Encoder {
    /// Encode every tree in `forest` into the encoder's owned command
    /// buffer in paint order. Per-tree result and cascade rows are
    /// looked up by layer.
    pub(crate) fn encode(
        &mut self,
        forest: &Forest,
        results: &LayoutResult,
        cascades: &CascadeResult,
        damage_filter: Option<&DamageRegion>,
        viewport: Rect,
    ) -> &RenderCmdBuffer {
        self.cmds.clear();
        for (layer, tree) in forest.iter_paint_order() {
            let layout = &results[layer];
            let rows = cascades.rows_for(layer);
            for root in &tree.roots {
                encode_node(
                    tree,
                    layout,
                    rows,
                    damage_filter,
                    viewport,
                    NodeId(root.first_node),
                    &mut self.cmds,
                );
            }
        }
        &self.cmds
    }
}

/// Emit one shape at `owner_rect`. Pulled out of `encode_node` so the
/// child-interleave loop can call it without duplicating the per-variant
/// match. `text_ordinal` is the within-node index of the next
/// `Shape::Text` to consume from `layout.text_spans[id]`; the caller
/// increments it after this function emits a text run.
fn emit_one_shape(
    tree: &Tree,
    layout: &LayerResult,
    id: NodeId,
    owner_rect: Rect,
    shape: &Shape,
    text_ordinal: u16,
    out: &mut RenderCmdBuffer,
) {
    match shape {
        Shape::RoundedRect {
            local_rect,
            radius,
            fill,
            stroke,
        } => {
            let r = match local_rect {
                None => owner_rect,
                Some(lr) => Rect {
                    min: owner_rect.min + lr.min,
                    size: lr.size,
                },
            };
            out.draw_rect(r, *radius, *fill, *stroke);
        }
        Shape::Text {
            local_rect,
            color,
            align,
            ..
        } => {
            let span = layout.text_spans[id.index()];
            assert!(
                u32::from(text_ordinal) < span.len,
                "encoder text-shape ordinal {text_ordinal} out of bounds for span len {}",
                span.len,
            );
            let shaped = layout.text_shapes[(span.start + u32::from(text_ordinal)) as usize];
            if shaped.key.is_invalid() {
                tracing::trace!(?shape, "encoder: dropping text with invalid key");
                return;
            }
            // `local_rect: None` → owner inner rect (padding-deflated).
            // `local_rect: Some` → owner-relative explicit rect, padding
            // skipped. `align` positions the glyph bbox inside whichever.
            let base = match local_rect {
                None => owner_rect.deflated_by(tree.records.layout()[id.index()].padding),
                Some(lr) => Rect {
                    min: owner_rect.min + lr.min,
                    size: lr.size,
                },
            };
            out.draw_text(
                align_text_in(base, shaped.measured, *align),
                *color,
                shaped.key,
            );
        }
        Shape::Line { .. } => {
            tracing::trace!(?shape, "encoder: dropping unsupported shape");
        }
    }
}

fn encode_node(
    tree: &Tree,
    layout: &LayerResult,
    rows: &[Cascade],
    damage_filter: Option<&DamageRegion>,
    viewport: Rect,
    id: NodeId,
    out: &mut RenderCmdBuffer,
) {
    if rows[id.index()].invisible {
        return;
    }

    // Off-screen subtree cull. Skips the whole subtree's recursion
    // when its screen-space bounds don't intersect the viewport.
    if !rows[id.index()].screen_rect.intersects(viewport) {
        return;
    }

    // Damage-aware subtree cull. Same shape as the viewport cull
    // above: if no damage rect intersects this subtree's screen
    // bounds, the whole subtree contributes nothing this frame —
    // skip recursion + Push/Pop emission entirely. **Soundness
    // caveat:** `Cascade.screen_rect` is the node's own paint rect,
    // not the subtree bbox; descendants of Canvas / non-clipped /
    // transformed parents may overflow. The viewport cull already
    // trusts this assumption "by convention"; damage cull inherits
    // the same. See `docs/roadmap/damage.md`.
    if let Some(region) = damage_filter
        && !region.any_intersects(rows[id.index()].screen_rect)
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
    let chrome = tree.chrome_for(id).copied();

    let paints =
        damage_filter.is_none_or(|region| region.any_intersects(rows[id.index()].screen_rect));

    // Chrome paints BEFORE the clip is pushed. The clip rect is
    // deflated by the chrome's stroke width (so children don't paint
    // over the stroke), which means chrome's own stroke pixels would
    // also fall outside the deflated region and be clipped. Painting
    // chrome first leaves it unclipped (the panel's SDF self-clips
    // correctly), preserving the stroke ring.
    //
    // No `is_noop` guard here: `Tree::open_node` already drops chrome
    // to `None` when the paint is invisible, so reaching this branch
    // means there's something to paint.
    if paints && let Some(bg) = chrome {
        out.draw_rect(rect, bg.radius, bg.fill, bg.stroke);
    }

    if clip {
        // Inset the clip by the chrome's stroke width so children
        // clip inside the painted stroke ring. With no chrome (paint
        // dropped because invisible, or clip set without a Surface),
        // inset is 0 — there's no painted ring to stay inside of.
        let inset = chrome.map_or(0.0, |bg| bg.stroke.width);
        let mask_rect = rect.deflated_by(Spacing::all(inset));
        match mode {
            ClipMode::Rect => out.push_clip(mask_rect),
            ClipMode::Rounded => {
                // Reduce each corner radius by `inset` so the mask
                // curve stays concentric with the painted stroke's
                // inner edge — both curves have center at
                // `(rect.min + paint.radius)`. Radius comes from the
                // dedicated `clip_radius` column, decoupled from
                // chrome so an invisible-paint rounded surface still
                // has a mask radius even after chrome was dropped.
                let painted = tree
                    .clip_radius_for(id)
                    .copied()
                    .expect("ClipMode::Rounded without clip_radius — open_node invariant violated");
                let mask_radius = Corners {
                    tl: (painted.tl - inset).max(0.0),
                    tr: (painted.tr - inset).max(0.0),
                    br: (painted.br - inset).max(0.0),
                    bl: (painted.bl - inset).max(0.0),
                };
                out.push_clip_rounded(mask_rect, mask_radius);
            }
            ClipMode::None => {}
        }
    }

    // Damage filter: skip leaf shape emission when this node's
    // *screen* rect (layout rect projected through ancestor
    // transforms via `cascades`) doesn't intersect the dirty region.
    // Damage rects in `damage_filter` are also screen-space, so the
    // comparison is consistent under arbitrary transform stacks.
    // Push/PopClip and Push/PopTransform are still emitted (above
    // and below) so scissor groups and child transforms stay
    // coherent. `None` filter ⇒ paint everything.
    //
    // Clip culling (skipping leaves outside the active ancestor
    // clip) intentionally does NOT live in the encoder: cmd shape
    // would depend on screen position, complicating downstream
    // walks. The composer culls per-cmd at compose time instead.

    // Skip Push/PopTransform when the transform is identity —
    // composing identity is a no-op, so emitting the pair just
    // wastes two cmd slots and a `transform_stack` push/pop in the
    // composer.
    let transform = tree
        .bounds(id)
        .transform
        .filter(|t| *t != TranslateScale::IDENTITY);

    // Interleave direct shapes with child recursion in record order.
    // Shapes paint *outside* the owner's pan transform so they stay
    // anchored to the owner regardless of scroll offset; transform is
    // pushed/popped per child accordingly.
    let mut text_ordinal: u16 = 0;
    for item in tree.tree_items(id) {
        match item {
            TreeItem::Shape(shape) => {
                if paints {
                    emit_one_shape(tree, layout, id, rect, shape, text_ordinal, out);
                }
                if matches!(shape, Shape::Text { .. }) {
                    text_ordinal += 1;
                }
            }
            TreeItem::Child(child) => {
                if let Some(t) = transform {
                    out.push_transform(t);
                }
                encode_node(tree, layout, rows, damage_filter, viewport, child.id, out);
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

#[cfg(test)]
mod tests;
