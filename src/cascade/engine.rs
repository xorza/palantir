//! The cascade walk: [`CascadeEngine`] and its scratch. The module root holds the retained product, this file the machinery that fills it.

use crate::cascade::Cascade;
use crate::cascade::cascade_input_hash::CascadeInputHash;
use crate::cascade::cascade_key::{CascadeKey, CascadeReuse};
use crate::cascade::cascade_source::CascadeSource;
use crate::cascade::counters::CascadeCounters;
use crate::cascade::paint::Paint;
use crate::cascade::paint_rect::{self, PaintRectCtx};
use crate::cascade::plane_backdrop::PlaneBackdrop;
use crate::cascade::plane_cascade::PlaneCascade;
use crate::cascade::rows::arrow_group_row::ArrowGroupRow;
use crate::cascade::rows::entry_row::EntryRow;
use crate::cascade::rows::hit_row::HitRow;
use crate::cascade::rows::root_row::RootRow;
use crate::cascade::rows::scope_row::ScopeRow;
use crate::cascade::rows::tab_stop_row::TabStopRow;
use crate::common::content_hash::ContentHash;
use crate::common::hash::Hasher;
use crate::common::non_max::NonMaxU32;
use crate::common::tracy;
use crate::layout::plane_layout::PlaneLayout;
use crate::primitives::geometry::rect::Rect;
use crate::primitives::geometry::translate_scale::TranslateScale;
use crate::primitives::identity::backdrop_key::BackdropKey;
use crate::primitives::identity::widget_id::WidgetId;
use crate::primitives::input::key_class::KeyFilter;
use crate::primitives::input::sense::Sense;
use crate::primitives::layout::clip_mode::ClipMode;
use crate::primitives::math::float_hash;
use crate::scene::forest::Forest;
use crate::scene::layer::Layer;
use crate::scene::node::inherited::Inherited;
use crate::scene::plane::Plane;
use crate::scene::tree::Tree;
use crate::scene::tree::backdrop_entry::{BackdropEntry, BackdropOrigin};
use crate::scene::tree::iter::{TreeItem, TreeItems};
use crate::scene::tree::node_id::NodeId;
use crate::shape::paint::blur_plan::BlurPlan;
use crate::shape::paint::closed_paint::ClosedPaint;
use crate::shape::paint::lowered_material::LoweredMaterial;
use crate::shape::record::ShapeRecord;
use std::hash::Hasher as _;

#[derive(Clone, Copy, Debug)]
struct NodeRows {
    node: usize,
    root: bool,
    visible_rect: Rect,
    transform: TranslateScale,
    disabled: bool,
    invisible: bool,
}

impl NodeRows {
    const fn entry(self) -> EntryRow {
        EntryRow {
            rect: self.visible_rect,
            transform: self.transform,
            disabled: self.disabled,
        }
    }
}

/// Where one tree's walk puts its per-node rows; a trait so each walk is monomorphized with the other's branches folded away.
trait RowSink {
    /// Whether the walk repairs a retained cascade in place: only then may it skip a node whose
    /// inputs held, with its subtree (`layout`'s module doc, "A subtree's rects follow from its
    /// node"), and it must bail on a paint span that changed length.
    const INCREMENTAL: bool;

    fn emit(&mut self, tree: &Tree, lc: &mut PlaneCascade, rows: NodeRows);
}

#[derive(Debug)]
struct TreeSink<'a> {
    entries: &'a mut Vec<EntryRow>,
    hits: &'a mut Vec<HitRow>,
    scopes: &'a mut Vec<ScopeRow>,
    tab_stops: &'a mut Vec<TabStopRow>,
    roots: &'a mut Vec<RootRow>,
    arrow_groups: &'a mut Vec<ArrowGroupRow>,
    plane: Plane,
    /// The root over the node the walk is on; the walk's first node is one.
    root: Option<WidgetId>,
    /// The slot in `tree.roots` of the next root the walk meets: roots are recorded in
    /// pre-order, as the walk visits them.
    next_root: usize,
}

