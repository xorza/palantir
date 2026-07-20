use crate::layout::Layout;
use crate::layout::axis::Axis;
use crate::layout::engine::LayoutEngine;
use crate::layout::intrinsic::{IntrinsicQuery, IntrinsicRange, LenReq};
use crate::layout::support::{
    AxisAlignPair, TextCtx, arrange_axis, resolved_axis_align, weighted_share, zero_subtree,
};
use crate::layout::types::layout_mode::{GridDefId, LayoutMode};
use crate::layout::types::track::Track;
use crate::primitives::span::Span;
use crate::primitives::{rect::Rect, size::Size};
use crate::scene::tree::Tree;
use crate::scene::tree::node::NodeId;
use fixedbitset::FixedBitSet;
use glam::Vec2;
use std::ops::Range;

#[derive(Clone, Copy, Debug)]
pub(crate) enum HugKind {
    Max,
    Min,
}

/// Pack/unpack order for hug arrays inside a snapshot. Single source of
/// truth — `snapshot_subtree` and `restore_subtree` both iterate this,
/// so reordering one without the other is impossible.
const HUG_ORDER: [(Axis, HugKind); 4] = [
    (Axis::X, HugKind::Max),
    (Axis::X, HugKind::Min),
    (Axis::Y, HugKind::Max),
    (Axis::Y, HugKind::Min),
];

/// Zero this grid's hug arrays so a re-measure of the grid (e.g.,
/// `LayoutEngine::measure`'s grow-driven second pass) starts with a
/// clean accumulator. Both Phase 1 col-intrinsic queries and Phase 2
/// cell-height records merge via `slot[i] = slot[i].max(...)`; without
/// this reset, a re-measure under a wider `available` would keep the
/// previous narrower-pass row heights, leaving cells over-allocated
/// and inflating the grid's `desired.h`. Measure-only — arrange must
/// preserve these. Pinned by
/// `cross_driver_tests::parent_contains_child::two_hug_cols_section_height_matches_post_grow_text`.
fn reset_hugs_for(layout: &mut LayoutEngine, idx: GridDefId) {
    let hugs = &mut layout.scratch.grid.hugs;
    for (axis, kind) in HUG_ORDER {
        hugs.slice_mut(idx, axis, kind).fill(0.0);
    }
}

/// Per-axis scratch for one nesting depth. `flexible` and `hug_bounds`
/// are transient lists used only inside `resolve_axis`; they live on
/// the per-axis struct so their capacity is retained across frames.
///
/// Per-track content-driven `[min, max]` Hug ranges live in
/// `GridHugStore` (durable across the whole layout pass); they're passed
/// into `resolve_axis` as slices alongside this scratch.
#[derive(Debug, Default)]
pub(crate) struct AxisScratch {
    pub(crate) sizes: Vec<f32>,
    pub(crate) resolved: FixedBitSet,
    pub(crate) offsets: Vec<f32>,
    flexible: Vec<usize>,
    hug_bounds: Vec<HugBound>,
}

#[derive(Clone, Copy, Debug)]
struct HugBound {
    idx: usize,
    lo: f32,
    hi: f32,
}

impl AxisScratch {
    /// Resize the per-track arrays. All arrays are zeroed; `resolved` is
    /// reset to all-false. Capacity is retained across frames.
    fn reset(&mut self, n: usize) {
        self.sizes.clear();
        self.sizes.resize(n, 0.0);
        self.resolved.clear();
        self.resolved.grow(n);
        self.offsets.clear();
        self.offsets.resize(n, 0.0);
    }
}

/// Per-frame scratch for `Grid` layout. Capacity is retained across frames; a
/// `Vec<GridScratch>` indexed by nesting depth lets nested grids each have
/// their own slot. Pushed on first descent to a new depth.
#[derive(Debug, Default)]
pub(crate) struct GridScratch {
    pub(crate) col: AxisScratch,
    pub(crate) row: AxisScratch,
}

/// All grid-layout scratch held by `LayoutEngine`, in one bag. `depth_stack`
/// and `hugs` are separate fields so callers can disjoint-borrow them —
/// `resolve_axis` takes `&mut AxisScratch` (from `depth_stack`) and `&[f32]`
/// hug slices (from `hugs`) in the same expression via destructuring.
/// `track_aggregator` is a bump-stack scratch for `grid::intrinsic`'s
/// per-track aggregator: each call extends by `n_tracks`, recurses (which
/// may extend further but always truncates back), then truncates to its
/// own base. Capacity retained.
#[derive(Debug, Default)]
pub(crate) struct GridContext {
    pub(crate) depth_stack: GridDepthStack,
    pub(crate) hugs: GridHugStore,
    pub(crate) track_aggregator: Vec<f32>,
}

/// Nesting stack of per-depth grid scratch. One `GridScratch` slot per
/// active `LayoutMode::Grid` ancestor. `depth` is the next free slot.
#[derive(Debug, Default)]
pub(crate) struct GridDepthStack {
    scratch: Vec<GridScratch>,
    pub(crate) depth: usize,
}

impl GridDepthStack {
    /// Reserve a scratch slot for the next nesting depth. Grows on first
    /// descent; reuses thereafter.
    fn enter(&mut self) -> usize {
        let d = self.depth;
        if self.scratch.len() == d {
            self.scratch.push(GridScratch::default());
        }
        self.depth = d + 1;
        d
    }

