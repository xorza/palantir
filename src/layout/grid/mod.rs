use super::{Axis, LayoutEngine, LenReq, place_axis, resolved_axis_align, zero_subtree};
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
pub(crate) struct AxisScratch {
    pub tracks: Rc<[Track]>,
    pub sizes: Vec<f32>,
    pub resolved: Vec<bool>,
    pub hug: Vec<f32>,
    pub offsets: Vec<f32>,
    flexible: Vec<usize>,
}

impl Default for AxisScratch {
    fn default() -> Self {
        Self {
            tracks: Rc::from([]),
            sizes: Vec::new(),
            resolved: Vec::new(),
            hug: Vec::new(),
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
        self.hug.clear();
        self.hug.resize(n, 0.0);
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
/// `GridDef`. Measure pass writes; arrange pass reads. Scratch in
/// `GridLayout::scratch[depth]` would get clobbered by sibling grids before
/// arrange runs, so the pool persists for the whole layout pass instead.
/// Reset at the start of each pass; capacity retained across frames.
#[derive(Default)]
pub(crate) struct GridHugStore {
    pool: Vec<f32>,
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
        self.pool.clear();
        self.slots.clear();
        for def in tree.grid_defs() {
            let rows = self.alloc(def.rows.len());
            let cols = self.alloc(def.cols.len());
            self.slots.push(GridHugSlot { rows, cols });
        }
    }

    fn alloc(&mut self, n: usize) -> HugSlice {
        let start = self.pool.len() as u32;
        self.pool.resize(start as usize + n, 0.0);
        HugSlice {
            start,
            len: n as u32,
        }
    }

    pub(super) fn rows(&self, idx: u16) -> &[f32] {
        &self.pool[self.slots[idx as usize].rows.range()]
    }

    pub(super) fn cols(&self, idx: u16) -> &[f32] {
        &self.pool[self.slots[idx as usize].cols.range()]
    }

    pub(super) fn rows_mut(&mut self, idx: u16) -> &mut [f32] {
        &mut self.pool[self.slots[idx as usize].rows.range()]
    }

    pub(super) fn cols_mut(&mut self, idx: u16) -> &mut [f32] {
        &mut self.pool[self.slots[idx as usize].cols.range()]
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
    text: &mut TextMeasurer,
) -> Size {
    let depth = layout.grid.depth_stack.enter();
    let result = measure_inner(layout, tree, node, idx, depth, text);
    layout.grid.depth_stack.exit();
    result
}

fn measure_inner(
    layout: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    idx: u16,
    depth: usize,
    text: &mut TextMeasurer,
) -> Size {
    let DefSnapshot {
        n_rows,
        n_cols,
        row_gap,
        col_gap,
    } = snapshot_def(layout, tree, idx, depth);
    {
        let s = layout.grid.depth_stack.at(depth);
        resolve_fixed(&mut s.col);
        resolve_fixed(&mut s.row);
    }

    if n_rows == 0 || n_cols == 0 {
        // Still measure children so their `desired` is set.
        let mut kids = tree.child_cursor(node);
        while let Some(c) = kids.next(tree) {
            layout.measure(tree, c, Size::ZERO, text);
        }
        return Size::ZERO;
    }

    // Walk children: brief scratch borrows around each recursion.
    let mut kids = tree.child_cursor(node);
    while let Some(c) = kids.next(tree) {
        let collapsed = tree.is_collapsed(c);
        let cell = tree.read_extras(c).grid;
        assert_cell(cell, n_rows, n_cols);

        let avail = {
            let s = layout.grid.depth_stack.at(depth);
            let avail_w = sum_spanned_known(&s.col.sizes, &s.col.resolved, cell.col, cell.col_span);
            let avail_h = sum_spanned_known(&s.row.sizes, &s.row.resolved, cell.row, cell.row_span);
            Size::new(avail_w, avail_h)
        };

        let d = layout.measure(tree, c, avail, text);
        if collapsed {
            continue;
        }

        // Span-1 only drives Hug-track sizing (avoids the WPF Auto↔Star
        // cyclic-iteration trap).
        let s = layout.grid.depth_stack.at(depth);
        record_hug(&mut s.col, cell.col, cell.col_span, d.w);
        record_hug(&mut s.row, cell.row, cell.row_span, d.h);
    }

    // Resolve Hug tracks from accumulated hug sizes, persist them onto
    // `LayoutResult` so `arrange` can read them without re-walking children
    // (engine scratch is depth-keyed and gets clobbered by sibling grids),
    // and sum content size. `layout.grid` and `layout.result` are separate
    // fields so both can be borrowed simultaneously via field access.
    let s = layout.grid.depth_stack.at(depth);
    resolve_hug(&mut s.col);
    resolve_hug(&mut s.row);
    let total_w = s.col.sizes.iter().sum::<f32>() + col_gap * n_cols.saturating_sub(1) as f32;
    let total_h = s.row.sizes.iter().sum::<f32>() + row_gap * n_rows.saturating_sub(1) as f32;

    layout.grid.hugs.cols_mut(idx).copy_from_slice(&s.col.hug);
    layout.grid.hugs.rows_mut(idx).copy_from_slice(&s.row.hug);

    Size::new(total_w, total_h)
}

fn resolve_fixed(a: &mut AxisScratch) {
    for (i, t) in a.tracks.iter().enumerate() {
        if let Sizing::Fixed(v) = t.size {
            a.sizes[i] = v.clamp(t.min, t.max);
            a.resolved[i] = true;
        }
    }
}

fn record_hug(a: &mut AxisScratch, idx: u16, span: u16, desired: f32) {
    if span != 1 {
        return;
    }
    let i = idx as usize;
    if matches!(a.tracks[i].size, Sizing::Hug) {
        a.hug[i] = a.hug[i].max(desired);
    }
}

fn resolve_hug(a: &mut AxisScratch) {
    for (i, t) in a.tracks.iter().enumerate() {
        if matches!(t.size, Sizing::Hug) {
            a.sizes[i] = a.hug[i].clamp(t.min, t.max);
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
        s.col.hug.copy_from_slice(layout.grid.hugs.cols(idx));
        s.row.hug.copy_from_slice(layout.grid.hugs.rows(idx));
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
        // (WPF default). `place_axis` is told `auto_stretches = true` so Auto
        // collapses to Stretch even when the child isn't `Sizing::Fill`.
        let (h_align, v_align) = resolved_axis_align(&s_node, parent_child_align);
        let (w, x_off) = place_axis(h_align, s_node.size.w, d.w, slot_w, true);
        let (h, y_off) = place_axis(v_align, s_node.size.h, d.h, slot_h, true);

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

/// Resolve track sizes on one axis into `a.sizes`. Fixed and Hug tracks are
/// clamped to `[min, max]` once. Star tracks split the leftover proportionally
/// to weight, using **constraint resolution by exclusion** (CSS Grid / Flutter
/// flex): any star whose proportional share violates `[min, max]` clamps and
/// exits the pool, the remaining stars rebalance, repeat until stable.
/// Bounded — each iteration removes at least one star, so O(N²) worst case.
fn resolve_axis(a: &mut AxisScratch, total: f32, gap: f32) {
    let n = a.tracks.len();
    a.sizes.fill(0.0);

    let mut consumed = gap * n.saturating_sub(1) as f32;
    a.flexible.clear();
    let mut flexible_weight = 0.0f32;

    for (i, t) in a.tracks.iter().enumerate() {
        match t.size {
            Sizing::Fixed(v) => {
                a.sizes[i] = v.clamp(t.min, t.max);
                consumed += a.sizes[i];
            }
            Sizing::Hug => {
                a.sizes[i] = a.hug[i].clamp(t.min, t.max);
                consumed += a.sizes[i];
            }
            Sizing::Fill(w) => {
                a.flexible.push(i);
                flexible_weight += w;
            }
        }
    }

    let mut remaining = (total - consumed).max(0.0);

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
/// Step B design commitment in `docs/intrinsics.md`).
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