impl RowSink for TreeSink<'_> {
    const INCREMENTAL: bool = false;

    #[inline]
    fn emit(&mut self, tree: &Tree, lc: &mut PlaneCascade, rows: NodeRows) {
        let iu = rows.node;
        let id = tree.records.widget_id()[iu];
        let attrs = tree.records.attrs()[iu];
        let plane = self.plane;
        if rows.root {
            let slot = &tree.roots[self.next_root];
            debug_assert_eq!(
                slot.first_node,
                NodeId(iu as u32),
                "roots are walked in their slots' order"
            );
            self.next_root += 1;
            self.root = Some(id);
            self.roots.push(RootRow {
                plane,
                layer: slot.layer,
                id,
            });
        }
        // The pointer layer draws a cursor over the app and takes nothing from it.
        let inert = plane.layer == Layer::Pointer;
        // Names the keyboard half alone; only `invisible` and the pointer layer remove a node from pointer routing.
        let keyboard_off = rows.disabled || rows.invisible || inert;
        // Disabling takes away what a widget may *do*, not what it occupies: it stays drawn and under the pointer, so it keeps its declared sense; the response it reads has its interaction half cleared.
        let sense = if rows.invisible || inert {
            Sense::NONE
        } else {
            attrs.sense()
        };
        // Focus is the exception: focusing a disabled widget would put the keyboard where no key is answered.
        let focusable = !keyboard_off && attrs.is_focusable();
        if !keyboard_off && let Some(axis) = attrs.arrow_focus() {
            self.arrow_groups.push(ArrowGroupRow { id, axis });
        }
        if focusable && attrs.is_tab_stop() {
            self.tab_stops.push(TabStopRow {
                plane,
                root: self.root.expect("the walk's first node is a root"),
                id,
                idx: tree.bounds(NodeId(iu as u32)).tab_idx,
            });
        }
        lc.hit_rows[iu] = (sense != Sense::NONE || focusable).then(|| {
            self.hits.push(HitRow {
                rect: rows.visible_rect,
                widget_id: id,
                sense,
                focusable,
                disabled: rows.disabled,
            });
            NonMaxU32::index(self.hits.len() - 1)
        });
        let filter = if keyboard_off {
            KeyFilter::NONE
        } else {
            attrs.key_filter()
        };
        if filter.is_scope() {
            self.scopes.push(ScopeRow {
                plane,
                id,
                filter,
                blocks_below: attrs.blocks_below(),
            });
        }
        self.entries.push(rows.entry());
    }
}

/// The rows a refresh rewrites in place: one plane's block of [`Cascade::entries`] and the hit
/// table; the key proved every other table unchanged. `INCREMENTAL` patches the plane's paint
/// arena in place; without it the walk lays the arena out afresh.
#[derive(Debug)]
struct TreePatch<'a, const INCREMENTAL: bool> {
    entries: &'a mut [EntryRow],
    hits: &'a mut [HitRow],
}

/// How [`CascadeEngine::refresh`] rewrites the paint arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Refresh {
    /// In place, skipping each node whose inputs held, on each plane whose row counts held.
    Patch,
    /// Afresh, every node walked.
    Repack,
}

impl<const I: bool> RowSink for TreePatch<'_, I> {
    const INCREMENTAL: bool = I;

    /// Only the rect and inherited transform are rewritten.
    #[inline]
    fn emit(&mut self, _tree: &Tree, lc: &mut PlaneCascade, rows: NodeRows) {
        self.entries[rows.node] = rows.entry();
        if let Some(hit) = lc.hit_rows[rows.node] {
            self.hits[hit.idx()].rect = rows.visible_rect;
        }
    }
}

/// The values a node hands its descendants, the only inputs [`Self::prefix`] hashes.
#[derive(Clone, Copy, Debug)]
pub(super) struct CascadeCtx {
    pub(super) transform: TranslateScale,
    pub(super) clip: Option<Rect>,
    /// The rounded clips around the node, outer to inner, as one fold of each one's screen mask:
    /// a clip that turns from a rect to a rounded one moves no rect, yet repaints its corners.
    pub(super) rounded: ContentHash,
    pub(super) inherited: Inherited,
}

impl CascadeCtx {
    pub(super) const ROOT: Self = Self {
        transform: TranslateScale::IDENTITY,
        clip: None,
        rounded: ContentHash::from_u64(0),
        inherited: Inherited::ROOT,
    };

