use super::support::{AutoBias, place_two_axis, zero_subtree};
use super::{Axis, LayoutEngine, LenReq};
use crate::element::LayoutMode;
use crate::primitives::{rect::Rect, size::Size, sizing::Sizing, span::Span, track::Track};
use crate::text::TextMeasurer;
use crate::tree::{Child, NodeId, Tree};
use glam::Vec2;
use std::ops::Range;
use std::rc::Rc;

#[derive(Copy, Clone)]
enum HugKind {
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

/// Track counts + gaps for a grid, returned by
/// [`prepare_axis_scratch_at`] after it adopts the def into per-depth
/// scratch. Carries no behavior; exists so call-site destructuring
/// names the scalars instead of relying on tuple position.
struct GridShape {
    n_rows: usize,
    n_cols: usize,
    row_gap: f32,
    col_gap: f32,
}

/// Adopt the `GridDef` at `idx` into the per-depth scratch slot:
/// refcount-clone the track `Rc<[Track]>`s, read gaps, reset the
/// per-axis `AxisScratch`. Returns the grid's track counts + gaps for
/// the caller. Hug arrays live on `GridHugStore` (durable across the
/// layout pass) and are read/written separately via
/// `hugs.min/max(idx, axis)`.
fn prepare_axis_scratch_at(
    layout: &mut LayoutEngine,
    tree: &Tree,
    idx: u16,
    depth: usize,
) -> GridShape {
    let def = &tree.grid.defs[idx as usize];
    let n_rows = def.rows.len();
    let n_cols = def.cols.len();
    let rows = def.rows.clone();
    let cols = def.cols.clone();
    let row_gap = def.row_gap;
    let col_gap = def.col_gap;
    let s = layout.scratch.grid.depth_stack.at(depth);
    s.col.reset(cols);
    s.row.reset(rows);
    GridShape {
        n_rows,
        n_cols,
        row_gap,
        col_gap,
    }
}

/// Per-axis scratch for one nesting depth. `tracks` shares the user's
/// `Rc<[Track]>` (refcount-only clone — no copy). `flexible` is a transient
/// list used only inside `resolve_axis`; it lives on the per-axis struct so
/// its capacity is retained across frames.
///
/// Per-track content-driven `[min, max]` Hug ranges live in
/// `GridHugStore` (durable across the whole layout pass); they're passed
/// into `resolve_axis` as slices alongside this scratch.
pub(crate) struct AxisScratch {
    pub(crate) tracks: Rc<[Track]>,
    pub(crate) sizes: Vec<f32>,
    pub(crate) resolved: Vec<bool>,
    pub(crate) offsets: Vec<f32>,
    flexible: Vec<usize>,
}

impl Default for AxisScratch {
    fn default() -> Self {
        Self {
            tracks: Rc::from([]),
            sizes: Vec::new(),
            resolved: Vec::new(),
            offsets: Vec::new(),
            flexible: Vec::new(),
        }
    }
}

impl AxisScratch {
    /// Adopt the user's track `Rc<[Track]>` (refcount-only) and (re)size the
    /// per-track arrays to match. All arrays are zeroed; `resolved` is reset
    /// to `false`. Capacity on the `Vec`s is retained across frames.
    fn reset(&mut self, tracks: Rc<[Track]>) {
        let n = tracks.len();
        self.tracks = tracks;
        self.sizes.clear();
        self.sizes.resize(n, 0.0);
        self.resolved.clear();
        self.resolved.resize(n, false);
        self.offsets.clear();
        self.offsets.resize(n, 0.0);
    }
}

/// Per-frame scratch for `Grid` layout. Capacity is retained across frames; a
/// `Vec<GridScratch>` indexed by nesting depth lets nested grids each have
/// their own slot. Pushed on first descent to a new depth.
#[derive(Default)]
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
#[derive(Default)]
pub(crate) struct GridContext {
    pub(crate) depth_stack: GridDepthStack,
    pub(crate) hugs: GridHugStore,
    pub(crate) track_aggregator: Vec<f32>,
}

/// Nesting stack of per-depth grid scratch. One `GridScratch` slot per
/// active `LayoutMode::Grid` ancestor. `depth` is the next free slot.
#[derive(Default)]
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
        debug_assert!(self.depth > 0);
        self.depth -= 1;
    }

    fn at(&mut self, depth: usize) -> &mut GridScratch {
        &mut self.scratch[depth]
    }
}