    fn exit(&mut self) {
        debug_assert!(self.depth > 0, "GridDepthStack::exit underflow");
        self.depth -= 1;
    }

    fn at(&mut self, depth: usize) -> &mut GridScratch {
        &mut self.scratch[depth]
    }
}

/// Flat per-track pool with one `(rows, cols)` slot per recorded
/// `GridDef`. Carries hug ranges (`max`/`min`, fed by Phase-1 cell
/// intrinsics and Phase-2 cell-height accumulation), measure-resolved
/// track sizes (`sizes`, the output of `resolve_axis`), and the input
/// `total` each axis was resolved against (`totals`). Measure pass
/// writes; arrange pass reads. Per-depth scratch in `depth_stack`
/// gets clobbered by sibling grids before arrange runs, so the pool
/// persists for the whole layout pass instead.
///
/// `reset_for` zeroes every slot at the top of each pass — load-bearing
/// for `max`/`min`/`sizes` because the Phase 1 column loop and the
/// Phase 2 cell-height accumulator both merge via `slot[i] =
/// slot[i].max(...)` and assume a 0.0 starting state. `totals` is also
/// zeroed; arrange interprets `total == 0.0` (combined with non-zero
/// arrange slot) as "measure didn't run this frame for this grid" and
/// falls back to re-resolving (the cache-hit-ancestor path).
///
/// Capacity retained across frames.
#[derive(Debug, Default)]
pub(crate) struct GridHugStore {
    max_pool: Vec<f32>,
    min_pool: Vec<f32>,
    /// Resolved track sizes from the last measure of each grid. Parallel
    /// indexing to `max_pool`/`min_pool` via the same per-slot spans.
    /// Read by arrange to skip a redundant `resolve_axis` call when the
    /// arrange-time slot matches the measure-time total.
    sizes_pool: Vec<f32>,
    /// `[col_total, row_total]` per grid slot — the `total` each axis
    /// was last resolved against. Arrange compares against the
    /// arrange-time slot extent and reuses persisted sizes on match.
    totals_pool: Vec<[f32; 2]>,
    slots: Vec<GridHugSlot>,
}

#[derive(Clone, Copy, Debug)]
struct GridHugSlot {
    rows: Span,
    cols: Span,
}

impl GridHugStore {
    pub(crate) fn reset_for(&mut self, tree: &Tree) {
        self.max_pool.clear();
        self.min_pool.clear();
        self.sizes_pool.clear();
        self.totals_pool.clear();
        self.slots.clear();
        for def in &tree.grid_defs {
            let rows = self.alloc(def.rows.len as usize);
            let cols = self.alloc(def.cols.len as usize);
            self.slots.push(GridHugSlot { rows, cols });
            self.totals_pool.push([0.0, 0.0]);
        }
    }

    fn alloc(&mut self, n: usize) -> Span {
        let start = self.max_pool.len() as u32;
        self.max_pool.resize(start as usize + n, 0.0);
        self.min_pool.resize(start as usize + n, 0.0);
        self.sizes_pool.resize(start as usize + n, 0.0);
        Span::new(start, n as u32)
    }

    fn axis_slice(&self, idx: GridDefId, axis: Axis) -> Range<usize> {
        let slot = self.slots[usize::from(idx)];
        let s = match axis {
            Axis::X => slot.cols,
            Axis::Y => slot.rows,
        };
        s.range()
    }

    pub(crate) fn slice(&self, idx: GridDefId, axis: Axis, kind: HugKind) -> &[f32] {
        let r = self.axis_slice(idx, axis);
        match kind {
            HugKind::Max => &self.max_pool[r],
            HugKind::Min => &self.min_pool[r],
        }
    }

    pub(crate) fn slice_mut(&mut self, idx: GridDefId, axis: Axis, kind: HugKind) -> &mut [f32] {
        let r = self.axis_slice(idx, axis);
        match kind {
            HugKind::Max => &mut self.max_pool[r],
            HugKind::Min => &mut self.min_pool[r],
        }
    }

    /// Both pools' slices for one `(idx, axis)` in one call. Single
    /// slot lookup; the borrow checker splits the `&mut self` because
    /// `min_pool` and `max_pool` are separate fields.
    pub(crate) fn slice_mut_pair(
        &mut self,
        idx: GridDefId,
        axis: Axis,
    ) -> (&mut [f32], &mut [f32]) {
        let r = self.axis_slice(idx, axis);
        (&mut self.min_pool[r.clone()], &mut self.max_pool[r])
    }

    fn axis_total_idx(axis: Axis) -> usize {
        match axis {
            Axis::X => 0,
            Axis::Y => 1,
        }
    }

    /// Persisted resolved track sizes for `(idx, axis)` from the last
    /// measure. Empty-equivalent until measure writes via
    /// `record_resolution`.
    pub(crate) fn sizes_slice(&self, idx: GridDefId, axis: Axis) -> &[f32] {
        let r = self.axis_slice(idx, axis);
        &self.sizes_pool[r]
    }

    /// `total` (measure-time `resolve_axis` input) for `(idx, axis)`.
    /// Returns `0.0` for grids that haven't been measured this frame
    /// (e.g. cache-hit descendants); arrange treats that as "no
    /// persisted state" and re-resolves.
    pub(crate) fn total_used(&self, idx: GridDefId, axis: Axis) -> f32 {
        self.totals_pool[usize::from(idx)][Self::axis_total_idx(axis)]
    }