    /// Ancestor-derived portion of the `cascade_input` hash, folded once per frame at push and cloned per descendant. Fed as words from registers: hashing a stored struct as bytes reads stores back across seams that can't be forwarded.
    #[inline]
    pub(super) fn prefix(self) -> Hasher {
        let (clip, clip_present) = match self.clip {
            Some(rect) => (rect.canon_lanes(), true),
            None => ([0; 4], false),
        };
        let flags = u32::from(clip_present)
            | (u32::from(self.inherited.disabled) << 1)
            | (u32::from(self.inherited.invisible) << 2);
        let word = |lo: u32, hi: u32| u64::from(lo) | (u64::from(hi) << 32);
        let mut h = Hasher::new();
        h.write_u64(word(
            float_hash::canon_bits(self.transform.translation().x),
            float_hash::canon_bits(self.transform.translation().y),
        ));
        h.write_u64(word(
            float_hash::canon_bits(self.transform.scale() - 1.0),
            flags,
        ));
        h.write_u64(word(clip[0], clip[1]));
        h.write_u64(word(clip[2], clip[3]));
        h.write_u64(self.rounded.as_u64());
        h
    }

    /// The fold [`Self::rounded`] hands a node's descendants: its own with a rounded clip of
    /// `mask`, the node's screen mask, folded in last.
    fn rounded_within(self, mask: Rect) -> ContentHash {
        let mut h = Hasher::new();
        h.write_u64(self.rounded.as_u64());
        let lanes = mask.canon_lanes();
        h.write_u64(u64::from(lanes[0]) | (u64::from(lanes[1]) << 32));
        h.write_u64(u64::from(lanes[2]) | (u64::from(lanes[3]) << 32));
        ContentHash::finish(&h)
    }
}

/// One plane's read-only input to [`CascadeEngine::run_tree`].
#[derive(Clone, Copy, Debug)]
struct TreeSource<'a> {
    tree: &'a Tree,
    layout: &'a PlaneLayout,
    display_scale: f32,
}

#[derive(Debug)]
struct Frame {
    cascade: CascadeCtx,
    subtree_end: u32,
    node_idx: usize,
    subtree_paint_rect: Rect,
    /// FxHasher state seeded with this frame's ancestor-derived inputs, cloned per descendant so the 32 B prefix isn't re-hashed.
    cascade_prefix: Hasher,
}

#[derive(Debug, Default)]
pub(crate) struct CascadeEngine {
    stack: Vec<Frame>,
    /// One node's rows on the incremental path, cleared per node: each node's rows are copied over its retained span at once, so the buffer never holds more than one node.
    paint_rows: Vec<Paint>,
    pub(crate) counters: CascadeCounters,
}

impl CascadeEngine {
    /// Brings the frozen cascade up to `source`'s key: an unchanged key skips, and a moved one
    /// keeps what [`CascadeKey::reuse`] says still holds.
    pub(crate) fn run(&mut self, source: CascadeSource<'_>, cascade: &mut Cascade) {
        tracy::zone!();
        let key = CascadeKey::new(source);
        let ran = cascade.key != Some(key);
        self.counters.note_ran(ran);
        if !ran {
            return;
        }
        match cascade
            .key
            .map_or(CascadeReuse::Nothing, |built| built.reuse(&key))
        {
            CascadeReuse::Nothing => self.rebuild(source, cascade),
            CascadeReuse::Structure => self.refresh(source, cascade, Refresh::Repack),
            CascadeReuse::Rows => self.refresh(source, cascade, Refresh::Patch),
        }
        seal(source.forest, key, cascade);
    }

    /// Rewrite `cascade`, whose structural tables hold, in place, plane by plane: patched while
    /// `how` and the plane's row counts allow, else repacked, as is a plane whose patch meets a
    /// paint span that changed length.
    fn refresh(&mut self, source: CascadeSource<'_>, cascade: &mut Cascade, how: Refresh) {
        debug_assert_retained_fits(source.forest, cascade);
        for (plane, tree) in source.forest.trees.iter_paint_order() {
            let n = tree.records.len();
            let tree_source = TreeSource {
                tree,
                layout: &source.layout[plane],
                display_scale: source.display.scale_factor(),
            };
            let lc = &mut cascade.planes[plane];
            let base = lc.entries_base as usize;
            let entries = &mut cascade.entries[base..base + n];
            let counts = tree.fingerprint.paint_counts;
            let patch = how == Refresh::Patch && lc.paint_counts == counts;
            lc.paint_counts = counts;
            if patch {
                self.stack.clear();
                let patched = self.run_tree(
                    tree_source,
                    lc,
                    &mut TreePatch::<true> {
                        entries: &mut *entries,
                        hits: &mut cascade.hits,
                    },
                );
                if patched {
                    continue;
                }
                self.counters.abandoned_incremental();
            }
            self.counters.repack();
            lc.paint_arena.reset_for(n);
            self.stack.clear();
            let complete = self.run_tree(
                tree_source,
                lc,
                &mut TreePatch::<false> {
                    entries,
                    hits: &mut cascade.hits,
                },
            );
            debug_assert!(complete, "a walk that skips nothing completes");
            lc.arena_hashes.copy_from_slice(&tree.rollups.subtree);
        }
    }

