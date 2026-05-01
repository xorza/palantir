use super::LayoutEngine;
use crate::primitives::{GridCell, Rect, Size, Sizing, Track, Visibility};
use crate::tree::{NodeId, Tree};

/// Per-frame scratch for `Grid` layout. Capacity is retained across frames; a
/// `Vec<GridScratch>` indexed by nesting depth lets nested grids each have
/// their own slot. Pushed on first descent to a new depth.
#[derive(Default)]
pub(crate) struct GridScratch {
    pub rows: Vec<Track>,
    pub cols: Vec<Track>,
    pub col_sizes: Vec<f32>,
    pub row_sizes: Vec<f32>,
    pub col_resolved: Vec<bool>,
    pub row_resolved: Vec<bool>,
    pub hug_w: Vec<f32>,
    pub hug_h: Vec<f32>,
    pub col_offsets: Vec<f32>,
    pub row_offsets: Vec<f32>,
    pub flexible: Vec<usize>,
}

/// Grid-layout state held by `LayoutEngine`. One `GridScratch` per nesting
/// depth of `LayoutMode::Grid`. `depth` is the next free slot.
#[derive(Default)]
pub(crate) struct GridLayout {
    scratch: Vec<GridScratch>,
    depth: usize,
}

impl GridLayout {
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

/// WPF-style grid measure. Resolves Fixed tracks, walks children once feeding
/// each `Σ spanned-track sizes` (or `∞` if any spanned track is unresolved —
/// the WPF infinity trick → child reports intrinsic), then resolves Hug
/// tracks from span-1 children's desired sizes. Star tracks contribute 0 to
/// the grid's content size — final star sizes only resolve in arrange. See
/// `docs/grid.md`.
///
/// Scratch lives in `Layout::grid_scratch[depth]` (heap, capacity-retained
/// across frames). No fixed track-count limit. Per-track hug sizes are
/// written through to `Tree::hug_pool` so `arrange` can read them without
/// re-walking children.
pub(super) fn measure(layout: &mut LayoutEngine, tree: &mut Tree, node: NodeId, idx: u32) -> Size {
    let depth = layout.grid.enter();
    let result = measure_inner(layout, tree, node, idx, depth);
    layout.grid.exit();
    result
}

fn measure_inner(
    layout: &mut LayoutEngine,
    tree: &mut Tree,
    node: NodeId,
    idx: u32,
    depth: usize,
) -> Size {
    // Setup: snapshot tracks + gaps onto our scratch slot, resolve Fixed.
    let (n_rows, n_cols, row_gap, col_gap, row_hugs_slice, col_hugs_slice) = {
        let def = tree.grid_def(idx);
        let row_tracks: &[Track] = tree.grid_tracks(def.rows);
        let col_tracks: &[Track] = tree.grid_tracks(def.cols);
        let n_rows = row_tracks.len();
        let n_cols = col_tracks.len();
        let s = layout.grid.at(depth);
        s.rows.clear();
        s.rows.extend_from_slice(row_tracks);
        s.cols.clear();
        s.cols.extend_from_slice(col_tracks);
        s.col_sizes.clear();
        s.col_sizes.resize(n_cols, 0.0);
        s.col_resolved.clear();
        s.col_resolved.resize(n_cols, false);
        s.row_sizes.clear();
        s.row_sizes.resize(n_rows, 0.0);
        s.row_resolved.clear();
        s.row_resolved.resize(n_rows, false);
        s.hug_w.clear();
        s.hug_w.resize(n_cols, 0.0);
        s.hug_h.clear();
        s.hug_h.resize(n_rows, 0.0);
        // Resolve Fixed tracks (disjoint field borrows on `s`).
        for (i, t) in s.cols.iter().enumerate() {
            if let Sizing::Fixed(v) = t.size {
                s.col_sizes[i] = v.clamp(t.min, t.max);
                s.col_resolved[i] = true;
            }
        }
        for (i, t) in s.rows.iter().enumerate() {
            if let Sizing::Fixed(v) = t.size {
                s.row_sizes[i] = v.clamp(t.min, t.max);
                s.row_resolved[i] = true;
            }
        }
        (
            n_rows,
            n_cols,
            def.row_gap,
            def.col_gap,
            def.row_hugs,
            def.col_hugs,
        )
    };

    if n_rows == 0 || n_cols == 0 {
        // Still measure children so their `desired` is set.
        let mut kids = tree.child_cursor(node);
        while let Some(c) = kids.next(tree) {
            layout.measure(tree, c, Size::ZERO);
        }
        return Size::ZERO;
    }

    // Walk children: brief scratch borrows around each recursion.
    let mut kids = tree.child_cursor(node);
    while let Some(c) = kids.next(tree) {
        let collapsed = tree.node(c).element.visibility == Visibility::Collapsed;
        let cell = clamp_cell(tree.node(c).element.grid, n_rows, n_cols);

        let avail = {
            let s = layout.grid.at(depth);
            let avail_w = sum_spanned_known(&s.col_sizes, &s.col_resolved, cell.col, cell.col_span);
            let avail_h = sum_spanned_known(&s.row_sizes, &s.row_resolved, cell.row, cell.row_span);
            Size::new(avail_w, avail_h)
        };

        let d = layout.measure(tree, c, avail);
        if collapsed {
            continue;
        }

        // Span-1 only drives Hug-track sizing (avoids the WPF Auto↔Star
        // cyclic-iteration trap).
        let s = layout.grid.at(depth);
        if cell.col_span == 1 && matches!(s.cols[cell.col as usize].size, Sizing::Hug) {
            let i = cell.col as usize;
            s.hug_w[i] = s.hug_w[i].max(d.w);
        }
        if cell.row_span == 1 && matches!(s.rows[cell.row as usize].size, Sizing::Hug) {
            let i = cell.row as usize;
            s.hug_h[i] = s.hug_h[i].max(d.h);
        }
    }

    // Resolve Hug tracks from accumulated hug-w/hug-h, write through to the
    // tree's hug pool so `arrange` can read them without re-walking children,
    // and sum content size.
    let s = layout.grid.at(depth);
    for (i, t) in s.cols.iter().enumerate() {
        if matches!(t.size, Sizing::Hug) {
            s.col_sizes[i] = s.hug_w[i].clamp(t.min, t.max);
        }
    }
    for (i, t) in s.rows.iter().enumerate() {
        if matches!(t.size, Sizing::Hug) {
            s.row_sizes[i] = s.hug_h[i].clamp(t.min, t.max);
        }
    }
    let total_w = s.col_sizes.iter().sum::<f32>() + col_gap * n_cols.saturating_sub(1) as f32;
    let total_h = s.row_sizes.iter().sum::<f32>() + row_gap * n_rows.saturating_sub(1) as f32;

    // Persist hug arrays into the tree pool so `arrange` can read them
    // without re-walking children. `s` (borrow of layout) and `tree` are
    // independent objects, so both can be borrowed mutably here.
    tree.grid_hugs_mut(col_hugs_slice).copy_from_slice(&s.hug_w);
    tree.grid_hugs_mut(row_hugs_slice).copy_from_slice(&s.hug_h);

    Size::new(total_w, total_h)
}

pub(super) fn arrange(
    layout: &mut LayoutEngine,
    tree: &mut Tree,
    node: NodeId,
    inner: Rect,
    idx: u32,
) {
    let depth = layout.grid.enter();
    arrange_inner(layout, tree, node, inner, idx, depth);
    layout.grid.exit();
}

fn arrange_inner(
    layout: &mut LayoutEngine,
    tree: &mut Tree,
    node: NodeId,
    inner: Rect,
    idx: u32,
    depth: usize,
) {
    // Snapshot tracks + gaps + read cached hug arrays from the tree pool.
    let (n_rows, n_cols, row_gap, col_gap) = {
        let def = tree.grid_def(idx);
        let row_tracks: &[Track] = tree.grid_tracks(def.rows);
        let col_tracks: &[Track] = tree.grid_tracks(def.cols);
        let row_hugs: &[f32] = tree.grid_hugs(def.row_hugs);
        let col_hugs: &[f32] = tree.grid_hugs(def.col_hugs);
        let n_rows = row_tracks.len();
        let n_cols = col_tracks.len();
        let s = layout.grid.at(depth);
        s.rows.clear();
        s.rows.extend_from_slice(row_tracks);
        s.cols.clear();
        s.cols.extend_from_slice(col_tracks);
        s.hug_w.clear();
        s.hug_w.extend_from_slice(col_hugs);
        s.hug_h.clear();
        s.hug_h.extend_from_slice(row_hugs);
        (n_rows, n_cols, def.row_gap, def.col_gap)
    };

    if n_rows == 0 || n_cols == 0 {
        let mut kids = tree.child_cursor(node);
        while let Some(c) = kids.next(tree) {
            super::zero_subtree(tree, c, inner.min);
        }
        return;
    }

    // Resolve track sizes into `col_sizes`/`row_sizes` and compute offsets.
    {
        let s = layout.grid.at(depth);
        s.col_sizes.clear();
        s.col_sizes.resize(n_cols, 0.0);
        s.row_sizes.clear();
        s.row_sizes.resize(n_rows, 0.0);
        s.col_offsets.clear();
        s.col_offsets.resize(n_cols, 0.0);
        s.row_offsets.clear();
        s.row_offsets.resize(n_rows, 0.0);

        // resolve_axis_tracks needs &mut to flexible scratch + read of tracks/hugs
        // and write to col_sizes/row_sizes. Run per-axis.
        resolve_axis(s, Axis::Col, inner.size.w, col_gap);
        resolve_axis(s, Axis::Row, inner.size.h, row_gap);

        track_offsets(&s.col_sizes, col_gap, &mut s.col_offsets);
        track_offsets(&s.row_sizes, row_gap, &mut s.row_offsets);
    }

    let parent_layout = tree.node(node).element;
    let mut kids = tree.child_cursor(node);
    while let Some(c) = kids.next(tree) {
        if tree.node(c).element.visibility == Visibility::Collapsed {
            super::zero_subtree(tree, c, inner.min);
            continue;
        }
        let cell = clamp_cell(tree.node(c).element.grid, n_rows, n_cols);
        let s_node = tree.node(c).element;
        let d = tree.node(c).desired;

        let (slot_x, slot_y, slot_w, slot_h) = {
            let s = layout.grid.at(depth);
            let slot_x = s.col_offsets[cell.col as usize];
            let slot_y = s.row_offsets[cell.row as usize];
            let slot_w = span_size(&s.col_sizes, cell.col, cell.col_span, col_gap);
            let slot_h = span_size(&s.row_sizes, cell.row, cell.row_span, row_gap);
            (slot_x, slot_y, slot_w, slot_h)
        };

        // Grid: a child with no explicit alignment stretches to fill its cell
        // (WPF default). `place_axis` is told `auto_stretches = true` so Auto
        // collapses to Stretch even when the child isn't `Sizing::Fill`.
        let h_align = s_node.align.h.or(parent_layout.child_align.h).to_axis();
        let v_align = s_node.align.v.or(parent_layout.child_align.v).to_axis();
        let (w, x_off) = super::place_axis(h_align, s_node.size.w, d.w, slot_w, true);
        let (h, y_off) = super::place_axis(v_align, s_node.size.h, d.h, slot_h, true);

        let child_rect = Rect::new(
            inner.min.x + slot_x + x_off,
            inner.min.y + slot_y + y_off,
            w,
            h,
        );
        layout.arrange(tree, c, child_rect);
    }
}

fn clamp_cell(c: GridCell, n_rows: usize, n_cols: usize) -> GridCell {
    debug_assert!(n_rows > 0 && n_cols > 0);
    let row = (c.row as usize).min(n_rows - 1);
    let col = (c.col as usize).min(n_cols - 1);
    let row_span = (c.row_span.max(1) as usize).min(n_rows - row);
    let col_span = (c.col_span.max(1) as usize).min(n_cols - col);
    GridCell {
        row: row as u16,
        col: col as u16,
        row_span: row_span as u16,
        col_span: col_span as u16,
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

fn span_size(sizes: &[f32], start: u16, span: u16, gap: f32) -> f32 {
    let s = start as usize;
    let n = (span as usize).min(sizes.len() - s);
    let mut total: f32 = sizes[s..s + n].iter().sum();
    if n > 1 {
        total += gap * (n - 1) as f32;
    }
    total
}

#[derive(Copy, Clone)]
enum Axis {
    Col,
    Row,
}

/// Resolve track sizes on one axis into `s.col_sizes` / `s.row_sizes`. Fixed
/// and Hug tracks are clamped to `[min, max]` once. Star tracks split the
/// leftover proportionally to weight, using **constraint resolution by
/// exclusion** (CSS Grid / Flutter flex): any star whose proportional share
/// violates `[min, max]` clamps and exits the pool, the remaining stars
/// rebalance, repeat until stable. Bounded — each iteration removes at least
/// one star, so O(N²) worst case.
fn resolve_axis(s: &mut GridScratch, axis: Axis, total: f32, gap: f32) {
    let (tracks, hug_sizes, out) = match axis {
        Axis::Col => (s.cols.as_slice(), s.hug_w.as_slice(), &mut s.col_sizes),
        Axis::Row => (s.rows.as_slice(), s.hug_h.as_slice(), &mut s.row_sizes),
    };
    let n = tracks.len();
    debug_assert_eq!(hug_sizes.len(), n);
    debug_assert_eq!(out.len(), n);
    out.fill(0.0);

    let mut consumed = gap * n.saturating_sub(1) as f32;
    s.flexible.clear();
    let mut flexible_weight = 0.0f32;

    for (i, t) in tracks.iter().enumerate() {
        match t.size {
            Sizing::Fixed(v) => {
                out[i] = v.clamp(t.min, t.max);
                consumed += out[i];
            }
            Sizing::Hug => {
                out[i] = hug_sizes[i].clamp(t.min, t.max);
                consumed += out[i];
            }
            Sizing::Fill(w) => {
                s.flexible.push(i);
                flexible_weight += w.max(0.0);
            }
        }
    }

    let mut remaining = (total - consumed).max(0.0);

    'outer: while !s.flexible.is_empty() && flexible_weight > 0.0 {
        let mut k = 0;
        while k < s.flexible.len() {
            let i = s.flexible[k];
            let t = &tracks[i];
            let w = match t.size {
                Sizing::Fill(w) => w.max(0.0),
                _ => unreachable!(),
            };
            let candidate = remaining * w / flexible_weight;
            if candidate < t.min || candidate > t.max {
                let clamped = candidate.clamp(t.min, t.max);
                out[i] = clamped;
                remaining = (remaining - clamped).max(0.0);
                flexible_weight -= w;
                s.flexible.remove(k);
                continue 'outer;
            }
            k += 1;
        }
        for &i in s.flexible.iter() {
            let w = match tracks[i].size {
                Sizing::Fill(w) => w.max(0.0),
                _ => unreachable!(),
            };
            out[i] = remaining * w / flexible_weight;
        }
        break;
    }
}

#[cfg(test)]
mod tests;
