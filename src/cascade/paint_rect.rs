//! How much of the screen one node's paint can reach, and the damage that comes to.

use crate::cascade::paint::Paint;
use crate::common::content_hash::ContentHash;
use crate::common::span::Span;
use crate::layout::plane_layout::PlaneLayout;
use crate::layout::text::text_runs::TextRuns;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::size::Size;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::paint::antialias::AA_HALF_WIDTH;
use crate::scene::tree::Tree;
use crate::scene::tree::iter::TreeItem;
use crate::scene::tree::node_id::NodeId;
use crate::shape::paint::quad_shape::QuadShape;
use crate::shape::record::{self, ShapeRecord, TextAnchor};
use crate::shape::stroke_bounds::{StrokeJoints, StrokeReach};
use crate::text;
use glam::Vec2;

/// How an owner-local rect reaches screen space: translate by the owner's arranged `origin`, then apply `transform`, the parent's for chrome and the shape transform for shapes.
#[derive(Clone, Copy, Debug)]
struct Lift {
    origin: Vec2,
    transform: TranslateScale,
}

impl Lift {
    /// `local` lifted into screen space, then clipped to `clip`.
    #[inline]
    fn to_screen(self, local: Rect, clip: Option<Rect>) -> Rect {
        let r = self.transform.apply_rect(Rect {
            min: self.origin + local.min,
            size: local.size,
        });
        clip_screen(r, clip)
    }
}

/// A screen rect held inside an optional clip. `None` means "no clip on this branch", not "clip to nothing".
#[inline]
pub(super) fn clip_screen(screen: Rect, clip: Option<Rect>) -> Rect {
    clip.map_or(screen, |c| screen.clamp_to(c))
}

/// Pad a text shape's screen rect by [`text::snap_slack`] of its local inked size, then re-clamp to `clip`: the composer paints at the ladder-snapped scale, the cascade lifts at the unsnapped one.
#[inline]
fn inflate_text_damage(screen: Rect, inked: Size, clip: Option<Rect>) -> Rect {
    // `screen` is already clipped, so a fully-off-clip run has collapsed to zero on an axis. Padding it would re-grow it across the clip edge and fabricate a damage sliver for offscreen text. Leave a non-paintable box empty; `is_paint_empty` also covers NaN and near-zero.
    if screen.is_paint_empty() {
        return screen;
    }
    let Size { w: pad_w, h: pad_h } = text::snap_slack(inked);
    let inflated = Rect {
        min: Vec2::new(screen.min.x - pad_w, screen.min.y - pad_h),
        size: Size {
            w: screen.size.w + 2.0 * pad_w,
            h: screen.size.h + 2.0 * pad_h,
        },
    };
    clip_screen(inflated, clip)
}

/// One node's rows as they are pushed, and the screen union of those that produce pixels.
#[derive(Debug)]
struct PaintSpan<'a> {
    rows: &'a mut Vec<Paint>,
    /// Seeded at `Rect::ZERO`, `Rect::union`'s identity: a node painting nothing folds to zero and a fully-clipped shape cannot drag the extent to the clip edge.
    union: Rect,
}

impl PaintSpan<'_> {
    /// Push one paint row and fold its screen rect into the union in one step, so [`PaintRectCtx::compute_paint_rect`]'s invariant cannot desync. A paint-empty screen still pushes its row (damage matches by identity) but drops out of the union through [`Rect::union`]'s identity.
    #[inline]
    fn push(&mut self, screen: Rect, hash: ContentHash) {
        self.union = self.union.union(screen);
        self.rows.push(Paint { screen, hash });
    }

    /// Push a row that produces no pixels of its own (an invisible chrome, a child marker) without touching the union.
    #[inline]
    fn push_unpainted(&mut self, hash: ContentHash) {
        self.rows.push(Paint {
            screen: Rect::ZERO,
            hash,
        });
    }
}

