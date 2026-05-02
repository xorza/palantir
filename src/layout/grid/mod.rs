use super::{AutoBias, Axis, LayoutEngine, LenReq, place_axis, resolved_axis_align, zero_subtree};
use crate::primitives::{GridCell, Rect, Size, Sizing, Track};
use crate::text::TextMeasurer;
use crate::tree::{NodeId, Tree};
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
/// scratch. Hug arrays live on `LayoutResult` and are read/written by
/// caller — they don't fit in the snapshot.
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
/// `hug_max` / `hug_min` are the per-track content-driven `[min, max]`
/// ranges for Hug tracks: max-content from `desired` (children measured
/// with INFINITY); min-content from intrinsic queries on each span-1
/// cell. The pair feeds the constraint solver in `resolve_axis` so Hug
/// tracks fit inside their parent's available width — see
/// `src/layout/intrinsic.md`.
pub(crate) struct AxisScratch {
    pub tracks: Rc<[Track]>,
    pub sizes: Vec<f32>,
    pub resolved: Vec<bool>,
    pub hug_max: Vec<f32>,
    pub hug_min: Vec<f32>,
    pub offsets: Vec<f32>,
    flexible: Vec<usize>,
}

impl Default for AxisScratch {
    fn default() -> Self {
        Self {
            tracks: Rc::from([]),
            sizes: Vec::new(),
            resolved: Vec::new(),
            hug_max: Vec::new(),
            hug_min: Vec::new(),
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
        self.hug_max.clear();
        self.hug_max.resize(n, 0.0);
        self.hug_min.clear();
        self.hug_min.resize(n, 0.0);
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

/// All grid-layout scratch held by `LayoutEngine`, in one bag. The two
/// fields are separate so writers can hold `&mut hugs` while a
/// `&mut depth_stack[i]` borrow is live (the encoder copies hugs from
/// scratch into the durable pool inside the same expression).
#[derive(Default)]
pub(crate) struct GridContext {
    pub(super) depth_stack: GridDepthStack,
    pub(super) hugs: GridHugStore,
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
/// track so Step B's constraint solver can range-distribute Hug tracks.
/// Measure pass writes; arrange pass reads. Scratch in
/// `GridLayout::scratch[depth]` would get clobbered by sibling grids before
/// arrange runs, so the pool persists for the whole layout pass instead.
/// Reset at the start of each pass; capacity retained across frames.
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

    pub(super) fn rows_max(&self, idx: u16) -> &[f32] {
        &self.max_pool[self.slots[idx as usize].rows.range()]
    }
    pub(super) fn cols_max(&self, idx: u16) -> &[f32] {
        &self.max_pool[self.slots[idx as usize].cols.range()]
    }
    pub(super) fn rows_min(&self, idx: u16) -> &[f32] {
        &self.min_pool[self.slots[idx as usize].rows.range()]
    }
    pub(super) fn cols_min(&self, idx: u16) -> &[f32] {
        &self.min_pool[self.slots[idx as usize].cols.range()]
    }

    pub(super) fn rows_max_mut(&mut self, idx: u16) -> &mut [f32] {
        &mut self.max_pool[self.slots[idx as usize].rows.range()]
    }
    pub(super) fn cols_max_mut(&mut self, idx: u16) -> &mut [f32] {
        &mut self.max_pool[self.slots[idx as usize].cols.range()]
    }
    pub(super) fn rows_min_mut(&mut self, idx: u16) -> &mut [f32] {
        &mut self.min_pool[self.slots[idx as usize].rows.range()]
    }
    pub(super) fn cols_min_mut(&mut self, idx: u16) -> &mut [f32] {
        &mut self.min_pool[self.slots[idx as usize].cols.range()]
    }
}

/// WPF-style grid measure. Resolves Fixed tracks, walks children once feeding
/// each `Σ spanned-track sizes` (or `∞` if any spanned track is unresolved —
/// the WPF infinity trick → child reports intrinsic), then resolves Hug
/// tracks from span-1 children's desired sizes. Star tracks contribute 0 to
/// the grid's content size — final star sizes only resolve in arrange. See
/// `docs/grid.md`.
///
/// Scratch lives in `Layout::grid_scratch[depth]` (heap, capacity-retained
/// across frames). No fixed track-count limit. Per-track hug sizes are
/// persisted onto `LayoutResult` so `arrange` can read them without
/// re-walking children — engine scratch is keyed by depth and would be
/// clobbered by sibling grids.
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
        let mut kids = tree.child_cursor(node);
        while let Some(c) = kids.next(tree) {
            layout.measure(tree, c, Size::ZERO, text);
        }
        return Size::ZERO;
    }

    // Phase 1: query column intrinsics for Hug-column span-1 cells.
    // Resolves the col axis without measuring children — gives cells a
    // committed column width before they shape, which is the whole point
    // of Step B.
    let col_tracks = tracks_at(layout, depth, Axis::X);
    let mut kids = tree.child_cursor(node);
    while let Some(c) = kids.next(tree) {
        if tree.is_collapsed(c) {
            continue;
        }
        let cell = tree.read_extras(c).grid;
        assert_cell(cell, n_rows, n_cols);
        if cell.col_span != 1 {
            continue;
        }
        let t = &col_tracks[cell.col as usize];
        if !matches!(t.size, Sizing::Hug) {
            continue;
        }
        let cmin = layout.intrinsic(tree, c, Axis::X, LenReq::MinContent, text);
        let cmax = layout.intrinsic(tree, c, Axis::X, LenReq::MaxContent, text);
        let s = layout.grid.depth_stack.at(depth);
        s.col.hug_min[cell.col as usize] = s.col.hug_min[cell.col as usize].max(cmin);
        s.col.hug_max[cell.col as usize] = s.col.hug_max[cell.col as usize].max(cmax);
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
        let s = layout.grid.depth_stack.at(depth);
        resolve_axis(&mut s.col, inner_avail.w, col_gap);
        if !matches!(grid_sizing_w, Sizing::Hug) && inner_avail.w.is_finite() {
            for (i, t) in s.col.tracks.iter().enumerate() {
                if matches!(t.size, Sizing::Fill(_)) {
                    s.col.resolved[i] = true;
                }
            }
        }
    }

    // Phase 2: measure cells with resolved col widths. Rows are still
    // unresolved (only Fixed is known); cells get INF on row axis as
    // before. Cell desired heights feed row Hug resolution next.
    let mut kids = tree.child_cursor(node);
    while let Some(c) = kids.next(tree) {
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
            resolve_fixed(&mut s.row);
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
        let s = layout.grid.depth_stack.at(depth);
        record_hug(&mut s.row, cell.row, cell.row_span, d.h, None);
    }

    // Resolve row heights. Same Fill-marking rule as cols above —
    // mark Fill rows resolved only when the grid is non-Hug on h.
    // (Cells already measured by this point, so the resolved flag here
    // doesn't affect the current measure; it carries forward into
    // arrange's re-resolve via the persisted state.)
    {
        let s = layout.grid.depth_stack.at(depth);
        resolve_axis(&mut s.row, inner_avail.h, row_gap);
        if !matches!(grid_sizing_h, Sizing::Hug) && inner_avail.h.is_finite() {
            for (i, t) in s.row.tracks.iter().enumerate() {
                if matches!(t.size, Sizing::Fill(_)) {
                    s.row.resolved[i] = true;
                }
            }
        }
    }

    // Persist hug pools so arrange can re-load them after sibling grids
    // clobber depth scratch.
    let s = layout.grid.depth_stack.at(depth);
    layout
        .grid
        .hugs
        .cols_max_mut(idx)
        .copy_from_slice(&s.col.hug_max);
    layout
        .grid
        .hugs
        .rows_max_mut(idx)
        .copy_from_slice(&s.row.hug_max);
    layout
        .grid
        .hugs
        .cols_min_mut(idx)
        .copy_from_slice(&s.col.hug_min);
    layout
        .grid
        .hugs
        .rows_min_mut(idx)
        .copy_from_slice(&s.row.hug_min);

    // Returned content size: sum of non-Fill track sizes + gaps. Fill
    // tracks "want 0" in measure context — they only claim leftover at
    // arrange time. Mirrors WPF's "Star contributes 0 to content size."
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

/// Borrow the per-axis tracks at `depth`. Helper so we can read tracks
/// for the per-axis `Sizing` check without holding a long-lived `at()`
/// borrow that conflicts with the subsequent intrinsic call.
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

fn record_hug(a: &mut AxisScratch, idx: u16, span: u16, desired: f32, intrinsic_min: Option<f32>) {
    if span != 1 {
        return;
    }
    let i = idx as usize;
    if matches!(a.tracks[i].size, Sizing::Hug) {
        a.hug_max[i] = a.hug_max[i].max(desired);
        if let Some(m) = intrinsic_min {
            a.hug_min[i] = a.hug_min[i].max(m);
        }
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
    // Re-snapshot and reload hugs from the per-grid pool: scratch at this
    // depth gets clobbered by sibling grids between measure and arrange, so
    // `GridHugStore` is the durable record across the layout pass.
    let DefSnapshot {
        n_rows,
        n_cols,
        row_gap,
        col_gap,
    } = snapshot_def(layout, tree, idx, depth);
    {
        let s = layout.grid.depth_stack.at(depth);
        s.col
            .hug_max
            .copy_from_slice(layout.grid.hugs.cols_max(idx));
        s.row
            .hug_max
            .copy_from_slice(layout.grid.hugs.rows_max(idx));
        s.col
            .hug_min
            .copy_from_slice(layout.grid.hugs.cols_min(idx));
        s.row
            .hug_min
            .copy_from_slice(layout.grid.hugs.rows_min(idx));
    }

    if n_rows == 0 || n_cols == 0 {
        let mut kids = tree.child_cursor(node);
        while let Some(c) = kids.next(tree) {
            zero_subtree(layout, tree, c, inner.min);
        }
        return;
    }

    // Resolve track sizes (Fixed + Hug + Fill) and compute offsets.
    {
        let s = layout.grid.depth_stack.at(depth);
        resolve_axis(&mut s.col, inner.size.w, col_gap);
        resolve_axis(&mut s.row, inner.size.h, row_gap);
        track_offsets(&s.col.sizes, col_gap, &mut s.col.offsets);
        track_offsets(&s.row.sizes, row_gap, &mut s.row.offsets);
    }

    let parent_child_align = tree.read_extras(node).child_align;
    let mut kids = tree.child_cursor(node);
    while let Some(c) = kids.next(tree) {
        if tree.is_collapsed(c) {
            zero_subtree(layout, tree, c, inner.min);
            continue;
        }
        let s_node = *tree.layout(c);
        let cell = tree.read_extras(c).grid;
        assert_cell(cell, n_rows, n_cols);
        let d = layout.desired(c);

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
        let (h_align, v_align) = resolved_axis_align(&s_node, parent_child_align);
        let (w, x_off) = place_axis(h_align, s_node.size.w, d.w, slot_w, AutoBias::AlwaysStretch);
        let (h, y_off) = place_axis(v_align, s_node.size.h, d.h, slot_h, AutoBias::AlwaysStretch);

        let child_rect = Rect::new(
            inner.min.x + slot_x + x_off,
            inner.min.y + slot_y + y_off,
            w,
            h,
        );
        layout.arrange(tree, c, child_rect);
    }
}

/// Validate a cell against the grid's track counts. Panics in both debug and
/// release if the cell is out of range or has zero span — silent clamping
/// would hide real authoring bugs (e.g. a child placed past the last column).
fn assert_cell(c: GridCell, n_rows: usize, n_cols: usize) {
    let row = c.row as usize;
    let col = c.col as usize;
    let row_span = c.row_span as usize;
    let col_span = c.col_span as usize;
    assert!(
        row < n_rows
            && col < n_cols
            && row_span >= 1
            && col_span >= 1
            && row + row_span <= n_rows
            && col + col_span <= n_cols,
        "grid cell out of range: {c:?} for {n_rows}x{n_cols}"
    );
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
fn resolve_axis(a: &mut AxisScratch, total: f32, gap: f32) {
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
            let lo = a.hug_min[i].max(t.min);
            let hi = a.hug_max[i].max(lo).min(t.max);
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
                    let lo = a.hug_min[i].max(t.min);
                    let hi = a.hug_max[i].max(lo).min(t.max);
                    a.sizes[i] = hi;
                    a.resolved[i] = true;
                    consumed += hi;
                }
            }
        } else if hug_min_sum >= remaining_after_fixed {
            // Cramped → each Hug at min, grid overflows at this point.
            for (i, t) in a.tracks.iter().enumerate() {
                if matches!(t.size, Sizing::Hug) {
                    let lo = a.hug_min[i].max(t.min);
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
                    let lo = a.hug_min[i].max(t.min);
                    let hi = a.hug_max[i].max(lo).min(t.max);
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
            assert!(w > 0.0, "Sizing::Fill weight must be positive");
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

    let mut track_size = vec![0.0_f32; n_tracks];
    for (i, t) in tracks.iter().enumerate() {
        track_size[i] = match t.size {
            Sizing::Fixed(v) => v.clamp(t.min, t.max),
            // Hug starts at Track.min; Fill stays at Track.min.
            _ => t.min,
        };
    }

    let mut kids = tree.child_cursor(node);
    while let Some(c) = kids.next(tree) {
        if tree.is_collapsed(c) {
            continue;
        }
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
        let child_v = layout.intrinsic(tree, c, axis, req, text);
        track_size[track_idx] = track_size[track_idx].max(child_v.clamp(t.min, t.max));
    }

    let total: f32 = track_size.iter().sum();
    total + gap * n_tracks.saturating_sub(1) as f32
}

#[cfg(test)]
mod tests;