    /// Snapshot the just-resolved `(sizes, total)` for `(idx, axis)`
    /// so a sibling-clobber-resistant arrange can read them back
    /// without re-running `resolve_axis`. Caller passes the same
    /// `total` it just handed to `resolve_axis` plus the resolved
    /// `sizes` slice from the per-depth scratch.
    pub(crate) fn record_resolution(
        &mut self,
        idx: GridDefId,
        axis: Axis,
        total: f32,
        sizes: &[f32],
    ) {
        let r = self.axis_slice(idx, axis);
        self.sizes_pool[r].copy_from_slice(sizes);
        self.totals_pool[usize::from(idx)][Self::axis_total_idx(axis)] = total;
    }

    /// Pack per-grid hug arrays for every `LayoutMode::Grid` descendant
    /// in `subtree` (pre-order node-index range) into `out`. Used by
    /// the cross-frame measure cache: when a subtree is snapshotted,
    /// arrange's hug state must be saved so a later cache hit at any
    /// ancestor can restore it via [`Self::restore_subtree`]. Order is
    /// dictated by [`HUG_ORDER`] per Grid, in pre-order.
    pub(crate) fn snapshot_subtree(&self, tree: &Tree, subtree: Range<usize>, out: &mut Vec<f32>) {
        let layouts = tree.records.layout();
        for i in subtree {
            let core = layouts[i];
            if core.mode == LayoutMode::Grid {
                let idx = core.grid_def_id();
                for (axis, kind) in HUG_ORDER {
                    out.extend_from_slice(self.slice(idx, axis, kind));
                }
            }
        }
    }

    /// Inverse of `snapshot_subtree`: walks the same pre-order range
    /// and pours four hug arrays per Grid back into the slot at the
    /// current frame's `idx`. `subtree_hash` equality on the cache key
    /// guarantees same Grid count and same `(n_cols, n_rows)` per
    /// Grid in the same order, so the slice and the walk align.
    pub(crate) fn restore_subtree(&mut self, tree: &Tree, subtree: Range<usize>, hugs: &[f32]) {
        let layouts = tree.records.layout();
        let mut pos = 0usize;
        for i in subtree {
            let core = layouts[i];
            if core.mode == LayoutMode::Grid {
                let idx = core.grid_def_id();
                for (axis, kind) in HUG_ORDER {
                    let dst = self.slice_mut(idx, axis, kind);
                    let n = dst.len();
                    dst.copy_from_slice(&hugs[pos..pos + n]);
                    pos += n;
                }
            }
        }
        debug_assert_eq!(
            pos,
            hugs.len(),
            "snapshot hug slice length disagrees with current subtree's grid descendants \
             (cache key let through a structural change?)",
        );
    }
}

/// WPF-style grid measure. Resolves Fixed tracks, walks children once feeding
/// each `Σ spanned-track sizes` (or `∞` if any spanned track is unresolved —
/// the WPF infinity trick → child reports intrinsic), then resolves Hug
/// tracks from span-1 children's desired sizes. Star tracks contribute 0 to
/// the grid's content size — final star sizes only resolve in arrange. The
/// full constraint solver is documented on [`resolve_axis`].
///
/// Per-depth scratch (`AxisScratch` columns) lives in `grid.depth_stack`
/// and gets clobbered by sibling grids between this measure and the
/// matching arrange. Hug sizes therefore live in `grid.hugs`
/// (`GridHugStore`), keyed by `GridDef` index, durable for the whole
/// layout pass. Both are heap-resident and capacity-retained across
/// frames; no fixed track-count limit.
#[profiling::function]
pub(crate) fn measure(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    idx: GridDefId,
    inner_avail: Size,
    tc: &TextCtx<'_>,
    out: &mut Layout,
) -> Size {
    let depth = layout.scratch.grid.depth_stack.enter();
    let result = measure_inner(layout, tree, node, idx, depth, inner_avail, tc, out);
    layout.scratch.grid.depth_stack.exit();
    result
}