/// Inputs to [`Self::compute_paint_rect`], threaded from `run_tree`.
///
/// Everything here the walk already holds: `shape_transform` (`parent ∘ self_anchored`) and `clips` are computed once to avoid re-probing columns, and `visible_rect` is pushed into `hits` and `entries` anyway. Cheap indexed loads (`layout_rect`, `padding`, `has_shapes`) are derived below; [`Self::has_children`] is here because the walk decides a leaf's rollup on it.
#[derive(Clone, Copy, Debug)]
pub(super) struct PaintRectCtx<'a> {
    pub(super) tree: &'a Tree,
    pub(super) layout: &'a PlaneLayout,
    pub(super) node: NodeId,
    pub(super) visible_rect: Rect,
    pub(super) parent_transform: TranslateScale,
    pub(super) parent_clip: Option<Rect>,
    pub(super) shape_clip: Option<Rect>,
    pub(super) shape_transform: TranslateScale,
    pub(super) display_scale: f32,
    pub(super) clips: bool,
    pub(super) has_children: bool,
    /// The cascaded reading: a subtree under a non-`Visible` ancestor paints nothing.
    pub(super) invisible: bool,
}

/// What [`PaintRectCtx::compute_paint_rect`] appended for one node.
#[derive(Clone, Copy, Debug)]
pub(super) struct NodePaint {
    /// The node's rows within the `rows` the call appended to.
    pub(super) span: Span,
    /// The screen union of the pixel-producing rows, the `subtree_paint_rects` seed for the encoder's cull.
    pub(super) rect: Rect,
}

