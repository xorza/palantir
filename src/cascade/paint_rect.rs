//! How much of the screen one node's paint can reach, and the damage that comes to.

use crate::cascade::paint::{Paint, PaintArena};
use crate::common::content_hash::ContentHash;
use crate::common::span::Span;
use crate::layout::layer_layout::LayerLayout;
use crate::layout::text::text_runs::TextRuns;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::paint::antialias::AA_HALF_WIDTH;
use crate::scene::tree::Tree;
use crate::scene::tree::iter::TreeItem;
use crate::scene::tree::node_id::NodeId;
use crate::scene::tree::paint_anims::PaintAnims;
use crate::shape::paint::quad_shape::QuadShape;
use crate::shape::record::{self, ShapeRecord};
use crate::shape::stroke_bounds;
use crate::text::TEXT_SCALE_STEP;
use glam::Vec2;

/// Lift an owner-local rect into screen space: translate by the owner's arranged origin, apply `parent_transform` (chrome / clip) or `shape_transform` (shapes), then clip to the ancestor clip.
#[inline]
fn lift_to_screen(local: Rect, origin: Vec2, t: TranslateScale, clip: Option<Rect>) -> Rect {
    let r = t.apply_rect(Rect {
        min: origin + local.min,
        size: local.size,
    });
    clip_screen(r, clip)
}

/// A screen rect held inside an optional clip. `None` means "no clip on this branch", not "clip to nothing".
#[inline]
pub(super) fn clip_screen(screen: Rect, clip: Option<Rect>) -> Rect {
    clip.map_or(screen, |c| screen.clamp_to(c))
}

/// Pad a text shape's screen rect by half a `TEXT_SCALE_STEP` of its inked extent per side, then re-clamp to `clip`.
///
/// The composer paints at the ladder-snapped scale but the cascade lifts at the unsnapped one, so the painted block can be `inked × STEP/2` longer per side in screen pixels, independent of cascade scale. Padding in local coords would underflow below scale 1.
#[inline]
fn inflate_text_damage(screen: Rect, inked: Size, clip: Option<Rect>) -> Rect {
    // `screen` is already clipped, so a fully-off-clip run has collapsed to zero on an axis. Padding it would re-grow it across the clip edge and fabricate a damage sliver for offscreen text. Leave a non-paintable box empty; `is_paint_empty` also covers NaN and near-zero.
    if screen.is_paint_empty() {
        return screen;
    }
    let pad_w = inked.w * (TEXT_SCALE_STEP * 0.5);
    let pad_h = inked.h * (TEXT_SCALE_STEP * 0.5);
    let inflated = Rect {
        min: Vec2::new(screen.min.x - pad_w, screen.min.y - pad_h),
        size: Size {
            w: screen.size.w + 2.0 * pad_w,
            h: screen.size.h + 2.0 * pad_h,
        },
    };
    clip_screen(inflated, clip)
}

/// The owner-local bound a stroked shape is damaged against: its recorded bbox, or the square it sweeps under a rotating paint anim (what the composer culls against). No angle is read.
#[inline]
fn spun_if_animated(bbox: Rect, owner_rect: Rect, anims: &PaintAnims, shape_idx: u32) -> Rect {
    if anims.rotates(shape_idx) {
        bbox.spun_cover(owner_rect.spin_pivot())
    } else {
        bbox
    }
}

/// Push one paint row and fold its screen rect into the running union in one step, so [`compute_paint_rect`]'s invariant cannot desync. A paint-empty screen still pushes its row (damage matches by identity) but drops out of the union through [`Rect::union`]'s identity.
#[inline]
fn push_paint(arena: &mut PaintArena, union: &mut Rect, screen: Rect, hash: ContentHash) {
    *union = union.union(screen);
    arena.rows.push(Paint { screen, hash });
}