#[allow(clippy::too_many_arguments)]
fn measure_inner(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    idx: GridDefId,
    depth: usize,
    inner_avail: Size,
    tc: &TextCtx<'_>,
    out: &mut Layout,
) -> Size {
    let def = tree.grid_defs[usize::from(idx)];
    let row_tracks = &tree.grid_tracks[def.rows.range()];
    let col_tracks = &tree.grid_tracks[def.cols.range()];
    let n_rows = row_tracks.len();
    let n_cols = col_tracks.len();
    let row_gap = def.row_gap;
    let col_gap = def.col_gap;
    let scratch = layout.scratch.grid.depth_stack.at(depth);
    scratch.col.reset(n_cols);
    scratch.row.reset(n_rows);
    reset_hugs_for(layout, idx);

    if n_rows == 0 || n_cols == 0 {
        // Recurse with `Size::ZERO` so leaves still take the Leaf measure arm
        // and push `ShapedText` entries for every `ShapeRecord::Text` —
        // the cascade walks shape records and asserts a matching shaped
        // entry per text record, regardless of whether the rect is zero.
        // Skipping the walk breaks `text_reshape_skipped_when_unchanged`.
        for c in tree.children(node).map(|c| c.id) {
            layout.measure(tree, c, Size::ZERO, tc, out);
        }
        return Size::ZERO;
    }

    // Phase 1: query column intrinsics for Hug-column span-1 cells.
    // Resolves the col axis without measuring children — the whole
    // point is to give cells a committed column width before they
    // shape (otherwise wrap text in Hug cols would always shape at INF
    // and never wrap).
    // Skip the span-1 child walk entirely when no column is content-
    // floor-sensitive. Hug cols need both `min` (constraint solver lo)
    // and `max` (constraint solver hi); Fill cols only need `min` so
    // the Phase 3 distributor floors them at their cells' min-content
    // (matching Stack's freeze-loop floor, prevents collapse below a
    // rigid descendant like a Fixed widget or unbreakable word).
    // Fixed cols read neither.
    let any_content_floor_col = col_tracks
        .iter()
        .any(|t| t.size.is_hug() || t.size.fill_weight().is_some());
    if any_content_floor_col {
        for c in tree.active_children(node) {
            let cell = tree.bounds(c).grid;
            if cell.col_span != 1 {
                continue;
            }
            let t = &col_tracks[cell.col as usize];
            let i = cell.col as usize;
            if t.size.is_hug() {
                let range = layout.intrinsic_range(tree, c, Axis::X, tc);
                let (cols_min, cols_max) = layout.scratch.grid.hugs.slice_mut_pair(idx, Axis::X);
                cols_min[i] = cols_min[i].max(range.min);
                cols_max[i] = cols_max[i].max(range.max);
            } else if t.size.fill_weight().is_some() {
                let min = layout.intrinsic(tree, c, Axis::X, LenReq::MinContent, tc);
                let cols_min = layout
                    .scratch
                    .grid
                    .hugs
                    .slice_mut(idx, Axis::X, HugKind::Min);
                cols_min[i] = cols_min[i].max(min);
            }
        }
    }

    // Resolve column widths now (Fixed + Hug + Fill). Gives every cell a
    // committed `available.w` before it measures.
    //
    // For Fill cols specifically, whether cells should see the resolved
    // Fill width or `INFINITY` depends on the *grid's* sizing on this
    // axis. A Hug grid's final slot is still unknown here: its desired
    // width is resolved later from `sum_non_fill` plus the intrinsic
    // floor that includes Fill content. Cells therefore stay unbounded
    // on Fill columns so row heights cannot commit to the unrelated
    // measure-time available width. For non-Hug grids (`Fill` / `Fixed`),
    // measure's `inner_avail.w` matches arrange's `inner.w`, so Fill cols
    // at measure time give cells the same width they'll get at arrange —
    // wrap text shapes correctly.
    let grid_sizing = tree.records.layout()[node.idx()].size;
    let grid_sizing_w = grid_sizing.w();
    let grid_sizing_h = grid_sizing.h();
    {
        let GridContext {
            depth_stack, hugs, ..
        } = &mut layout.scratch.grid;
        let s = depth_stack.at(depth);
        resolve_axis(
            &mut s.col,
            col_tracks,
            hugs.slice(idx, Axis::X, HugKind::Min),
            hugs.slice(idx, Axis::X, HugKind::Max),
            inner_avail.w,
            col_gap,
            !grid_sizing_w.is_hug(),
        );
        // Stash col sizes for arrange's reuse path (skips a redundant
        // `resolve_axis` when the arrange-time slot matches `inner_avail.w`).
        hugs.record_resolution(idx, Axis::X, inner_avail.w, &s.col.sizes);
        // Resolve Fixed rows once before the per-cell loop — values are
        // constant per GridDef and `resolve_fixed` is idempotent, so
        // calling it inside the loop just re-set the same slots.
        resolve_fixed(&mut s.row, row_tracks);
    }

    // Phase 2: measure cells with resolved col widths. Rows are still
    // unresolved (only Fixed is known); cells get INF on row axis as
    // before. Cell desired heights feed row Hug resolution next.
    // Collapsed children skipped — `LayoutScratch::resize_for` already
    // zeroed `desired` for the whole frame, and arrange anchors
    // collapsed subtrees via `zero_subtree`.
    for c in tree.active_children(node) {
        let cell = tree.bounds(c).grid;

        let avail = {
            let s = layout.scratch.grid.depth_stack.at(depth);
            // `known_span_size` returns INFINITY if any spanned col is
            // unresolved. After `resolve_axis` ran above, Fixed and Hug
            // cols are marked resolved; Fill cols intentionally stay
            // unresolved so cells in them get INF here — Fill stays
            // finalized at arrange time. Without this, cells in Fill
            // cols would measure at a different width than they're
            // arranged at, and that discrepancy commits row heights
            // based on a width arrange doesn't honor.
            let avail_w = known_span_size(
                &s.col.sizes,
                &s.col.resolved,
                cell.track_span(Axis::X),
                col_gap,
            );
            // Rows: only Fixed is known yet; Hug and Fill are unresolved
            // → INF (WPF intrinsic trick), as before.
            let avail_h = known_span_size(
                &s.row.sizes,
                &s.row.resolved,
                cell.track_span(Axis::Y),
                row_gap,
            );
            Size::new(avail_w, avail_h)
        };

        let d = layout.measure(tree, c, avail, tc, out);

        // Row Hug accumulates from cell's measured height. Row min-content
        // could come from a Y intrinsic query, but it'd be the single-line
        // height — the wrapped height (in `desired.h`) is what actually
        // matters. For Fill rows, the same `d.h` is the min-content
        // floor used by `resolve_axis` Phase 3 to prevent collapse
        // below a rigid descendant (matches Stack's freeze-loop floor).
        // Skip multi-row spans: their height is distributed across rows,
        // not attributable to one row.
        if cell.row_span == 1 {
            let hugs = &mut layout.scratch.grid.hugs;
            let row = cell.row as usize;
            let sizing = row_tracks[row].size;
            if sizing.is_hug() {
                let hug_max = hugs.slice_mut(idx, Axis::Y, HugKind::Max);
                hug_max[row] = hug_max[row].max(d.h);
            } else if sizing.fill_weight().is_some() {
                let hug_min = hugs.slice_mut(idx, Axis::Y, HugKind::Min);
                hug_min[row] = hug_min[row].max(d.h);
            }
        }
    }

    // Resolve row heights. Shares `resolve_axis` with the col pass, so
    // Phase 4 still runs — but the row `resolved` marking is inert here:
    // its only reader (`known_span_size` in Phase 2) has already run,
    // `resolved` is not part of the persisted arrange state (only `sizes`
    // + `total` are), and arrange's re-resolve rebuilds it from scratch.
    // Only the resolved `sizes` recorded below matter past this point.
    {
        let GridContext {
            depth_stack, hugs, ..
        } = &mut layout.scratch.grid;
        let s = depth_stack.at(depth);
        resolve_axis(
            &mut s.row,
            row_tracks,
            hugs.slice(idx, Axis::Y, HugKind::Min),
            hugs.slice(idx, Axis::Y, HugKind::Max),
            inner_avail.h,
            row_gap,
            !grid_sizing_h.is_hug(),
        );
        hugs.record_resolution(idx, Axis::Y, inner_avail.h, &s.row.sizes);
    }

    // Returned content size: sum of non-Fill track sizes + gaps. Fill
    // claims leftover at arrange; `resolve_sizing` separately floors this
    // raw answer at the Grid intrinsic, which includes Fill content.
    let s = layout.scratch.grid.depth_stack.at(depth);
    let total_w =
        sum_non_fill(col_tracks, &s.col.sizes) + col_gap * n_cols.saturating_sub(1) as f32;
    let total_h =
        sum_non_fill(row_tracks, &s.row.sizes) + row_gap * n_rows.saturating_sub(1) as f32;
    Size::new(total_w, total_h)
}