    fn rebuild(&mut self, source: CascadeSource<'_>, cascade: &mut Cascade) {
        self.counters.full_rebuild();
        let forest = source.forest;
        let total = forest.total_nodes();
        cascade.entries.clear();
        cascade.entries.reserve_exact(total);
        cascade.hits.clear();
        cascade.scopes.clear();
        cascade.tab_stops.clear();
        cascade.roots.clear();
        cascade.arrow_groups.clear();
        cascade.planes.cover(&forest.trees);

        for (plane, tree) in forest.trees.iter_paint_order() {
            let n = tree.records.len();
            let entries_base = cascade.entries.len() as u32;
            let lc = &mut cascade.planes[plane];
            lc.reset_for(n, entries_base);
            self.stack.clear();
            let full_complete = self.run_tree(
                TreeSource {
                    tree,
                    layout: &source.layout[plane],
                    display_scale: source.display.scale_factor(),
                },
                &mut cascade.planes[plane],
                &mut TreeSink {
                    entries: &mut cascade.entries,
                    hits: &mut cascade.hits,
                    scopes: &mut cascade.scopes,
                    tab_stops: &mut cascade.tab_stops,
                    roots: &mut cascade.roots,
                    arrow_groups: &mut cascade.arrow_groups,
                    plane,
                    root: None,
                    next_root: 0,
                },
            );
            debug_assert!(full_complete);
            let lc = &mut cascade.planes[plane];
            lc.arena_hashes.copy_from_slice(&tree.rollups.subtree);
            lc.paint_counts = tree.fingerprint.paint_counts;
            debug_assert_eq!(
                cascade.entries.len() as u32 - entries_base,
                n as u32,
                "run_tree must emit one entry per recorded node",
            );
        }

        // `SeenIds::pre_record` clears `cur` before a relayout pass can query the preceding pass's responses.
        //
        // Refilled, not `clone_from`: `cur` and `prev` swap every frame and hashbrown's `clone_from` reallocates when bucket counts differ.
        cascade.resolved.clear();
        cascade.resolved.extend(forest.ids.resolved());
        cascade.by_id.clear();
        cascade.by_id.extend(
            cascade
                .resolved
                .iter()
                .filter_map(|row| Some((row.id, row.endpoint?))),
        );
    }
}

/// Stamp `cascade`, now built from `forest`, with `key`, and list its backdrops' keys.
fn seal(forest: &Forest, key: CascadeKey, cascade: &mut Cascade) {
    cascade.key = Some(key);
    let Cascade {
        planes,
        backdrop_keys,
        ..
    } = cascade;
    backdrop_keys.refill(
        forest
            .trees
            .iter_paint_order()
            .flat_map(|(plane, _)| planes[plane].backdrops.iter().map(|backdrop| backdrop.key)),
    );
}

/// A cascade whose key keeps its structure still has one entry per node and one arena column per plane, in `forest`'s paint order.
fn debug_assert_retained_fits(forest: &Forest, cascade: &Cascade) {
    debug_assert_eq!(cascade.entries.len(), forest.total_nodes());
    let mut entries_base = 0u32;
    for (plane, tree) in forest.trees.iter_paint_order() {
        let n = tree.records.len();
        let lc = &cascade.planes[plane];
        debug_assert_eq!(lc.entries_base, entries_base);
        debug_assert_eq!(
            lc.arena_hashes.len(),
            n,
            "retained arena column outlived its plane",
        );
        entries_base += n as u32;
    }
}

