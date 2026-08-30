//! How much of the screen one node's paint can reach, and what the damage of
//! it comes to.

use crate::common::content_hash::ContentHash;
use crate::layout::LayerLayout;
use crate::layout::text_runs::TextRuns;
use crate::primitives::rect::Rect;
use crate::primitives::size::Size;
use crate::primitives::span::Span;
use crate::primitives::translate_scale::TranslateScale;
use crate::scene::cascade::paint::{Paint, PaintArena};
use crate::scene::shapes::paint::QuadShape;
use crate::scene::shapes::record::{self, ShapeRecord};
use crate::scene::tree::Tree;
use crate::scene::tree::iter::TreeItem;
use crate::scene::tree::node_id::NodeId;
use crate::scene::tree::paint_anims::PaintAnims;
use crate::shape::stroke_bounds::{self, HALF_FRINGE};
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
    clip_screen(r, clip)
}

/// A screen rect held inside an optional clip — the shape every rect in
/// the cascade takes on its way out.
///
/// `None` is "no clip on this branch", not "clip to nothing", which is
/// the whole reason this is worth a name: the `map_or` that says so reads
/// backwards at a glance, and it was written out at four sites.
#[inline]
pub(super) fn clip_screen(screen: Rect, clip: Option<Rect>) -> Rect {
    clip.map_or(screen, |c| screen.clamp_to(c))
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
    // `screen` is already clipped, so a fully-off-clip run has collapsed
    // to zero on an axis (a zero-width box pinned at the clip edge). It
    // has no visible glyphs to pad; inflating it here would re-grow the
    // box *back across the clip edge*, fabricating a sub-pixel damage
    // sliver at the viewport edge for text that isn't on screen at all
    // (the "offscreen node casts a shadow at the window edge" bug). Leave
    // a non-paintable box empty — `is_paint_empty` also folds in the NaN
    // and float-boundary near-zero cases a bare `<= 0` compare would miss.
    if screen.is_paint_empty() {
        return screen;
    }
    let pad_w = measured.w * (TEXT_SCALE_STEP * 0.5);
    let pad_h = measured.h * (TEXT_SCALE_STEP * 0.5);
    let inflated = Rect {
        min: Vec2::new(screen.min.x - pad_w, screen.min.y - pad_h),
        size: Size {
            w: screen.size.w + 2.0 * pad_w,
            h: screen.size.h + 2.0 * pad_h,
        },
    };
    clip_screen(inflated, clip)
}

/// The owner-local bound a stroked shape is damaged against: its recorded
/// centerline bbox, or the square that bbox sweeps when the shape carries
/// a rotating paint anim.
///
/// The composer culls and batches the spun shape against exactly that
/// square (`StrokeBounds::new`), and the row this feeds is what
/// `extend_predamaged` repaints every frame the spin wakes. Left as the
/// recorded bbox, a spun stroke whose bbox is not a pivot-centred disc
/// paints outside the region damage cleared for it.
///
/// The angle is not needed and not read: the square is the cover at every
/// angle, so the cascade answers without sampling the animation the
/// encoder samples one pass later.
#[inline]
fn spun_if_animated(bbox: Rect, owner_rect: Rect, anims: &PaintAnims, shape_idx: u32) -> Rect {
    if anims.rotates(shape_idx) {
        bbox.spun_cover(owner_rect.spin_pivot())
    } else {
        bbox
    }
}

/// Push one paint row and fold its screen rect into the running union
/// in a single step. [`compute_paint_rect`]'s invariant requires the
/// union to track exactly the set of pushed non-paint-empty rows;
/// doing both here makes the two legs impossible to desync at a call
/// site. A paint-empty screen (shape fully clipped away) still pushes
/// its row — damage matches rows by identity and needs the slot — and
/// drops out of the union through [`Rect::union`]'s identity, which is
/// what keeps the degenerate box pinned at the clip edge from growing
/// it.
#[inline]
fn push_paint(arena: &mut PaintArena, union: &mut Rect, screen: Rect, hash: ContentHash) {
    *union = union.union(screen);
    arena.rows.push(Paint { screen, hash });
}

/// Inputs to [`compute_paint_rect`], threaded from `run_tree`.
///
/// Everything here is something the walk **already holds for its own
/// reasons**, so passing it is reuse rather than a wide-parameter habit;
/// a field earns its place by that test alone. `shape_transform` (the
/// `parent ∘ self_anchored` descendants also inherit) and `clips` are
/// the pointed cases — computed once at the call site so we don't
/// re-probe the sparse `transform_of` column, recompose the transform,
/// or re-read the SoA `attrs` column, all of which showed up as
/// duplicate work in cascade profiling. `visible_rect` is the same
/// bargain from the other end: the full walk pushes it into `hits` and
/// `entries` regardless, and deriving it here would apply and intersect
/// it a second time per node.
///
/// What is *not* here is the counterpart: `layout_rect`, the node's
/// `padding`, and `has_shapes` are one indexed load each off lines this
/// walk has already touched, and `padding` is read only by the text arm
/// — so they are derived below instead of widening every node's bundle.
/// [`Self::has_children`] is the one that *is* here: the walk decides a
/// leaf's rollup on it, so it already holds it, where it holds nothing
/// about shapes.
#[derive(Debug)]
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