fn sum_non_fill(tracks: &[Track], sizes: &[f32]) -> f32 {
    tracks
        .iter()
        .zip(sizes.iter())
        .map(|(t, &s)| {
            if t.size.fill_weight().is_some() {
                0.0
            } else {
                s
            }
        })
        .sum()
}

fn resolve_fixed(a: &mut AxisScratch, tracks: &[Track]) {
    for (i, t) in tracks.iter().enumerate() {
        if let Some(value) = t.size.fixed_value() {
            a.sizes[i] = value.clamp(t.min, t.max);
            a.resolved.insert(i);
        }
    }
}

pub(crate) fn arrange(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    inner: Rect,
    idx: GridDefId,
    out: &mut Layout,
) {
    let depth = layout.scratch.grid.depth_stack.enter();
    arrange_inner(layout, tree, node, inner, idx, depth, out);
    layout.scratch.grid.depth_stack.exit();
}

fn arrange_inner(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    inner: Rect,
    idx: GridDefId,
    depth: usize,
    out: &mut Layout,
) {
    let def = tree.grid_defs[usize::from(idx)];
    let row_tracks = &tree.grid_tracks[def.rows.range()];
    let col_tracks = &tree.grid_tracks[def.cols.range()];
    let n_rows = row_tracks.len();
    let n_cols = col_tracks.len();
    let row_gap = def.row_gap;
    let col_gap = def.col_gap;
    let scratch = layout.scratch.grid.depth_stack.at(depth);
    scratch.col.reset(n_cols);
    scratch.row.reset(n_rows);

    if n_rows == 0 || n_cols == 0 {
        for c in tree.children(node).map(|c| c.id) {
            zero_subtree(layout, tree, c, inner.min, out);
        }
        return;
    }

    // Resolve track sizes (Fixed + Hug + Fill) and compute offsets.
    // Fast path: when measure already resolved this axis against the
    // same `total` (recorded in `hugs.total_used`), copy the persisted
    // sizes instead of re-running the constraint solver. The path is
    // safe when:
    //   - measure ran for this grid this frame (`total_used != 0` —
    //     cache-hit-ancestor descendants have 0 since `reset_for`
    //     zeros them and we never wrote);
    //   - arrange's `inner.size.X` matches measure's `inner_avail.X`
    //     (no WPF Stretch grow on this axis since measure committed).
    // The `track_offsets` cumulative-sum is cheap relative to
    // `resolve_axis` (O(n_tracks), no constraint solving) so we re-run
    // it unconditionally — keeps the offsets in sync regardless of
    // which path produced `sizes`.
    {
        let GridContext {
            depth_stack, hugs, ..
        } = &mut layout.scratch.grid;
        let s = depth_stack.at(depth);
        resolve_or_reuse(
            &mut s.col,
            col_tracks,
            hugs,
            idx,
            Axis::X,
            inner.size.w,
            col_gap,
        );
        resolve_or_reuse(
            &mut s.row,
            row_tracks,
            hugs,
            idx,
            Axis::Y,
            inner.size.h,
            row_gap,
        );
        track_offsets(&s.col.sizes, col_gap, &mut s.col.offsets);
        track_offsets(&s.row.sizes, row_gap, &mut s.row.offsets);
    }

    let parent_child_align = tree.panel(node).child_align;
    let layouts = tree.records.layout();
    let self_outer = out[layout.active_layer].rect[node.idx()].size;
    for child in tree.children(node) {
        let c = child.id;
        if child.visibility.is_collapsed() {
            zero_subtree(layout, tree, c, inner.min, out);
            continue;
        }
        let i = c.idx();
        let s_node = layouts[i];
        let bounds = tree.bounds(c);
        let cell = bounds.grid;
        let d = layout.scratch.desired[i];

        let (slot_x, slot_y, slot_w, slot_h) = {
            let s = layout.scratch.grid.depth_stack.at(depth);
            let slot_x = s.col.offsets[cell.col as usize];
            let slot_y = s.row.offsets[cell.row as usize];
            let slot_w = span_size(&s.col.sizes, cell.track_span(Axis::X), col_gap);
            let slot_h = span_size(&s.row.sizes, cell.track_span(Axis::Y), row_gap);
            (slot_x, slot_y, slot_w, slot_h)
        };

        // Grid's default alignment stretches non-Fixed children to their cell.
        let AxisAlignPair { h, v } = resolved_axis_align(&s_node, parent_child_align);
        let x = arrange_axis(Axis::X, h.or_stretch_if_auto(), &s_node, bounds, d, slot_w);
        let y = arrange_axis(Axis::Y, v.or_stretch_if_auto(), &s_node, bounds, d, slot_h);
        let child_rect = Rect {
            min: inner.min + Vec2::new(slot_x + x.offset, slot_y + y.offset),
            size: Size::new(x.size, y.size),
        };
        layout.arrange(tree, c, self_outer, child_rect, out);
    }
}