/// Finalizes one stack frame: writes the rolled-up `subtree_paint_rect` and unions upward into the new top.
#[inline]
const fn finalize_frame(stack: &mut [Frame], subtree_paint_rects: &mut [Rect], popped: &Frame) {
    subtree_paint_rects[popped.node_idx] = popped.subtree_paint_rect;
    if let Some(parent) = stack.last_mut() {
        parent.subtree_paint_rect = parent.subtree_paint_rect.union(popped.subtree_paint_rect);
    }
}

impl CascadeEngine {
    /// Walks one tree, writing its cascade columns. Returns whether it *completed*; only the incremental path returns `false`, when a node's repaired paint span changes length and the caller must fully rebuild.
    ///
    /// **Incremental recompute.** A node whose `cascade_input` (inherited state and own rect) and subtree rollup both held keeps its rows; a moved transform or clip changes every child's prefix, so the change reaches exactly its subtree. A clean node is skipped with its subtree (`layout`'s module doc, "A subtree's rects follow from its node"). Movement within `EPS` of zero reads as held.
    fn run_tree<S: RowSink>(
        &mut self,
        source: TreeSource<'_>,
        lc: &mut PlaneCascade,
        sink: &mut S,
    ) -> bool {
        let TreeSource {
            tree,
            layout,
            display_scale,
        } = source;
        let n = tree.records.len() as u32;
        let layout_col = tree.records.layout();
        let attrs_col = tree.records.attrs();
        let ends = tree.records.subtree_end();
        let subtree_hashes = tree.rollups.subtree.as_slice();
        let root_prefix = CascadeCtx::ROOT.prefix();

        let mut i: u32 = 0;
        while i < n {
            while let Some(popped) = self.stack.pop_if(|top| i >= top.subtree_end) {
                finalize_frame(&mut self.stack, &mut lc.subtree_paint_rects, &popped);
            }
            let top = self.stack.last();
            let parent = top.map_or(CascadeCtx::ROOT, |frame| frame.cascade);
            let parent_prefix = top.map_or(&root_prefix, |frame| &frame.cascade_prefix);

            let iu = i as usize;
            let id = NodeId(i);
            let attrs = attrs_col[iu];
            let layout_core = layout_col[iu];

            let inherited = parent.inherited.child(attrs, layout_core.meta.visibility());
            let Inherited {
                disabled,
                invisible,
            } = inherited;

            let layout_rect = layout.rect[iu];
            let subtree_end = ends[iu].end();
            let has_children = ends[iu].has_children(iu);
            let outside = layout.outside_input(tree, id);
            let cascade_input =
                CascadeInputHash::finish(parent_prefix, layout_rect, outside, invisible);
            let clean = S::INCREMENTAL
                && lc.cascade_inputs[iu] == cascade_input
                && lc.arena_hashes[iu] == subtree_hashes[iu];
            if clean {
                if let Some(parent_frame) = self.stack.last_mut() {
                    parent_frame.subtree_paint_rect = parent_frame
                        .subtree_paint_rect
                        .union(lc.subtree_paint_rects[iu]);
                }
                i = subtree_end;
                continue;
            }

            let screen_rect = parent.transform.apply_rect(layout_rect);
            let visible_rect = paint_rect::clip_screen(screen_rect, parent.clip);
            // The transform descendants inherit and direct shapes paint under (`Panel::transform`): `parent ∘ self_anchored`. The `None` arm skips the compose; `compute_paint_rect` reuses it.
            let desc_transform = match tree.anchored_transform(id, layout_rect) {
                Some(t) => parent.transform.compose(t),
                None => parent.transform,
            };
            let clips = attrs.clip_mode().is_clip();
            // The encoder pushes this inner box as the clip mask before the body; clipping here keeps shape damage and child clips matching what paints (else a TextEdit's tall text shape damages past the editor on every scroll).
            let shape_clip = if clips {
                let mask_screen = parent
                    .transform
                    .apply_rect(layout_core.inner_rect(layout_rect));
                Some(paint_rect::clip_screen(mask_screen, parent.clip))
            } else {
                parent.clip
            };
            let rounded = match (attrs.clip_mode(), shape_clip) {
                (ClipMode::Rounded, Some(mask)) => parent.rounded_within(mask),
                _ => parent.rounded,
            };
            // Invisible nodes never paint; a `Rect::ZERO` rollup seed keeps a hidden subtree from inflating the ancestor's `subtree_paint_rect` and defeating the cull.
            let ctx = PaintRectCtx {
                tree,
                layout,
                node: id,
                visible_rect,
                parent_transform: parent.transform,
                parent_clip: parent.clip,
                shape_clip,
                shape_transform: desc_transform,
                display_scale,
                clips,
                has_children,
                invisible,
            };
            let paint_rect = if S::INCREMENTAL {
                self.counters.refreshed_node();
                let old_span = lc.paint_arena.node_spans[iu];
                self.paint_rows.clear();
                let paint = ctx.compute_paint_rect(&mut self.paint_rows);
                if old_span.len != paint.span.len {
                    return false;
                }
                lc.paint_arena.rows[old_span.range()].copy_from_slice(&self.paint_rows);
                paint.rect
            } else {
                let paint = ctx.compute_paint_rect(&mut lc.paint_arena.rows);
                lc.paint_arena.node_spans[iu] = paint.span;
                paint.rect
            };
            let subtree_seed = if invisible { Rect::ZERO } else { paint_rect };
            if S::INCREMENTAL {
                lc.arena_hashes[iu] = subtree_hashes[iu];
            } else {
                lc.subtree_ends[iu] = subtree_end;
            }
            lc.cascade_inputs[iu] = cascade_input;
            lc.shape_scales[iu] = desc_transform.scale();
            sink.emit(
                tree,
                lc,
                NodeRows {
                    node: iu,
                    root: self.stack.is_empty(),
                    visible_rect,
                    transform: parent.transform,
                    disabled,
                    invisible,
                },
            );
            lc.subtree_paint_rects[iu] = subtree_seed;

            if has_children {
                let cascade = CascadeCtx {
                    transform: desc_transform,
                    clip: shape_clip,
                    rounded,
                    inherited,
                };
                self.stack.push(Frame {
                    cascade,
                    subtree_end,
                    node_idx: iu,
                    subtree_paint_rect: subtree_seed,
                    cascade_prefix: cascade.prefix(),
                });
            } else if let Some(parent_frame) = self.stack.last_mut() {
                // Leaf: no frame; its slot holds the seed, folded into the parent. Skips a Frame push/pop and prefix hashing.
                parent_frame.subtree_paint_rect =
                    parent_frame.subtree_paint_rect.union(subtree_seed);
            }
            i += 1;
        }
        while let Some(popped) = self.stack.pop() {
            finalize_frame(&mut self.stack, &mut lc.subtree_paint_rects, &popped);
        }
        source.list_backdrops(lc);
        true
    }
}