/// Inputs to [`compute_paint_rect`], threaded from `run_tree`.
///
/// Everything here the walk already holds: `shape_transform` (`parent ∘ self_anchored`) and `clips` are computed once to avoid re-probing columns, and `visible_rect` is pushed into `hits` and `entries` anyway. Cheap indexed loads (`layout_rect`, `padding`, `has_shapes`) are derived below; [`Self::has_children`] is here because the walk decides a leaf's rollup on it.
#[derive(Clone, Copy, Debug)]
pub(super) struct PaintRectCtx<'a> {
    pub(super) tree: &'a Tree,
    pub(super) layout: &'a LayerLayout,
    pub(super) node: NodeId,
    pub(super) visible_rect: Rect,
    pub(super) parent_transform: TranslateScale,
    pub(super) parent_clip: Option<Rect>,
    pub(super) shape_clip: Option<Rect>,
    pub(super) shape_transform: TranslateScale,
    pub(super) display_scale: f32,
    pub(super) clips: bool,
    pub(super) has_children: bool,
}

/// Emit every paint row for `node`: chrome at row 0 when present, then direct shapes and child markers in record order. Write the covering [`Span`] into `node_spans[node]` and return the screen union of the pixel-producing rows, the `subtree_paint_rects` seed for the encoder's cull.
///
/// Chrome rides `parent_transform`; shapes ride `shape_transform`. Child markers are pushed raw (zero screen, child `WidgetId` as hash) so damage sees paint-order interleave.
///
/// # Invariant
///
/// The returned `Rect` is the union of the non-paint-empty rows in `arena.rows[paints_start..]` plus the clip-only fold, so it equals `PaintRows::union_screens` except for a chromeless clip-only container, where it adds that container's visible rect: the encoder culls on it, while damage must not invent pixels. [`push_paint`] keeps union and rows in lockstep; child markers bypass it, and the clip-only branch is the one fold-without-push.
pub(super) fn compute_paint_rect(ctx: PaintRectCtx<'_>, arena: &mut PaintArena) -> Rect {
    let PaintRectCtx {
        tree,
        layout,
        node,
        visible_rect,
        parent_transform,
        parent_clip,
        shape_clip,
        shape_transform,
        display_scale,
        clips,
        has_children,
    } = ctx;
    let layout_rect = layout.rect[node.idx()];
    let paints_start = arena.rows.len() as u32;

    // Seeded at `Rect::ZERO`, `Rect::union`'s identity: a node painting nothing folds to zero and a fully-clipped shape cannot drag the extent to the clip edge.
    let mut union = Rect::ZERO;

    let owner_local = Rect {
        min: Vec2::ZERO,
        size: layout_rect.size,
    };

    match tree.chrome(node) {
        Some(bg) if bg.is_invisible() => {
            arena.rows.push(Paint {
                screen: Rect::ZERO,
                hash: bg.hash,
            });
            union = visible_rect;
        }
        Some(bg) => {
            let screen = if bg.shadow.is_noop() {
                visible_rect
            } else {
                let chrome_local =
                    owner_local.union(bg.shadow.paint_rect_local(None, layout_rect.size));
                lift_to_screen(chrome_local, layout_rect.min, parent_transform, parent_clip)
            };
            push_paint(arena, &mut union, screen, bg.hash);
        }
        None if clips => {
            // Chromeless clip-only container: union the owner rect into the cull rollup so the encoder emits the PushClip/PopClip pair even when the subtree paints nothing. No Paint row.
            union = visible_rect;
        }
        None => {}
    }

    let has_shapes = tree.records.shape_span()[node.idx()].len > 0;
    if has_shapes || has_children {
        let mut text_runs = TextRuns::new(layout.text_spans[node.idx()]);
        let shape_hashes = tree.shapes.hashes.as_slice();
        let widget_ids = tree.records.widget_id();
        for item in tree.tree_items(node) {
            let (idx, s) = match item {
                TreeItem::ShapeRecord(idx, s) => (idx, s),
                TreeItem::Child(c) => {
                    arena.rows.push(Paint {
                        screen: Rect::ZERO,
                        hash: ContentHash(widget_ids[c.id.idx()].0),
                    });
                    continue;
                }
            };
            // Every direct text shape has one layout-derived entry, from measure (leaf) or post-arrange shaping (container), handed out by the cursor the encoder walks the column with.
            let shaped = text_runs.shaped(s, layout);
            let screen = match s {
                ShapeRecord::Text {
                    local_origin,
                    align,
                    ..
                } => {
                    let shaped =
                        shaped.expect("a text record always draws its run from the cursor");
                    let padding = tree.records.layout()[node.idx()].padding;
                    let inked = record::text_paint_bbox_local(
                        *local_origin,
                        *align,
                        padding,
                        layout_rect.size,
                        shaped.extent.size,
                    )
                    .inflated_by(shaped.extent.ink);
                    let screen = lift_to_screen(inked, layout_rect.min, shape_transform, None);
                    inflate_text_damage(screen, inked.size, shape_clip)
                }
                ShapeRecord::Polyline {
                    width,
                    cap,
                    join,
                    points,
                    bbox,
                    ..
                } => {
                    // The AA fringe is physical: inflate only after the centerline and stroke width reach screen space.
                    let local = spun_if_animated(*bbox, layout_rect, &tree.paint_anims, idx);
                    let centerline = lift_to_screen(local, layout_rect.min, shape_transform, None);
                    let screen = stroke_bounds::bbox(
                        centerline,
                        *width * shape_transform.scale,
                        AA_HALF_WIDTH / display_scale,
                        *cap,
                        (points.len > 2).then_some(*join),
                    );
                    clip_screen(screen, shape_clip)
                }
                ShapeRecord::Curve {
                    stroke, cap, bbox, ..
                } => {
                    let local = spun_if_animated(*bbox, layout_rect, &tree.paint_anims, idx);
                    let centerline = lift_to_screen(local, layout_rect.min, shape_transform, None);
                    let screen = stroke_bounds::bbox(
                        centerline,
                        stroke.width * shape_transform.scale,
                        AA_HALF_WIDTH / display_scale,
                        *cap,
                        None,
                    );
                    clip_screen(screen, shape_clip)
                }
                // A triangle's bbox carries its corner radius but not the physical AA fringe, added here like the stroked kinds above.
                ShapeRecord::Quad(QuadShape::Triangle { bbox, .. }) => clip_screen(
                    lift_to_screen(*bbox, layout_rect.min, shape_transform, None)
                        .inflated(AA_HALF_WIDTH / display_scale),
                    shape_clip,
                ),
                // These kinds resolve their whole bound in owner-local space, so one lift finishes each. A quad's shadow halo is inside `QuadShape::bbox_local`.
                ShapeRecord::Quad(shape) => lift_to_screen(
                    shape.bbox_local(layout_rect.size),
                    layout_rect.min,
                    shape_transform,
                    shape_clip,
                ),
                ShapeRecord::Mesh {
                    bbox, local_rect, ..
                } => lift_to_screen(
                    record::mesh_paint_bbox_local(*bbox, *local_rect),
                    layout_rect.min,
                    shape_transform,
                    shape_clip,
                ),
                // Bounded by the rect the encoder draws, which overflows the base under `ImageFit::None`.
                ShapeRecord::Image {
                    local_rect,
                    source,
                    fit,
                    ..
                } => lift_to_screen(
                    fit.resolve(local_rect.unwrap_or(owner_local), source.intrinsic())
                        .rect,
                    layout_rect.min,
                    shape_transform,
                    shape_clip,
                ),
                ShapeRecord::Icon {
                    local_rect,
                    handle,
                    fit,
                    ..
                } => lift_to_screen(
                    fit.resolve(local_rect.unwrap_or(owner_local), handle.view_box()),
                    layout_rect.min,
                    shape_transform,
                    shape_clip,
                ),
            };
            push_paint(arena, &mut union, screen, shape_hashes[idx as usize]);
        }
        debug_assert!(
            text_runs.is_drained(),
            "cascade text count differs from the node's shaped-text span",
        );
    }

    let paints_len = arena.rows.len() as u32 - paints_start;
    arena.node_spans[node.idx()] = Span::new(paints_start, paints_len);
    union
}