/// Sum of spanned tracks' resolved sizes, or `∞` if any spanned track is not
/// yet resolved (Hug / Fill at measure time). Internal gaps contribute only
/// when the whole span is known. Infinity makes the child fall back to its
/// intrinsic size on that axis (the WPF trick).
fn known_span_size(sizes: &[f32], resolved: &FixedBitSet, span: Span, gap: f32) -> f32 {
    // Cells are range-checked against the parent's track counts at record
    // time (`Tree::check_grid_cell`), so `span.range()` is always in
    // bounds here — index directly.
    let mut sum = 0.0;
    for i in span.range() {
        if !resolved.contains(i) {
            return f32::INFINITY;
        }
        sum += sizes[i];
    }
    sum + gap * span.len.saturating_sub(1) as f32
}

fn track_offsets(sizes: &[f32], gap: f32, out: &mut [f32]) {
    debug_assert_eq!(sizes.len(), out.len());
    let mut acc = 0.0f32;
    for (i, &s) in sizes.iter().enumerate() {
        out[i] = acc;
        acc += s;
        if i + 1 < sizes.len() {
            acc += gap;
        }
    }
}

fn span_size(sizes: &[f32], span: Span, gap: f32) -> f32 {
    // In-bounds by the same record-time cell range check as
    // `known_span_size`.
    let r = span.range();
    let n = r.len();
    let mut total: f32 = sizes[r].iter().sum();
    if n > 1 {
        total += gap * (n - 1) as f32;
    }
    total
}

/// Either copy persisted resolved sizes from the last measure or
/// re-run [`resolve_axis`] — whichever is sound for arrange's
/// `(grid, axis, slot)`. See the call-site comment for the
/// soundness conditions; the predicate here is just the boolean
/// version of those.
fn resolve_or_reuse(
    a: &mut AxisScratch,
    tracks: &[Track],
    hugs: &mut GridHugStore,
    idx: GridDefId,
    axis: Axis,
    total: f32,
    gap: f32,
) {
    let recorded_total = hugs.total_used(idx, axis);
    let can_reuse = recorded_total != 0.0 && recorded_total == total;
    if can_reuse {
        a.sizes.copy_from_slice(hugs.sizes_slice(idx, axis));
        return;
    }
    resolve_axis(
        a,
        tracks,
        hugs.slice(idx, axis, HugKind::Min),
        hugs.slice(idx, axis, HugKind::Max),
        total,
        gap,
        false,
    );
}

#[inline]
fn content_floor(track: &Track, min_content: f32) -> f32 {
    min_content.max(track.min).min(track.max)
}