impl TreeSource<'_> {
    /// Rebuild `lc.backdrops` from the backdrops `tree` recorded, each at its paint row and that
    /// row's screen, and `lc.backdrop_rows` from them.
    fn list_backdrops(self, lc: &mut PlaneCascade) {
        let Self {
            tree,
            display_scale,
            ..
        } = self;
        lc.backdrops.clear();
        let widget_ids = tree.records.widget_id();
        for entry in &tree.backdrops {
            let node = entry.node.idx();
            let span = lc.paint_arena.node_spans[node];
            if span.len == 0 {
                continue;
            }
            debug_assert!(
                entry.row < span.len,
                "a backdrop's row lies in its owner's span"
            );
            let row = span.start + entry.row;
            let rect = lc.paint_arena.rows[row as usize].screen;
            if rect.is_paint_empty() {
                continue;
            }
            let paint_row = after_children(tree, lc, entry).max(row);
            let capture = match entry.origin {
                BackdropOrigin::Chrome => tree
                    .chrome(entry.node)
                    .and_then(|row| row.material)
                    .and_then(LoweredMaterial::capture),
                BackdropOrigin::Shape => {
                    tree.shapes.records[entry.shape_idx as usize].backdrop_capture()
                }
            }
            .expect("a listed backdrop reads under it");
            let shape_scale = lc.shape_scales[node];
            let scale = shape_scale * display_scale;
            let plan = BlurPlan::new(capture.sigma * scale);
            let narrowest = self
                .narrowest_side(entry, rect, shape_scale)
                .map(|n| n * display_scale);
            lc.backdrops.push(PlaneBackdrop {
                key: BackdropKey {
                    owner: widget_ids[node],
                    shape: entry.row,
                },
                row,
                paint_row,
                rect,
                plan,
                footprint: rect.inflated(
                    PlaneBackdrop::reach(plan, capture.reach.margin(scale, narrowest))
                        / display_scale,
                ),
            });
        }
        lc.backdrop_rows.clear();
        lc.backdrop_rows
            .extend(lc.backdrops.iter().map(|backdrop| backdrop.row));
        lc.backdrop_rows.sort_unstable();
    }

    /// The narrowest side, in logical px, of a backdrop drawn whole at `rect` on the screen, which
    /// a lens reads inside of: a shape's quad that no paint animation turns and no clip cuts.
    /// `None` for a turning or clipped backdrop, whose screen rect is not its own, and for chrome,
    /// whose lift this walk does not keep.
    fn narrowest_side(self, entry: &BackdropEntry, rect: Rect, shape_scale: f32) -> Option<f32> {
        let Self { tree, layout, .. } = self;
        if entry.origin != BackdropOrigin::Shape || tree.paint_animations.rotates(entry.shape_idx) {
            return None;
        }
        let ShapeRecord::Closed {
            geometry,
            paint: ClosedPaint::Fill(paint),
        } = &tree.shapes.records[entry.shape_idx as usize]
        else {
            return None;
        };
        let local = paint
            .bbox_local(*geometry, layout.rect[entry.node.idx()].size)
            .size;
        let (w, h) = (local.w * shape_scale, local.h * shape_scale);
        // The lift and a clip that misses the backdrop round the size within a few f32 ulps; a clip
        // that cuts less than this lets a sample out by less than the pixel `PlaneBackdrop::reach`
        // rounds out.
        let whole = |screen: f32, lifted: f32| screen >= lifted * (1.0 - 1e-5);
        (whole(rect.size.w, w) && whole(rect.size.h, h)).then_some(w.min(h))
    }
}