/// Flat per-track hug-size pool with one `(rows, cols)` slot per recorded
/// `GridDef`. Carries both `max` (max-content) and `min` (min-content) per
/// track so the Hug-track constraint solver can range-distribute Hug
/// tracks. Measure pass writes; arrange pass reads. Per-depth scratch in
/// `depth_stack` gets clobbered by sibling grids before arrange runs, so
/// the pool persists for the whole layout pass instead.
///
/// `reset_for` zeroes every slot at the top of each pass — load-bearing,
/// because `record_hug` and the Phase 1 column loop both accumulate via
/// `slot[i] = slot[i].max(...)` and assume a 0.0 starting state.
/// Capacity retained across frames.
#[derive(Default)]
pub(crate) struct GridHugStore {
    max_pool: Vec<f32>,
    min_pool: Vec<f32>,
    slots: Vec<GridHugSlot>,
}

#[derive(Clone, Copy)]
struct GridHugSlot {
    rows: Span,
    cols: Span,
}

impl GridHugStore {
    pub(crate) fn reset_for(&mut self, tree: &Tree) {
        self.max_pool.clear();
        self.min_pool.clear();
        self.slots.clear();
        for def in &tree.grid.defs {
            let rows = self.alloc(def.rows.len());
            let cols = self.alloc(def.cols.len());
            self.slots.push(GridHugSlot { rows, cols });
        }
    }

    fn alloc(&mut self, n: usize) -> Span {
        let start = self.max_pool.len() as u32;
        self.max_pool.resize(start as usize + n, 0.0);
        self.min_pool.resize(start as usize + n, 0.0);
        Span::new(start, n as u32)
    }

    fn axis_slice(&self, idx: u16, axis: Axis) -> Range<usize> {
        let slot = self.slots[idx as usize];
        let s = match axis {
            Axis::X => slot.cols,
            Axis::Y => slot.rows,
        };
        s.range()
    }

    pub(crate) fn max(&self, idx: u16, axis: Axis) -> &[f32] {
        &self.max_pool[self.axis_slice(idx, axis)]
    }
    pub(crate) fn min(&self, idx: u16, axis: Axis) -> &[f32] {
        &self.min_pool[self.axis_slice(idx, axis)]
    }
    pub(crate) fn max_mut(&mut self, idx: u16, axis: Axis) -> &mut [f32] {
        let r = self.axis_slice(idx, axis);
        &mut self.max_pool[r]
    }
    pub(crate) fn min_mut(&mut self, idx: u16, axis: Axis) -> &mut [f32] {
        let r = self.axis_slice(idx, axis);
        &mut self.min_pool[r]
    }