/// Resolve track sizes on one axis into `a.sizes` for a grid with
/// `total` available main-axis length and `gap` between adjacent tracks.
/// `commit_fill` marks Fill tracks resolved when measure knows its
/// available extent is the final arrange extent.
///
/// **Algorithm**, four phases:
/// 1. **Fixed:** clamp `Sizing::fixed(v)` to `[Track.min, Track.max]`,
///    consume from available.
/// 2. **Hug:** constraint-solve each track's content range, with both
///    its min-content floor and preferred size capped by `Track.max`,
///    against the remaining-after-Fixed:
///    - If `sum_hug_max <= remaining`: each Hug at max.
///    - If `sum_hug_min >= remaining`: each Hug at min, grid overflows.
///    - Else: each Hug starts at min, slack distributed proportional to
///      `(max - min)`.
/// 3. **Fill:** original constraint-by-exclusion algorithm — Fill tracks
///    distribute leftover proportional to weight; any Fill whose share
///    falls outside its capped min-content floor and `Track.max` clamps
///    and exits the pool; remaining Fills rebalance.
/// 4. **Mark Fill resolved (commit):** by default Fill tracks stay
///    unresolved so cells in Fill cols see `INF` via `known_span_size`
///    during measure (preserves "Fill is finalized at arrange"). When
///    the grid itself is non-Hug on this axis with a finite slot, the
///    measure-time `total` matches arrange's, so Fill tracks can be
///    committed up-front and cells measure at the resolved width — wrap
///    text shapes correctly. Hug grids must keep Fill unresolved (their
///    arrange slot is unknown here). Arrange passes `false` because it
///    consumes only sizes and offsets, never the resolved flags.
fn resolve_axis(
    a: &mut AxisScratch,
    tracks: &[Track],
    hug_min: &[f32],
    hug_max: &[f32],
    total: f32,
    gap: f32,
    commit_fill: bool,
) {
    let n = tracks.len();
    a.sizes.fill(0.0);
    // Reset resolved flags. Fixed + Hug get marked resolved as they're
    // computed. Fill stays unresolved so cells in Fill cols see INF as
    // their available width via `known_span_size`, preserving the old
    // "Fill is finalized at arrange" behavior. Without this, cells in
    // Fill cols would measure with measure-time Fill leftover (a
    // finite value), then arrange might assign a different
    // intrinsic-floor-driven slot to a Hug grid and the cell
    // rect/shape would disagree.
    a.resolved.clear();
    let total_gap = gap * n.saturating_sub(1) as f32;

    // Phase 1: Fixed.
    let mut consumed = total_gap;
    for (i, t) in tracks.iter().enumerate() {
        if let Some(value) = t.size.fixed_value() {
            a.sizes[i] = value.clamp(t.min, t.max);
            a.resolved.insert(i);
            consumed += a.sizes[i];
        }
    }

    // Phase 2: Hug, constraint-solved against remaining-after-Fixed.
    // Single pass: snapshot each Hug track's clamped `(lo, hi)` once,
    // pick the distribution rule from the totals, then write sizes.
    a.hug_bounds.clear();
    let mut hug_min_sum = 0.0_f32;
    let mut hug_max_sum = 0.0_f32;
    for (i, t) in tracks.iter().enumerate() {
        if t.size.is_hug() {
            let lo = content_floor(t, hug_min[i]);
            let hi = hug_max[i].max(lo).min(t.max);
            hug_min_sum += lo;
            hug_max_sum += hi;
            a.hug_bounds.push(HugBound { idx: i, lo, hi });
        }
    }

    if !a.hug_bounds.is_empty() {
        let remaining_after_fixed = (total - consumed).max(0.0);
        // Pick distribution mode once. `unconstrained` covers infinite
        // total (Hug parent) and the "every Hug fits at max" case;
        // `cramped` covers "even at min the Hugs overflow"; otherwise
        // distribute slack proportional to per-track `(hi - lo)`.
        let unconstrained = total.is_infinite() || hug_max_sum <= remaining_after_fixed;
        let cramped = !unconstrained && hug_min_sum >= remaining_after_fixed;
        let slack = remaining_after_fixed - hug_min_sum;
        let total_range = hug_max_sum - hug_min_sum;

        for &HugBound { idx, lo, hi } in &a.hug_bounds {
            let v = if unconstrained {
                hi
            } else if cramped {
                lo
            } else if total_range > 0.0 {
                (lo + slack * (hi - lo) / total_range).min(hi)
            } else {
                lo
            };
            a.sizes[idx] = v;
            a.resolved.insert(idx);
            consumed += v;
        }
    }

    // Phase 3: Fill — constraint-by-exclusion. Fills get the leftover
    // after Fixed + Hug, distributed by weight; any Fill whose share
    // falls outside `[content_floor, Track.max]` clamps and exits the
    // pool, then remaining Fills rebalance. Capping the min-content floor
    // keeps the interval ordered when a rigid descendant exceeds the
    // explicit track cap. This mirrors the `[floor, cap]` freeze in
    // `stack::freeze_distribute` (kept in sync by hand; see its doc for
    // why the two aren't physically merged).
    let mut remaining = (total - consumed).max(0.0);
    a.flexible.clear();
    let mut flexible_weight = 0.0_f64;
    for (i, t) in tracks.iter().enumerate() {
        if let Some(weight) = t.size.fill_weight() {
            a.flexible.push(i);
            flexible_weight += f64::from(weight);
        }
    }

    // Clamp-and-rebalance loop. Each iteration looks for one Fill whose
    // proportional share violates `[lo, Track.max]`; if it exists,
    // clamp it, remove it from the pool, and rerun. When every
    // remaining Fill's share is in-range, commit them at that share and
    // exit. Converges in ≤ N iterations (each clamp removes one).
    while !a.flexible.is_empty() && flexible_weight > 0.0 {
        let clamp_idx = a.flexible.iter().position(|&i| {
            let t = &tracks[i];
            let weight = t.size.fill_weight().unwrap();
            let candidate = weighted_share(remaining, weight, flexible_weight);
            let lo = content_floor(t, hug_min[i]);
            candidate < lo || candidate > t.max
        });
        match clamp_idx {
            Some(k) => {
                let i = a.flexible[k];
                let t = &tracks[i];
                let weight = t.size.fill_weight().unwrap();
                let candidate = weighted_share(remaining, weight, flexible_weight);
                let lo = content_floor(t, hug_min[i]);
                let clamped = candidate.clamp(lo, t.max);
                a.sizes[i] = clamped;
                remaining = (remaining - clamped).max(0.0);
                flexible_weight -= f64::from(weight);
                a.flexible.swap_remove(k);
            }
            None => {
                for &i in a.flexible.iter() {
                    let weight = tracks[i].size.fill_weight().unwrap();
                    a.sizes[i] = weighted_share(remaining, weight, flexible_weight);
                }
                break;
            }
        }
    }

    // Phase 4: commit Fill tracks as resolved when the grid's own axis
    // sizing guarantees measure-time `total` matches arrange-time slot.
    if commit_fill && total.is_finite() {
        for (i, t) in tracks.iter().enumerate() {
            if t.size.fill_weight().is_some() {
                a.resolved.insert(i);
            }
        }
    }
}