/// The first row past the subtree of the last child `entry`'s node records before the backdrop, `0`
/// for a backdrop no child precedes: a node's rows lie before its children's, each subtree's rows
/// in one run from its root's span, so the run ends where the next node's span starts.
fn after_children(tree: &Tree, lc: &PlaneCascade, entry: &BackdropEntry) -> u32 {
    if entry.origin == BackdropOrigin::Chrome {
        return 0;
    }
    let mut child = None;
    for item in TreeItems::new(&tree.records, &tree.shapes.records, entry.node) {
        match item {
            TreeItem::ShapeRecord(idx, _) if idx == entry.shape_idx => break,
            TreeItem::ShapeRecord(..) => {}
            TreeItem::Child(c) => child = Some(c.id),
        }
    }
    let Some(child) = child else {
        return 0;
    };
    let spans = &lc.paint_arena.node_spans;
    let next = lc.subtree_ends[child.idx()] as usize;
    let end = spans
        .get(next)
        .map_or(lc.paint_arena.rows.len() as u32, |span| span.start);
    debug_assert!(
        spans[child.idx()..next]
            .iter()
            .all(|span| span.start >= spans[child.idx()].start && span.start + span.len <= end),
        "a subtree's rows lie in one run, before the next node's"
    );
    end
}

#[cfg(feature = "internals")]
pub(crate) mod internals {
    use crate::cascade::Cascade;
    use crate::cascade::cascade_key::CascadeKey;
    use crate::cascade::cascade_source::CascadeSource;
    use crate::cascade::engine::{CascadeEngine, seal};
    #[cfg(test)]
    use crate::cascade::paint::Paint;
    #[cfg(test)]
    use crate::internals::scratch_report;

    impl CascadeEngine {
        /// Rebuilds every row from `source`, whatever the cascade holds.
        pub(crate) fn run_full(&mut self, source: CascadeSource<'_>, cascade: &mut Cascade) {
            self.rebuild(source, cascade);
            seal(source.forest, CascadeKey::new(source), cascade);
        }

        /// The last node's rows the incremental walk recomputed.
        #[cfg(test)]
        pub(crate) fn paint_rows(&self) -> &[Paint] {
            &self.paint_rows
        }

        #[cfg(test)]
        pub(crate) fn scratch_bytes(&self) -> usize {
            let Self {
                stack,
                paint_rows,
                counters: _,
            } = self;
            scratch_report::vec_bytes(stack) + scratch_report::vec_bytes(paint_rows)
        }
    }
}
