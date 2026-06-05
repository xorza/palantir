//! Per-node paint-extent computation: emit each node's [`Paint`] rows
//! (chrome + direct shapes, lifted to screen space) into the
//! [`PaintArena`] and return their screen-space union — the seed for the
//! encoder's subtree cull. The one place the cascade walks a node's
//! shapes; called from `recascade_node`.

use super::{Paint, PaintArena};
use crate::forest::rollups::NodeHash;
use crate::forest::shapes::record::{ShapeRecord, shadow_paint_rect_local, text_paint_bbox_local};
use crate::forest::tree::{NodeId, Tree, TreeItem, TreeItems};
use crate::layout::LayerLayout;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::span::Span;
use crate::primitives::transform::TranslateScale;
use crate::text::TEXT_SCALE_STEP;
use glam::Vec2;

/// Lift an owner-local rect into screen space: translate by the owner's
/// arranged origin, apply the relevant transform (`parent_transform`
/// for chrome / clip lift, `shape_transform` for shapes), then clip
/// to the ancestor clip. One source of truth for the three coord-
/// space hops the paint emit does.
#[inline]
fn lift_to_screen(local: Rect, origin: Vec2, t: TranslateScale, clip: Option<Rect>) -> Rect {
    let r = t.apply_rect(Rect {
        min: origin + local.min,
        size: local.size,
    });
    clip.map_or(r, |c| r.intersect(c))
}

/// Pad a text shape's screen rect by half a `TEXT_SCALE_STEP` of its
/// measured extent on each axis side, then re-clamp to `clip`.
///
/// The composer paints glyphs at the ladder-*snapped* scale
/// (`composer::snap_text_scale`), while the cascade lifts the rect at
/// the unsnapped scale. The painted block can be up to
/// `|snapped − cascade| ≤ STEP/2` longer per axis than the lifted
/// rect, which works out to `measured × STEP/2` of absolute screen
/// pixels per side — independent of cascade scale. A local-coord pad
/// would multiply by cascade and underflow at `cascade < 1`
/// (zoomed-out content), leaking glyph fringes past the damage rect.
/// Padding in screen space keeps damage covering the worst-case
/// painted extent at any zoom.
#[inline]
fn inflate_text_damage(screen: Rect, measured: Size, clip: Option<Rect>) -> Rect {
    let pad_w = measured.w * (TEXT_SCALE_STEP * 0.5);
    let pad_h = measured.h * (TEXT_SCALE_STEP * 0.5);
    let inflated = Rect {
        min: Vec2::new(screen.min.x - pad_w, screen.min.y - pad_h),
        size: Size {
            w: screen.size.w + 2.0 * pad_w,
            h: screen.size.h + 2.0 * pad_h,
        },
    };
    match clip {
        Some(c) => inflated.intersect(c),
        None => inflated,
    }
}

/// Push one paint row and fold its screen rect into the running union
/// in a single step. [`compute_paint_rect`]'s invariant requires the
/// union to track exactly the set of pushed rows; doing both here makes
/// the two legs impossible to desync at a call site.
#[inline]
fn push_paint(arena: &mut PaintArena, union: &mut Option<Rect>, screen: Rect, hash: NodeHash) {
    *union = Some(union.map_or(screen, |a| a.union(screen)));
    arena.rows.push(Paint { screen, hash });
}

/// Inputs to [`compute_paint_rect`], threaded from `recascade_node`.
/// `shape_transform` (the `parent ∘ self_anchored` descendants also
/// inherit) and `clips` are computed once at the call site and passed
/// in so we don't re-probe the sparse `transform_of` column, recompose
/// the transform, or re-read the SoA `attrs` column — all showed up as
/// duplicate work in cascade profiling.
pub(crate) struct PaintRectCtx<'a> {
    pub(crate) tree: &'a Tree,
    pub(crate) layout: &'a LayerLayout,
    pub(crate) node: NodeId,
    pub(crate) layout_rect: Rect,
    pub(crate) parent_transform: TranslateScale,
    pub(crate) parent_clip: Option<Rect>,
    pub(crate) shape_clip: Option<Rect>,
    pub(crate) shape_transform: TranslateScale,
    pub(crate) clips: bool,
}