/// Intrinsic size of a Grid: per-track contribution aggregated from
/// span-1 cells, summed across tracks plus gaps. Answers "what would
/// the Grid prefer to be on this axis?" so callers can read it without
/// running `measure`.
///
/// Per-track contribution mirrors `Track`'s `Sizing` interpretation:
/// - `Fixed(v)`: contributes `v` clamped to `[Track.min, Track.max]`.
/// - `Hug`: starts at `Track.min`, grown by span-1 cells' intrinsic on
///   the same axis, clamped to `[Track.min, Track.max]`.
/// - `Fill(_)`: same content floor as Hug; weight is ignored until
///   distribution.
///
/// Span > 1 cells are excluded (matches existing `measure` and the
/// commitment in `src/layout/intrinsic.md`).
pub(crate) fn intrinsic<const RANGE: bool>(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    idx: GridDefId,
    axis: Axis,
    query: IntrinsicQuery<RANGE>,
    tc: &TextCtx<'_>,
) -> IntrinsicRange {
    let def = tree.grid_defs[usize::from(idx)];
    // An empty dimension means no cells, so the grid measures to
    // `Size::ZERO` (see `measure_inner`); its intrinsic must match on
    // *both* axes — a declared `Fixed` track on the non-empty axis
    // contributes nothing when there's nothing to place in it.
    if def.cols.len == 0 || def.rows.len == 0 {
        return IntrinsicRange::ZERO;
    }
    let (track_span, gap) = match axis {
        Axis::X => (def.cols, def.col_gap),
        Axis::Y => (def.rows, def.row_gap),
    };
    let tracks = &tree.grid_tracks[track_span.range()];
    let n_tracks = tracks.len();

    let wants_min = query.includes(LenReq::MinContent);
    let wants_max = query.includes(LenReq::MaxContent);
    let base = layout.scratch.grid.track_aggregator.len();
    let min_base = base;
    let max_base = base + usize::from(wants_min) * n_tracks;
    let slot_count = (usize::from(wants_min) + usize::from(wants_max)) * n_tracks;
    layout
        .scratch
        .grid
        .track_aggregator
        .resize(base + slot_count, 0.0);
    for (i, t) in tracks.iter().enumerate() {
        let initial = t
            .size
            .fixed_value()
            .map_or(t.min, |value| value.clamp(t.min, t.max));
        if wants_min {
            layout.scratch.grid.track_aggregator[min_base + i] = initial;
        }
        if wants_max {
            layout.scratch.grid.track_aggregator[max_base + i] = initial;
        }
    }

    for c in tree.active_children(node) {
        let cell_span = tree.bounds(c).grid.track_span(axis);
        if cell_span.len != 1 {
            continue;
        }
        let track_idx = cell_span.start as usize;
        let t = &tracks[track_idx];
        if t.size.fixed_value().is_some() {
            continue;
        }
        let child = query.child(layout, tree, c, axis, tc);
        if wants_min {
            let slot = &mut layout.scratch.grid.track_aggregator[min_base + track_idx];
            *slot = slot.max(content_floor(t, child.min));
        }
        if wants_max {
            let slot = &mut layout.scratch.grid.track_aggregator[max_base + track_idx];
            *slot = slot.max(content_floor(t, child.max));
        }
    }

    let gaps = gap * n_tracks.saturating_sub(1) as f32;
    let mut range = IntrinsicRange::ZERO;
    if wants_min {
        range.min = layout.scratch.grid.track_aggregator[min_base..min_base + n_tracks]
            .iter()
            .sum::<f32>()
            + gaps;
    }
    if wants_max {
        range.max = layout.scratch.grid.track_aggregator[max_base..max_base + n_tracks]
            .iter()
            .sum::<f32>()
            + gaps;
    }
    layout.scratch.grid.track_aggregator.truncate(base);
    range
}

#[cfg(test)]
mod tests;