/// Emit every paint row for `node` — chrome at row 0 when present,
/// then direct shapes and child markers in record order — write the
/// covering [`Span`] into `node_spans[node]`, and return the
/// screen-space union of the pixel-producing rows — used locally as
/// the `subtree_paint_rects` seed for the encoder's cull. Damage
/// recomputes the same union from the `paint_arena` rows on demand
/// (its cold paths), so it isn't stored per node.
///
/// Chrome rides `parent_transform` (encoder emits chrome before the
/// body push); shapes ride `shape_transform = parent ∘ self_anchored`
/// (inside the body push, per `Panel::transform`). Child markers are
/// pushed raw (zero screen, child `WidgetId` as hash) — they exist so
/// the damage diff sees the paint-order interleave; the child's pixels
/// are covered by its own rows.
///
/// # Invariant
///
/// The returned `Rect` is the screen-space union of the non-paint-empty
/// rows in `arena.rows[paints_start..arena.rows.len()]`, **plus the
/// clip-only fold below** — so it is bit-identical to what
/// `damage::union_screens` recomputes from the stored rows for every
/// node except a chromeless clip-only container, where it is larger by
/// that container's visible rect.
///
/// The difference is deliberate and the two consumers want opposite
/// halves of it: the encoder culls against this return value and needs
/// the container's extent, while damage reads the rows and must not
/// invent pixels for a node that painted none. [`push_paint`] keeps the
/// union and the pushed rows in lockstep everywhere else; child markers
/// bypass it (zero rect, no pixels), and the clip-only branch is the
/// sole fold-without-push case.
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
    // The walk read this same slot to build `visible_rect`, so it is a
    // hot line rather than a fresh fetch.
    let layout_rect = layout.rect[node.idx()];
    let paints_start = arena.rows.len() as u32;

    // Seeded at `Rect::ZERO`, which is what `Rect::union` documents the
    // identity to be — a paint-empty operand drops out, so a node that
    // paints nothing folds to zero and a fully-clipped shape cannot drag
    // the extent to its degenerate box at the clip edge.
    let mut union = Rect::ZERO;

    let owner_local = Rect {
        min: Vec2::ZERO,
        size: layout_rect.size,
    };

    if let Some(bg) = tree.chrome(node) {
        let screen = if bg.shadow.is_noop() {
            visible_rect
        } else {
            let chrome_local =
                owner_local.union(bg.shadow.paint_rect_local(None, layout_rect.size));
            lift_to_screen(chrome_local, layout_rect.min, parent_transform, parent_clip)
        };
        push_paint(arena, &mut union, screen, bg.hash);
    } else if clips {
        // Chromeless clip-only container: union the owner rect into
        // the cull rollup so the encoder emits the PushClip/PopClip
        // pair even when the subtree paints nothing (empty scroll
        // host, etc.). No Paint row — the node contributes no pixels.
        union = visible_rect;
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
            // Every direct text shape has one layout-derived entry, whether
            // measure produced it for a leaf or post-arrange shaping produced
            // it for a container — handed out by the same cursor the encoder
            // walks the column with.
            let shaped = text_runs.shaped(s, layout);
            let screen = match s {
                ShapeRecord::Text {
                    local_origin,
                    align,
                    ..
                } => {
                    let shaped =
                        shaped.expect("a text record always draws its run from the cursor");
                    // Read here rather than carried in: the text arm is
                    // the only reader, so a node with no text shape never
                    // touches the column.
                    let padding = tree.records.layout()[node.idx()].padding;
                    let local = record::text_paint_bbox_local(
                        *local_origin,
                        *align,
                        padding,
                        layout_rect.size,
                        shaped.measured,
                    );
                    let screen = lift_to_screen(local, layout_rect.min, shape_transform, None);
                    inflate_text_damage(screen, shaped.measured, shape_clip)
                }
                ShapeRecord::Polyline {
                    width,
                    cap,
                    join,
                    points,
                    bbox,
                    ..
                } => {
                    // The AA fringe is physical, so inflate only after the
                    // centerline and stroke width reach screen space.
                    let local = spun_if_animated(*bbox, layout_rect, &tree.paint_anims, idx);
                    let centerline = lift_to_screen(local, layout_rect.min, shape_transform, None);
                    let screen = stroke_bounds::bbox(
                        centerline,
                        *width * shape_transform.scale,
                        HALF_FRINGE / display_scale,
                        *cap,
                        (points.len > 2).then_some(*join),
                    );
                    clip_screen(screen, shape_clip)
                }
                ShapeRecord::Curve {
                    width, cap, bbox, ..
                } => {
                    let local = spun_if_animated(*bbox, layout_rect, &tree.paint_anims, idx);
                    let centerline = lift_to_screen(local, layout_rect.min, shape_transform, None);
                    let screen = stroke_bounds::bbox(
                        centerline,
                        *width * shape_transform.scale,
                        HALF_FRINGE / display_scale,
                        *cap,
                        None,
                    );
                    clip_screen(screen, shape_clip)
                }
                // A triangle's stored bbox carries its corner radius but
                // not the AA fringe, which is physical and cannot be
                // folded into an owner-local rect — the same reason the
                // two stroked kinds above add it out here.
                ShapeRecord::Quad(QuadShape::Triangle { bbox, .. }) => clip_screen(
                    lift_to_screen(*bbox, layout_rect.min, shape_transform, None)
                        .inflated(HALF_FRINGE / display_scale),
                    shape_clip,
                ),
                // The three kinds that resolve their whole paint bound in
                // owner-local space, so one lift finishes each: no stroke
                // width and no physical fringe to add out here, and no
                // shaped extent to fold in. A quad's shadow halo is
                // already inside `QuadShape::bbox_local`.
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
                ShapeRecord::Image { local_rect, .. } | ShapeRecord::Icon { local_rect, .. } => {
                    lift_to_screen(
                        local_rect.unwrap_or(owner_local),
                        layout_rect.min,
                        shape_transform,
                        shape_clip,
                    )
                }
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