    /// Pack per-grid hug arrays for every `LayoutMode::Grid` descendant
    /// in `subtree` (pre-order node-index range) into `out`. Used by
    /// the cross-frame measure cache: when a subtree is snapshotted,
    /// arrange's hug state must be saved so a later cache hit at any
    /// ancestor can restore it via [`Self::restore_subtree`]. Order is
    /// dictated by [`HUG_ORDER`] per Grid, in pre-order.
    pub(crate) fn snapshot_subtree(&self, tree: &Tree, subtree: Range<usize>, out: &mut Vec<f32>) {
        for i in subtree {
            if let LayoutMode::Grid(idx) = tree.layout[i].mode {
                for (axis, kind) in HUG_ORDER {
                    out.extend_from_slice(self.axis_array(idx, axis, kind));
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
        let mut pos = 0usize;
        for i in subtree {
            if let LayoutMode::Grid(idx) = tree.layout[i].mode {
                for (axis, kind) in HUG_ORDER {
                    let dst = self.axis_array_mut(idx, axis, kind);
                    let n = dst.len();
                    dst.copy_from_slice(&hugs[pos..pos + n]);
                    pos += n;
                }
            }
        }
        assert_eq!(
            pos,
            hugs.len(),
            "snapshot hug slice length disagrees with current subtree's grid descendants \
             (cache key let through a structural change?)",
        );
    }

    fn axis_array(&self, idx: u16, axis: Axis, kind: HugKind) -> &[f32] {
        match kind {
            HugKind::Max => self.max(idx, axis),
            HugKind::Min => self.min(idx, axis),
        }
    }

    fn axis_array_mut(&mut self, idx: u16, axis: Axis, kind: HugKind) -> &mut [f32] {
        match kind {
            HugKind::Max => self.max_mut(idx, axis),
            HugKind::Min => self.min_mut(idx, axis),
        }
    }
}

/// WPF-style grid measure. Resolves Fixed tracks, walks children once feeding
/// each `Σ spanned-track sizes` (or `∞` if any spanned track is unresolved —
/// the WPF infinity trick → child reports intrinsic), then resolves Hug
/// tracks from span-1 children's desired sizes. Star tracks contribute 0 to
/// the grid's content size — final star sizes only resolve in arrange. See
/// `docs/grid.md`.
///
/// Per-depth scratch (`AxisScratch` columns) lives in `grid.depth_stack`
/// and gets clobbered by sibling grids between this measure and the
/// matching arrange. Hug sizes therefore live in `grid.hugs`
/// (`GridHugStore`), keyed by `GridDef` index, durable for the whole
/// layout pass. Both are heap-resident and capacity-retained across
/// frames; no fixed track-count limit.
pub(crate) fn measure(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    idx: u16,
    inner_avail: Size,
    text: &mut TextMeasurer,
) -> Size {
    let depth = layout.scratch.grid.depth_stack.enter();
    let result = measure_inner(layout, tree, node, idx, depth, inner_avail, text);
    layout.scratch.grid.depth_stack.exit();
    result
}

fn measure_inner(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    idx: u16,
    depth: usize,
    inner_avail: Size,
    text: &mut TextMeasurer,
) -> Size {
    let GridShape {
        n_rows,
        n_cols,
        row_gap,
        col_gap,
    } = prepare_axis_scratch_at(layout, tree, idx, depth);

    if n_rows == 0 || n_cols == 0 {
        // Still measure children so their `desired` is set.
        for c in tree.children(node) {
            layout.measure(tree, c, Size::ZERO, text);
        }
        return Size::ZERO;
    }

    // Phase 1: query column intrinsics for Hug-column span-1 cells.
    // Resolves the col axis without measuring children — the whole
    // point is to give cells a committed column width before they
    // shape (otherwise wrap text in Hug cols would always shape at INF
    // and never wrap).
    let col_tracks = tracks_at(layout, depth, Axis::X);
    for c in tree.children_active(node) {
        let cell = tree.read_extras(c).grid;
        if cell.col_span != 1 {
            continue;
        }
        let t = &col_tracks[cell.col as usize];
        if !matches!(t.size, Sizing::Hug) {
            continue;
        }
        let bounds = layout.intrinsic_pair(tree, c, Axis::X, text);
        let i = cell.col as usize;
        let cols_min = layout.scratch.grid.hugs.min_mut(idx, Axis::X);
        cols_min[i] = cols_min[i].max(bounds.min);
        let cols_max = layout.scratch.grid.hugs.max_mut(idx, Axis::X);
        cols_max[i] = cols_max[i].max(bounds.max);
    }

    // Resolve column widths now (Fixed + Hug + Fill). Gives every cell a
    // committed `available.w` before it measures.
    //
    // For Fill cols specifically, whether cells should see the resolved
    // Fill width or `INFINITY` depends on the *grid's* sizing on this
    // axis. If the grid is `Sizing::Hug`, arrange's `inner.w` will be
    // `grid.desired.w = sum_non_fill` — Fill cols get 0 leftover at
    // arrange. Cells measured at the measure-time finite Fill width
    // would commit row heights to a width arrange doesn't honor (the
    // "rows grow on horizontal resize" surprise). For non-Hug grids
    // (`Fill` / `Fixed`), measure's `inner_avail.w` matches arrange's
    // `inner.w`, so Fill cols at measure time give cells the same
    // width they'll get at arrange — wrap text shapes correctly.
    let grid_sizing_w = tree.layout(node).size.w;
    let grid_sizing_h = tree.layout(node).size.h;
    {
        let GridContext {
            depth_stack, hugs, ..
        } = &mut layout.scratch.grid;
        let s = depth_stack.at(depth);
        resolve_axis(
            &mut s.col,
            hugs.min(idx, Axis::X),
            hugs.max(idx, Axis::X),
            inner_avail.w,
            col_gap,
            grid_sizing_w,
        );
        // Resolve Fixed rows once before the per-cell loop — values are
        // constant per GridDef and `resolve_fixed` is idempotent, so
        // calling it inside the loop just re-set the same slots.
        resolve_fixed(&mut s.row);
    }

    // Phase 2: measure cells with resolved col widths. Rows are still
    // unresolved (only Fixed is known); cells get INF on row axis as
    // before. Cell desired heights feed row Hug resolution next.
    for child in tree.children_with_state(node) {
        let c = match child {
            Child::Collapsed(c) => {
                // Still measure so the subtree's `desired` is zeroed
                // (LayoutEngine::measure short-circuits on collapsed).
                layout.measure(tree, c, Size::ZERO, text);
                continue;
            }
            Child::Active(c) => c,
        };
        let cell = tree.read_extras(c).grid;

        let avail = {
            let s = layout.scratch.grid.depth_stack.at(depth);
            // `sum_spanned_known` returns INFINITY if any spanned col is
            // unresolved. After `resolve_axis` ran above, Fixed and Hug
            // cols are marked resolved; Fill cols intentionally stay
            // unresolved so cells in them get INF here — Fill stays
            // finalized at arrange time. Without this, cells in Fill
            // cols would measure at a different width than they're
            // arranged at, and that discrepancy commits row heights
            // based on a width arrange doesn't honor.
            let avail_w = sum_spanned_known(&s.col.sizes, &s.col.resolved, cell.col, cell.col_span);
            // Rows: only Fixed is known yet; Hug and Fill are unresolved
            // → INF (WPF intrinsic trick), as before.
            let avail_h = sum_spanned_known(&s.row.sizes, &s.row.resolved, cell.row, cell.row_span);
            Size::new(avail_w, avail_h)
        };

        let d = layout.measure(tree, c, avail, text);

        // Row Hug accumulates from cell's measured height. Row min-content
        // could come from a Y intrinsic query, but it'd be the single-line
        // height — the wrapped height (in `desired.h`) is what actually
        // matters, so leave row hug_min at zero for now.
        let GridContext {
            depth_stack, hugs, ..
        } = &mut layout.scratch.grid;
        record_hug(
            &depth_stack.at(depth).row.tracks,
            hugs.max_mut(idx, Axis::Y),
            cell.row,
            cell.row_span,
            d.h,
        );
    }

    // Resolve row heights. Same Fill-marking rule as cols above —
    // mark Fill rows resolved only when the grid is non-Hug on h.
    // (Cells already measured by this point, so the resolved flag here
    // doesn't affect the current measure; it carries forward into
    // arrange's re-resolve via the persisted state.)
    {
        let GridContext {
            depth_stack, hugs, ..
        } = &mut layout.scratch.grid;
        let s = depth_stack.at(depth);
        resolve_axis(
            &mut s.row,
            hugs.min(idx, Axis::Y),
            hugs.max(idx, Axis::Y),
            inner_avail.h,
            row_gap,
            grid_sizing_h,
        );
    }

    // Returned content size: sum of non-Fill track sizes + gaps. Fill
    // tracks "want 0" in measure context — they only claim leftover at
    // arrange time. Mirrors WPF's "Star contributes 0 to content size."
    let s = layout.scratch.grid.depth_stack.at(depth);
    let total_w =
        sum_non_fill(&s.col.tracks, &s.col.sizes) + col_gap * n_cols.saturating_sub(1) as f32;
    let total_h =
        sum_non_fill(&s.row.tracks, &s.row.sizes) + row_gap * n_rows.saturating_sub(1) as f32;
    Size::new(total_w, total_h)
}

fn sum_non_fill(tracks: &[Track], sizes: &[f32]) -> f32 {
    tracks
        .iter()
        .zip(sizes.iter())
        .map(|(t, &s)| {
            if matches!(t.size, Sizing::Fill(_)) {
                0.0
            } else {
                s
            }
        })
        .sum()
}

/// Refcount-clone the per-axis tracks at `depth` so the caller can hold
/// `&[Track]` while also calling `layout.intrinsic(...)` on the same
/// engine — releasing the `&mut depth_stack[depth]` borrow that the
/// intrinsic call would conflict with. Single caller (Phase 1 column
/// loop in `measure_inner`); kept as a named helper to keep the borrow
/// dance off the hot path.
fn tracks_at(layout: &mut LayoutEngine, depth: usize, axis: Axis) -> Rc<[Track]> {
    let s = layout.scratch.grid.depth_stack.at(depth);
    match axis {
        Axis::X => s.col.tracks.clone(),
        Axis::Y => s.row.tracks.clone(),
    }
}

fn resolve_fixed(a: &mut AxisScratch) {
    for (i, t) in a.tracks.iter().enumerate() {
        if let Sizing::Fixed(v) = t.size {
            a.sizes[i] = v.clamp(t.min, t.max);
            a.resolved[i] = true;
        }
    }
}

fn record_hug(tracks: &[Track], hug_max: &mut [f32], idx: u16, span: u16, desired: f32) {
    if span != 1 {
        return;
    }
    let i = idx as usize;
    if matches!(tracks[i].size, Sizing::Hug) {
        hug_max[i] = hug_max[i].max(desired);
    }
}

pub(crate) fn arrange(layout: &mut LayoutEngine, tree: &Tree, node: NodeId, inner: Rect, idx: u16) {
    let depth = layout.scratch.grid.depth_stack.enter();
    arrange_inner(layout, tree, node, inner, idx, depth);
    layout.scratch.grid.depth_stack.exit();
}

fn arrange_inner(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    inner: Rect,
    idx: u16,
    depth: usize,
) {
    // Re-snapshot at this depth: scratch gets clobbered by sibling grids
    // between measure and arrange, so we re-read tracks/gaps from the
    // GridDef. Hug sizes are read directly from `GridHugStore`, the
    // durable record across the layout pass.
    let GridShape {
        n_rows,
        n_cols,
        row_gap,
        col_gap,
    } = prepare_axis_scratch_at(layout, tree, idx, depth);

    if n_rows == 0 || n_cols == 0 {
        for c in tree.children(node) {
            zero_subtree(layout, tree, c, inner.min);
        }
        return;
    }

    let grid_size = tree.layout(node).size;

    // Resolve track sizes (Fixed + Hug + Fill) and compute offsets.
    // Arrange ignores `resolved` flags but `resolve_axis` requires
    // `grid_sizing` for its Phase-4 commit (no-op on arrange's read
    // path; passing the real value keeps the call self-consistent).
    {
        let GridContext {
            depth_stack, hugs, ..
        } = &mut layout.scratch.grid;
        let s = depth_stack.at(depth);
        resolve_axis(
            &mut s.col,
            hugs.min(idx, Axis::X),
            hugs.max(idx, Axis::X),
            inner.size.w,
            col_gap,
            grid_size.w,
        );
        resolve_axis(
            &mut s.row,
            hugs.min(idx, Axis::Y),
            hugs.max(idx, Axis::Y),
            inner.size.h,
            row_gap,
            grid_size.h,
        );
        track_offsets(&s.col.sizes, col_gap, &mut s.col.offsets);
        track_offsets(&s.row.sizes, row_gap, &mut s.row.offsets);
    }

    let parent_child_align = tree.read_extras(node).child_align;
    for child in tree.children_with_state(node) {
        let c = match child {
            Child::Collapsed(c) => {
                zero_subtree(layout, tree, c, inner.min);
                continue;
            }
            Child::Active(c) => c,
        };
        let s_node = *tree.layout(c);
        let cell = tree.read_extras(c).grid;
        let d = layout.scratch.desired[c.index()];

        let (slot_x, slot_y, slot_w, slot_h) = {
            let s = layout.scratch.grid.depth_stack.at(depth);
            let slot_x = s.col.offsets[cell.col as usize];
            let slot_y = s.row.offsets[cell.row as usize];
            let slot_w = span_size(&s.col.sizes, cell.col, cell.col_span, col_gap);
            let slot_h = span_size(&s.row.sizes, cell.row, cell.row_span, row_gap);
            (slot_x, slot_y, slot_w, slot_h)
        };

        // Grid: a child with no explicit alignment stretches to fill its cell
        // (WPF default) — `AutoBias::AlwaysStretch` collapses Auto to Stretch
        // even when the child isn't `Sizing::Fill`.
        let slot = Size::new(slot_w, slot_h);
        let p = place_two_axis(
            &s_node,
            parent_child_align,
            d,
            slot,
            AutoBias::AlwaysStretch,
        );
        let child_rect = Rect {
            min: inner.min + Vec2::new(slot_x, slot_y) + p.offset,
            size: p.size,
        };
        layout.arrange(tree, c, child_rect);
    }
}

/// Sum of spanned tracks' resolved sizes, or `∞` if any spanned track is not
/// yet resolved (Hug / Fill at measure time). Infinity makes the child fall
/// back to its intrinsic size on that axis (the WPF trick).
fn sum_spanned_known(sizes: &[f32], resolved: &[bool], start: u16, span: u16) -> f32 {
    let s = (start as usize).min(sizes.len());
    let n = (span as usize).min(sizes.len() - s);
    let mut sum = 0.0;
    for i in s..s + n {
        if !resolved[i] {
            return f32::INFINITY;
        }
        sum += sizes[i];
    }
    sum
}

fn track_offsets(sizes: &[f32], gap: f32, out: &mut [f32]) {
    assert_eq!(sizes.len(), out.len());
    let mut acc = 0.0f32;
    for (i, &s) in sizes.iter().enumerate() {
        out[i] = acc;
        acc += s;
        if i + 1 < sizes.len() {
            acc += gap;
        }
    }
}

fn span_size(sizes: &[f32], start: u16, span: u16, gap: f32) -> f32 {
    let s = (start as usize).min(sizes.len());
    let n = (span as usize).min(sizes.len() - s);
    let mut total: f32 = sizes[s..s + n].iter().sum();
    if n > 1 {
        total += gap * (n - 1) as f32;
    }
    total
}

/// Resolve track sizes on one axis into `a.sizes` for a grid with
/// `total` available main-axis length and `gap` between adjacent tracks.
/// `grid_sizing` is the grid node's own `Sizing` on this axis — used
/// only by Phase 4 to decide whether Fill tracks can be marked resolved
/// at measure time.
///
/// **Algorithm**, four phases:
/// 1. **Fixed:** clamp `Sizing::Fixed(v)` to `[Track.min, Track.max]`,
///    consume from available.
/// 2. **Hug:** constraint-solve `[hug_min ⊔ Track.min, hug_max ⊓ Track.max]`
///    for each Hug track against the remaining-after-Fixed:
///    - If `sum_hug_max <= remaining`: each Hug at max.
///    - If `sum_hug_min >= remaining`: each Hug at min, grid overflows.
///    - Else: each Hug starts at min, slack distributed proportional to
///      `(max - min)`.
/// 3. **Fill:** original constraint-by-exclusion algorithm — Fill tracks
///    distribute leftover proportional to weight; any Fill whose share
///    falls outside `[Track.min, Track.max]` clamps and exits the pool,
///    remaining Fills rebalance.
/// 4. **Mark Fill resolved (commit):** by default Fill tracks stay
///    unresolved so cells in Fill cols see `INF` via `sum_spanned_known`
///    during measure (preserves "Fill is finalized at arrange"). When
///    the grid itself is non-Hug on this axis with a finite slot, the
///    measure-time `total` matches arrange's, so Fill tracks can be
///    committed up-front and cells measure at the resolved width — wrap
///    text shapes correctly. Hug grids must keep Fill unresolved (their
///    arrange slot is unknown here). Arrange ignores `resolved`, so
///    passing the actual `grid_sizing` from arrange's call site is
///    harmless.
fn resolve_axis(
    a: &mut AxisScratch,
    hug_min: &[f32],
    hug_max: &[f32],
    total: f32,
    gap: f32,
    grid_sizing: Sizing,
) {
    let n = a.tracks.len();
    a.sizes.fill(0.0);
    // Reset resolved flags. Fixed + Hug get marked resolved as they're
    // computed. Fill stays unresolved so cells in Fill cols see INF as
    // their available width via `sum_spanned_known`, preserving the old
    // "Fill is finalized at arrange" behavior. Without this, cells in
    // Fill cols would measure with measure-time Fill leftover (a
    // finite value), then arrange might collapse Fill to 0 (e.g., Hug
    // grid) and the cell rect/shape would disagree.
    a.resolved.fill(false);
    let total_gap = gap * n.saturating_sub(1) as f32;

    // Phase 1: Fixed.
    let mut consumed = total_gap;
    for (i, t) in a.tracks.iter().enumerate() {
        if let Sizing::Fixed(v) = t.size {
            a.sizes[i] = v.clamp(t.min, t.max);
            a.resolved[i] = true;
            consumed += a.sizes[i];
        }
    }

    // Phase 2: Hug, constraint-solved against remaining-after-Fixed.
    let remaining_after_fixed = (total - consumed).max(0.0);
    let mut hug_min_sum = 0.0_f32;
    let mut hug_max_sum = 0.0_f32;
    let mut hug_count = 0_usize;
    for (i, t) in a.tracks.iter().enumerate() {
        if matches!(t.size, Sizing::Hug) {
            let lo = hug_min[i].max(t.min);
            let hi = hug_max[i].max(lo).min(t.max);
            hug_min_sum += lo;
            hug_max_sum += hi;
            hug_count += 1;
        }
    }

    if hug_count > 0 {
        if hug_max_sum <= remaining_after_fixed || total.is_infinite() {
            // Plenty of room (or unconstrained) → each Hug at max.
            for (i, t) in a.tracks.iter().enumerate() {
                if matches!(t.size, Sizing::Hug) {
                    let lo = hug_min[i].max(t.min);
                    let hi = hug_max[i].max(lo).min(t.max);
                    a.sizes[i] = hi;
                    a.resolved[i] = true;
                    consumed += hi;
                }
            }
        } else if hug_min_sum >= remaining_after_fixed {
            // Cramped → each Hug at min, grid overflows at this point.
            for (i, t) in a.tracks.iter().enumerate() {
                if matches!(t.size, Sizing::Hug) {
                    let lo = hug_min[i].max(t.min);
                    a.sizes[i] = lo;
                    a.resolved[i] = true;
                    consumed += lo;
                }
            }
        } else {
            // Slack distribution: start at min, grow toward max
            // proportional to per-track slack `(hi - lo)`.
            let slack = remaining_after_fixed - hug_min_sum;
            let total_range = hug_max_sum - hug_min_sum;
            for (i, t) in a.tracks.iter().enumerate() {
                if matches!(t.size, Sizing::Hug) {
                    let lo = hug_min[i].max(t.min);
                    let hi = hug_max[i].max(lo).min(t.max);
                    let share = if total_range > 0.0 {
                        slack * (hi - lo) / total_range
                    } else {
                        0.0
                    };
                    a.sizes[i] = (lo + share).min(hi);
                    a.resolved[i] = true;
                    consumed += a.sizes[i];
                }
            }
        }
    }

    // Phase 3: Fill — constraint-by-exclusion. Fills get the leftover
    // after Fixed + Hug, distributed by weight; any Fill whose share
    // falls outside `[Track.min, Track.max]` clamps and exits the
    // pool, remaining Fills rebalance.
    let mut remaining = (total - consumed).max(0.0);
    a.flexible.clear();
    let mut flexible_weight = 0.0_f32;
    for (i, t) in a.tracks.iter().enumerate() {
        if let Sizing::Fill(w) = t.size {
            a.flexible.push(i);
            flexible_weight += w;
        }
    }

    'outer: while !a.flexible.is_empty() && flexible_weight > 0.0 {
        let mut k = 0;
        while k < a.flexible.len() {
            let i = a.flexible[k];
            let t = &a.tracks[i];
            let w = match t.size {
                Sizing::Fill(w) => w,
                _ => unreachable!(),
            };
            let candidate = remaining * w / flexible_weight;
            if candidate < t.min || candidate > t.max {
                let clamped = candidate.clamp(t.min, t.max);
                a.sizes[i] = clamped;
                remaining = (remaining - clamped).max(0.0);
                flexible_weight -= w;
                a.flexible.remove(k);
                continue 'outer;
            }
            k += 1;
        }
        for &i in a.flexible.iter() {
            let w = match a.tracks[i].size {
                Sizing::Fill(w) => w,
                _ => unreachable!(),
            };
            a.sizes[i] = remaining * w / flexible_weight;
        }
        break;
    }

    // Phase 4: commit Fill tracks as resolved when the grid's own axis
    // sizing guarantees measure-time `total` matches arrange-time slot.
    if !matches!(grid_sizing, Sizing::Hug) && total.is_finite() {
        for (i, t) in a.tracks.iter().enumerate() {
            if matches!(t.size, Sizing::Fill(_)) {
                a.resolved[i] = true;
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
/// - `Fill(_)`: contributes `Track.min` only — Fill claims leftover at
///   distribution time, not in intrinsic.
///
/// Span > 1 cells are excluded (matches existing `measure` and the
/// commitment in `src/layout/intrinsic.md`).
pub(crate) fn intrinsic(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    idx: u16,
    axis: Axis,
    req: LenReq,
    text: &mut TextMeasurer,
) -> f32 {
    let def = &tree.grid.defs[idx as usize];
    let (tracks, gap, n_tracks) = match axis {
        Axis::X => (def.cols.clone(), def.col_gap, def.cols.len()),
        Axis::Y => (def.rows.clone(), def.row_gap, def.rows.len()),
    };
    if n_tracks == 0 {
        return 0.0;
    }

    // Bump-allocate `n_tracks` slots on the shared scratch. Recursive
    // intrinsic calls extend past `base + n_tracks` and truncate back, so
    // our slice stays valid across them.
    let base = layout.scratch.grid.track_aggregator.len();
    layout
        .scratch
        .grid
        .track_aggregator
        .resize(base + n_tracks, 0.0);
    for (i, t) in tracks.iter().enumerate() {
        layout.scratch.grid.track_aggregator[base + i] = match t.size {
            Sizing::Fixed(v) => v.clamp(t.min, t.max),
            // Hug starts at Track.min; Fill stays at Track.min.
            _ => t.min,
        };
    }

    for c in tree.children_active(node) {
        let cell = tree.read_extras(c).grid;
        let span = match axis {
            Axis::X => cell.col_span,
            Axis::Y => cell.row_span,
        };
        if span != 1 {
            continue;
        }
        let track_idx = match axis {
            Axis::X => cell.col as usize,
            Axis::Y => cell.row as usize,
        };
        if track_idx >= n_tracks {
            continue;
        }
        let t = &tracks[track_idx];
        if !matches!(t.size, Sizing::Hug) {
            continue;
        }
        let (t_min, t_max) = (t.min, t.max);
        let child_v = layout.intrinsic(tree, c, axis, req, text);
        let slot = &mut layout.scratch.grid.track_aggregator[base + track_idx];
        *slot = slot.max(child_v.clamp(t_min, t_max));
    }

    let total: f32 = layout.scratch.grid.track_aggregator[base..base + n_tracks]
        .iter()
        .sum();
    layout.scratch.grid.track_aggregator.truncate(base);
    total + gap * n_tracks.saturating_sub(1) as f32
}

#[cfg(test)]
mod tests;