impl PaintRectCtx<'_> {
    /// Append every paint row for `node` to `rows`: chrome at row 0 when present, then direct shapes and child markers in record order.
    ///
    /// Chrome rides `parent_transform`; shapes ride `shape_transform`. Child markers are pushed raw (zero screen, child `WidgetId` as hash) so damage sees paint-order interleave.
    ///
    /// An invisible node gets an empty span and `Rect::ZERO`. Damage reads the emptied span as a rows-to-rowless transition. `Tree::container_text` uses the same reading, which [`TextRuns`]' text-record pairing depends on.
    ///
    /// # Invariant
    ///
    /// [`NodePaint::rect`] is the union of the non-paint-empty rows in `rows[span.range()]` plus the clip-only fold, so it equals `PaintRows::union_screens` except for a chromeless clip-only container, where it adds that container's visible rect: the encoder culls on it, while damage must not invent pixels. [`PaintSpan::push`] keeps union and rows in lockstep; child markers bypass it, and the clip-only branch is the one fold-without-push.
    pub(super) fn compute_paint_rect(self, rows: &mut Vec<Paint>) -> NodePaint {
        let Self {
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
            invisible,
        } = self;
        let paints_start = rows.len() as u32;
        if invisible {
            return NodePaint {
                span: Span::new(paints_start, 0),
                rect: Rect::ZERO,
            };
        }
        let layout_rect = layout.rect[node.idx()];
        let mut span = PaintSpan {
            rows,
            union: Rect::ZERO,
        };
        let chrome = Lift {
            origin: layout_rect.min,
            transform: parent_transform,
        };
        let shape = Lift {
            origin: layout_rect.min,
            transform: shape_transform,
        };

        let owner_local = Rect {
            min: Vec2::ZERO,
            size: layout_rect.size,
        };

        match tree.chrome(node) {
            Some(row) if row.is_invisible() => {
                span.push_unpainted(row.hash);
                span.union = visible_rect;
            }
            Some(row) => {
                let screen = if row.shadow.is_noop() {
                    visible_rect
                } else {
                    let chrome_local =
                        owner_local.union(row.shadow.paint_rect_local(None, layout_rect.size));
                    chrome.to_screen(chrome_local, parent_clip)
                };
                span.push(screen, row.hash);
            }
            None if clips => {
                // Chromeless clip-only container: union the owner rect into the cull rollup so the encoder emits the PushClip/PopClip pair even when the subtree paints nothing. No Paint row.
                span.union = visible_rect;
            }
            None => {}
        }

        // The owner-local bound a shape is damaged against: its own bound, or the square that bound sweeps under a rotating paint anim (what the composer culls against). No angle is read.
        let spun_if_animated = |bbox: Rect, shape_idx: u32| {
            if tree.paint_animations.rotates(shape_idx) {
                bbox.spun_cover(layout_rect.spin_pivot())
            } else {
                bbox
            }
        };
        let has_shapes = tree.records.shape_span()[node.idx()].len > 0;
        if has_shapes || has_children {
            let mut text_runs = TextRuns::new(layout.text_spans[node.idx()]);
            let shape_hashes = tree.shapes.hashes.as_slice();
            let widget_ids = tree.records.widget_id();
            for item in tree.tree_items(node) {
                let (idx, s) = match item {
                    TreeItem::ShapeRecord(idx, s) => (idx, s),
                    TreeItem::Child(c) => {
                        span.push_unpainted(ContentHash(widget_ids[c.id.idx()].0));
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
                        let anchor = TextAnchor {
                            local_origin: *local_origin,
                            align: *align,
                        };
                        let inked = anchor
                            .bbox_local(layout_rect.size, padding, shaped.extent.size)
                            .inflated_by(shaped.extent.ink);
                        // A turned run's slack turns with it, so it pads the swept square by the
                        // slack of that square: no slack vector reaches past it.
                        let swept = spun_if_animated(inked, idx);
                        inflate_text_damage(shape.to_screen(swept, None), swept.size, shape_clip)
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
                        let centerline = shape.to_screen(spun_if_animated(*bbox, idx), None);
                        let screen = StrokeReach {
                            width: *width * shape_transform.scale(),
                            fringe: AA_HALF_WIDTH / display_scale,
                            cap: *cap,
                            joints: StrokeJoints::polyline(points.len, *join),
                        }
                        .bbox(centerline);
                        clip_screen(screen, shape_clip)
                    }
                    ShapeRecord::Curve {
                        stroke, cap, bbox, ..
                    } => {
                        let centerline = shape.to_screen(spun_if_animated(*bbox, idx), None);
                        let screen = StrokeReach {
                            width: stroke.width * shape_transform.scale(),
                            fringe: AA_HALF_WIDTH / display_scale,
                            cap: *cap,
                            joints: StrokeJoints::Unjoined,
                        }
                        .bbox(centerline);
                        clip_screen(screen, shape_clip)
                    }
                    // A triangle's bbox carries its corner radius but not the physical AA fringe, added here like the stroked kinds above.
                    ShapeRecord::Quad(QuadShape::Triangle { bbox, .. }) => clip_screen(
                        shape
                            .to_screen(spun_if_animated(*bbox, idx), None)
                            .inflated(AA_HALF_WIDTH / display_scale),
                        shape_clip,
                    ),
                    // These kinds resolve their whole bound in owner-local space, so one lift finishes each. A quad's shadow halo is inside `QuadShape::bbox_local`.
                    ShapeRecord::Quad(quad) => shape.to_screen(
                        spun_if_animated(quad.bbox_local(layout_rect.size), idx),
                        shape_clip,
                    ),
                    ShapeRecord::Mesh {
                        bbox, local_rect, ..
                    } => shape.to_screen(
                        spun_if_animated(record::mesh_paint_bbox_local(*bbox, *local_rect), idx),
                        shape_clip,
                    ),
                    // Bounded by the rect the encoder draws, which overflows the base under `ImageFit::None`.
                    ShapeRecord::Image {
                        local_rect,
                        source,
                        fit,
                        ..
                    } => shape.to_screen(
                        spun_if_animated(
                            fit.resolve(owner_local.resolve_local(*local_rect), source.intrinsic())
                                .rect,
                            idx,
                        ),
                        shape_clip,
                    ),
                    ShapeRecord::Icon {
                        local_rect,
                        handle,
                        fit,
                        ..
                    } => shape.to_screen(
                        spun_if_animated(
                            fit.resolve(owner_local.resolve_local(*local_rect), handle.view_box()),
                            idx,
                        ),
                        shape_clip,
                    ),
                };
                span.push(screen, shape_hashes[idx as usize]);
            }
            debug_assert!(
                text_runs.is_drained(),
                "cascade text count differs from the node's shaped-text span",
            );
        }

        let rect = span.union;
        NodePaint {
            span: Span::new(paints_start, rows.len() as u32 - paints_start),
            rect,
        }
    }
}
