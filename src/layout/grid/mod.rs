use super::support::{AutoBias, place_two_axis, zero_subtree};
use super::{Axis, LayoutEngine, LenReq};
use crate::primitives::{Rect, Size, Sizing, Track};
use crate::text::TextMeasurer;
use crate::tree::{NodeId, Tree};
use glam::Vec2;
use std::ops::Range;
use std::rc::Rc;

struct DefSnapshot {
    n_rows: usize,
    n_cols: usize,
    row_gap: f32,
    col_gap: f32,
}

/// Snapshot a `GridDef` onto the scratch slot at `depth`: clones the track
/// `Rc<[Track]>`s (refcount-only), reads gaps, and resets the per-axis
/// scratch. Hug arrays live on `GridHugStore` (durable across the layout
/// pass) and are read/written by callers via `hugs.min/max(idx, axis)`.
fn snapshot_def(layout: &mut LayoutEngine, tree: &Tree, idx: u16, depth: usize) -> DefSnapshot {
    let def = tree.grid_def(idx);
    let n_rows = def.rows.len();
    let n_cols = def.cols.len();
    let rows = def.rows.clone();
    let cols = def.cols.clone();
    let row_gap = def.row_gap;
    let col_gap = def.col_gap;
    let s = layout.grid.depth_stack.at(depth);
    s.col.reset(cols);
    s.row.reset(rows);
    DefSnapshot {
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
    pub tracks: Rc<[Track]>,
    pub sizes: Vec<f32>,
    pub resolved: Vec<bool>,
    pub offsets: Vec<f32>,
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
    pub col: AxisScratch,
    pub row: AxisScratch,
}

/// All grid-layout scratch held by `LayoutEngine`, in one bag. `depth_stack`
/// and `hugs` are separate fields so callers can disjoint-borrow them —
/// `resolve_axis` takes `&mut AxisScratch` (from `depth_stack`) and `&[f32]`
/// hug slices (from `hugs`) in the same expression via destructuring.
/// `intrinsic_scratch` is a bump-stack scratch for `grid::intrinsic`'s
/// per-track aggregator: each call extends by `n_tracks`, recurses (which
/// may extend further but always truncates back), then truncates to its
/// own base. Capacity retained.
#[derive(Default)]
pub(crate) struct GridContext {
    pub(super) depth_stack: GridDepthStack,
    pub(super) hugs: GridHugStore,
    pub(super) intrinsic_scratch: Vec<f32>,
}

/// Nesting stack of per-depth grid scratch. One `GridScratch` slot per
/// active `LayoutMode::Grid` ancestor. `depth` is the next free slot.
#[derive(Default)]
pub(crate) struct GridDepthStack {
    scratch: Vec<GridScratch>,
    depth: usize,
}

impl GridDepthStack {
    pub(super) fn depth(&self) -> usize {
        self.depth
    }

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
    rows: HugSlice,
    cols: HugSlice,
}

#[derive(Clone, Copy, Default)]
struct HugSlice {
    start: u32,
    len: u32,
}

impl HugSlice {
    fn range(self) -> Range<usize> {
        self.start as usize..(self.start as usize + self.len as usize)
    }
}

impl GridHugStore {
    pub(super) fn reset_for(&mut self, tree: &Tree) {
        self.max_pool.clear();
        self.min_pool.clear();
        self.slots.clear();
        for def in tree.grid_defs() {
            let rows = self.alloc(def.rows.len());
            let cols = self.alloc(def.cols.len());
            self.slots.push(GridHugSlot { rows, cols });
        }
    }

    fn alloc(&mut self, n: usize) -> HugSlice {
        let start = self.max_pool.len() as u32;
        self.max_pool.resize(start as usize + n, 0.0);
        self.min_pool.resize(start as usize + n, 0.0);
        HugSlice {
            start,
            len: n as u32,
        }
    }

    fn axis_slice(&self, idx: u16, axis: Axis) -> Range<usize> {
        let slot = self.slots[idx as usize];
        let s = match axis {
            Axis::X => slot.cols,
            Axis::Y => slot.rows,
        };
        s.range()
    }

    pub(super) fn max(&self, idx: u16, axis: Axis) -> &[f32] {
        &self.max_pool[self.axis_slice(idx, axis)]
    }
    pub(super) fn min(&self, idx: u16, axis: Axis) -> &[f32] {
        &self.min_pool[self.axis_slice(idx, axis)]
    }
    pub(super) fn max_mut(&mut self, idx: u16, axis: Axis) -> &mut [f32] {
        let r = self.axis_slice(idx, axis);
        &mut self.max_pool[r]
    }
    pub(super) fn min_mut(&mut self, idx: u16, axis: Axis) -> &mut [f32] {
        let r = self.axis_slice(idx, axis);
        &mut self.min_pool[r]
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
pub(super) fn measure(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    idx: u16,
    inner_avail: Size,
    text: &mut TextMeasurer,
) -> Size {
    let depth = layout.grid.depth_stack.enter();
    let result = measure_inner(layout, tree, node, idx, depth, inner_avail, text);
    layout.grid.depth_stack.exit();
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
    let DefSnapshot {
        n_rows,
        n_cols,
        row_gap,
        col_gap,
    } = snapshot_def(layout, tree, idx, depth);

    if n_rows == 0 || n_cols == 0 {
        // Still measure children so their `desired` is set.
        for c in tree.children(node) {
            layout.measure(tree, c, Size::ZERO, text);
        }
        return Size::ZERO;
    }

    // Phase 1: query column intrinsics for Hug-column span-1 cells.
    // Resolves the col axis without measuring children — gives cells a
    // committed column width before they shape, which is the whole point
    // of Step B.
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
        let cmin = layout.intrinsic(tree, c, Axis::X, LenReq::MinContent, text);
        let cmax = layout.intrinsic(tree, c, Axis::X, LenReq::MaxContent, text);
        let i = cell.col as usize;
        let cols_min = layout.grid.hugs.min_mut(idx, Axis::X);
        cols_min[i] = cols_min[i].max(cmin);
        let cols_max = layout.grid.hugs.max_mut(idx, Axis::X);
        cols_max[i] = cols_max[i].max(cmax);
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
        } = &mut layout.grid;
        let s = depth_stack.at(depth);
        resolve_axis(
            &mut s.col,
            hugs.min(idx, Axis::X),
            hugs.max(idx, Axis::X),
            inner_avail.w,
            col_gap,
        );
        mark_fill_resolved(&mut s.col, grid_sizing_w, inner_avail.w);
        // Resolve Fixed rows once before the per-cell loop — values are
        // constant per GridDef and `resolve_fixed` is idempotent, so
        // calling it inside the loop just re-set the same slots.
        resolve_fixed(&mut s.row);
    }

    // Phase 2: measure cells with resolved col widths. Rows are still
    // unresolved (only Fixed is known); cells get INF on row axis as
    // before. Cell desired heights feed row Hug resolution next.
    for c in tree.children(node) {
        let collapsed = tree.is_collapsed(c);
        let cell = tree.read_extras(c).grid;

        let avail = {
            let s = layout.grid.depth_stack.at(depth);
            // `sum_spanned_known` returns INFINITY if any spanned col is
            // unresolved. After `resolve_axis` ran above, Fixed and Hug
            // cols are marked resolved; Fill cols intentionally stay
            // unresolved so cells in them get INF here — preserves the
            // pre-Step B behavior where Fill is finalized only at
            // arrange time. Without this, cells in Fill cols measure at
            // a different width than they're arranged at, and that
            // discrepancy commits row heights based on a width arrange
            // doesn't honor.
            let avail_w = sum_spanned_known(&s.col.sizes, &s.col.resolved, cell.col, cell.col_span);
            // Rows: only Fixed is known yet; Hug and Fill are unresolved
            // → INF (WPF intrinsic trick), as before.
            let avail_h = sum_spanned_known(&s.row.sizes, &s.row.resolved, cell.row, cell.row_span);
            Size::new(avail_w, avail_h)
        };

        let d = layout.measure(tree, c, avail, text);
        if collapsed {
            continue;
        }

        // Row Hug accumulates from cell's measured height. Row min-content
        // could come from a Y intrinsic query, but it'd be the single-line
        // height — the wrapped height (in `desired.h`) is what actually
        // matters, so leave row hug_min at zero for now.
        let GridContext {
            depth_stack, hugs, ..
        } = &mut layout.grid;
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
        } = &mut layout.grid;
        let s = depth_stack.at(depth);
        resolve_axis(
            &mut s.row,
            hugs.min(idx, Axis::Y),
            hugs.max(idx, Axis::Y),
            inner_avail.h,
            row_gap,
        );
        mark_fill_resolved(&mut s.row, grid_sizing_h, inner_avail.h);
    }

    // Returned content size: sum of non-Fill track sizes + gaps. Fill
    // tracks "want 0" in measure context — they only claim leftover at
    // arrange time. Mirrors WPF's "Star contributes 0 to content size."
    let s = layout.grid.depth_stack.at(depth);
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
    let s = layout.grid.depth_stack.at(depth);
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

/// After `resolve_axis` runs, Fill tracks stay unresolved so cells in
/// Fill cols/rows see `INF` via `sum_spanned_known` during measure. When
/// the grid itself is non-Hug on this axis with a finite slot, that slot
/// width will match arrange's, so we can mark Fill resolved up-front and
/// let cells measure at the resolved width — wrap text shapes correctly.
/// Hug grids must keep Fill unresolved (their arrange slot is unknown
/// here).
fn mark_fill_resolved(a: &mut AxisScratch, grid_sizing: Sizing, avail: f32) {
    if matches!(grid_sizing, Sizing::Hug) || !avail.is_finite() {
        return;
    }
    for (i, t) in a.tracks.iter().enumerate() {
        if matches!(t.size, Sizing::Fill(_)) {
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

pub(super) fn arrange(layout: &mut LayoutEngine, tree: &Tree, node: NodeId, inner: Rect, idx: u16) {
    let depth = layout.grid.depth_stack.enter();
    arrange_inner(layout, tree, node, inner, idx, depth);
    layout.grid.depth_stack.exit();
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
    let DefSnapshot {
        n_rows,
        n_cols,
        row_gap,
        col_gap,
    } = snapshot_def(layout, tree, idx, depth);

    if n_rows == 0 || n_cols == 0 {
        for c in tree.children(node) {
            zero_subtree(layout, tree, c, inner.min);
        }
        return;
    }

    // Resolve track sizes (Fixed + Hug + Fill) and compute offsets.
    {
        let GridContext {
            depth_stack, hugs, ..
        } = &mut layout.grid;
        let s = depth_stack.at(depth);
        resolve_axis(
            &mut s.col,
            hugs.min(idx, Axis::X),
            hugs.max(idx, Axis::X),
            inner.size.w,
            col_gap,
        );
        resolve_axis(
            &mut s.row,
            hugs.min(idx, Axis::Y),
            hugs.max(idx, Axis::Y),
            inner.size.h,
            row_gap,
        );
        track_offsets(&s.col.sizes, col_gap, &mut s.col.offsets);
        track_offsets(&s.row.sizes, row_gap, &mut s.row.offsets);
    }

    let parent_child_align = tree.read_extras(node).child_align;
    for c in tree.children(node) {
        if tree.is_collapsed(c) {
            zero_subtree(layout, tree, c, inner.min);
            continue;
        }
        let s_node = *tree.layout(c);
        let cell = tree.read_extras(c).grid;
        let d = layout.desired[c.index()];

        let (slot_x, slot_y, slot_w, slot_h) = {
            let s = layout.grid.depth_stack.at(depth);
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
        let (size, off) = place_two_axis(
            &s_node,
            parent_child_align,
            d,
            slot,
            AutoBias::AlwaysStretch,
        );
        let child_rect = Rect {
            min: inner.min + Vec2::new(slot_x, slot_y) + off,
            size,
        };
        layout.arrange(tree, c, child_rect);
    }
}

/// Sum of spanned tracks' resolved sizes, or `∞` if any spanned track is not
/// yet resolved (Hug / Fill at measure time). Infinity makes the child fall
/// back to its intrinsic size on that axis (the WPF trick).
fn sum_spanned_known(sizes: &[f32], resolved: &[bool], start: u16, span: u16) -> f32 {
    let s = start as usize;
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
    let s = start as usize;
    let n = (span as usize).min(sizes.len() - s);
    let mut total: f32 = sizes[s..s + n].iter().sum();
    if n > 1 {
        total += gap * (n - 1) as f32;
    }
    total
}

/// Resolve track sizes on one axis into `a.sizes` for a grid with
/// `total` available main-axis length and `gap` between adjacent tracks.
///
/// **Step B algorithm**, three phases:
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
fn resolve_axis(a: &mut AxisScratch, hug_min: &[f32], hug_max: &[f32], total: f32, gap: f32) {
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

    // Phase 3: Fill — constraint-by-exclusion (preserved from pre-Step B
    // algorithm). Fills get the leftover after Fixed + Hug.
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
}

/// Intrinsic size of a Grid: per-track contribution aggregated from
/// span-1 cells, summed across tracks plus gaps. Step B's full algorithm
/// will refactor `measure` to consume this; for Step A this just answers
/// "what would the Grid prefer to be on this axis?" so callers can read
/// it without measuring.
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
pub(super) fn intrinsic(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    idx: u16,
    axis: Axis,
    req: LenReq,
    text: &mut TextMeasurer,
) -> f32 {
    let def = tree.grid_def(idx);
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
    let base = layout.grid.intrinsic_scratch.len();
    layout.grid.intrinsic_scratch.resize(base + n_tracks, 0.0);
    for (i, t) in tracks.iter().enumerate() {
        layout.grid.intrinsic_scratch[base + i] = match t.size {
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
        let slot = &mut layout.grid.intrinsic_scratch[base + track_idx];
        *slot = slot.max(child_v.clamp(t_min, t_max));
    }

    let total: f32 = layout.grid.intrinsic_scratch[base..base + n_tracks]
        .iter()
        .sum();
    layout.grid.intrinsic_scratch.truncate(base);
    total + gap * n_tracks.saturating_sub(1) as f32
}

#[cfg(test)]
mod tests;