/// Emit every paint row for `node` (chrome at row 0 when present, then
/// direct shapes in record order) via [`push_paint`], write the
/// covering [`Span`] into `node_spans[node]`, and return the
/// screen-space union of every row — used locally as the
/// `subtree_paint_rects` seed for the encoder's cull. Damage recomputes
/// the same union from the `paint_arena` rows on demand (its cold
/// paths), so it isn't stored per node.
///
/// Chrome rides `parent_transform` (encoder emits chrome before the
/// body push); shapes ride `shape_transform = parent ∘ self_anchored`
/// (inside the body push, per `Panel::transform`). The two transforms
/// are the only structural difference between the two row kinds.
///
/// # Invariant
///
/// The returned `Rect` is bit-identical to the screen-space union of
/// `arena.rows[paints_start..arena.rows.len()].iter().map(|p| p.screen)`
/// — the same union `damage::union_screens` recomputes from the stored
/// rows. [`push_paint`] keeps the union and the pushed rows in lockstep;
/// the chromeless clip-only branch is the sole fold-without-push case
/// (it contributes a cull rect but emits no pixels).
pub(crate) fn compute_paint_rect(ctx: PaintRectCtx<'_>, arena: &mut PaintArena) -> Rect {
    let PaintRectCtx {
        tree,
        layout,
        node,
        layout_rect,
        parent_transform,
        parent_clip,
        shape_clip,
        shape_transform,
        clips,
    } = ctx;
    let paints_start = arena.rows.len() as u32;

    // `Option<Rect>` because zero-size sentinels bias `Rect::union`
    // toward the origin and an owner-rect seed would inflate damage
    // for chromeless shape hosts.
    let mut union: Option<Rect> = None;

    let owner_local = Rect {
        min: Vec2::ZERO,
        size: layout_rect.size,
    };

    if let Some(bg) = tree.chrome(node) {
        let chrome_local = if bg.shadow.is_noop() {
            owner_local
        } else {
            let g = bg.shadow.geom();
            owner_local.union(shadow_paint_rect_local(
                None,
                layout_rect.size,
                g.offset,
                g.blur,
                g.spread,
                bg.shadow.inset(),
            ))
        };
        let screen = lift_to_screen(chrome_local, layout_rect.min, parent_transform, parent_clip);
        push_paint(arena, &mut union, screen, bg.hash);
    } else if clips {
        // Chromeless clip-only container: union the owner rect into
        // the cull rollup so the encoder emits the PushClip/PopClip
        // pair even when the subtree paints nothing (empty scroll
        // host, etc.). No Paint row — the node contributes no pixels.
        let screen = lift_to_screen(owner_local, layout_rect.min, parent_transform, parent_clip);
        union = Some(union.map_or(screen, |a| a.union(screen)));
    }

    if tree.records.shape_span()[node.idx()].len > 0 {
        let text_span = layout.text_spans[node.idx()];
        let mut text_ord: u32 = 0;
        let shape_hashes = tree.shapes.hashes.as_slice();
        for item in TreeItems::new(&tree.records, &tree.shapes.records, node) {
            let TreeItem::ShapeRecord(idx, s) = item else {
                continue;
            };
            // Text shapes live only on Leaf nodes (`leaf_text_shapes`
            // asserts the same), so when this node has any text shape
            // `text_span.len` must equal the count of `Text` variants
            // yielded by `TreeItems` here. Drift would silently fall
            // back to the owner rect — assert instead.
            let (local, text_measured) = match s {
                ShapeRecord::Text {
                    local_origin,
                    align,
                    ..
                } => {
                    assert!(
                        text_ord < text_span.len,
                        "cascade saw a text shape without a matching ShapedText entry — \
                         leaf_content_size and the cascade walk are out of sync",
                    );
                    let shaped = layout.text_shapes[(text_span.start + text_ord) as usize];
                    text_ord += 1;
                    let local = text_paint_bbox_local(
                        *local_origin,
                        *align,
                        tree.records.layout()[node.idx()].padding,
                        layout_rect.size,
                        shaped.measured,
                    );
                    (local, Some(shaped.measured))
                }
                _ => (s.paint_bbox_local(layout_rect.size), None),
            };
            let mut screen = lift_to_screen(local, layout_rect.min, shape_transform, shape_clip);
            if let Some(measured) = text_measured {
                screen = inflate_text_damage(screen, measured, shape_clip);
            }
            push_paint(arena, &mut union, screen, shape_hashes[idx as usize]);
        }
    }

    let paints_len = arena.rows.len() as u32 - paints_start;
    arena.node_spans[node.idx()] = Span::new(paints_start, paints_len);
    union.unwrap_or(Rect::ZERO)
}
