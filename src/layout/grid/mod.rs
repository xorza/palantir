use crate::primitives::{GridCell, MAX_TRACKS, Rect, Size, Sizing, Track, TrackSlice, Visibility};
use crate::tree::{NodeId, Tree};

/// Stack-resident snapshot of a `GridDef`. Holds the row/col track arrays
/// (copied off the tree's track pool into stack buffers so the `&Tree` borrow
/// can be dropped before measure/arrange recurses with `&mut Tree`) plus the
/// per-track hug-pool slices.
struct Snapshot<'a> {
    rows: &'a [Track],
    cols: &'a [Track],
    row_gap: f32,
    col_gap: f32,
    row_hugs: TrackSlice,
    col_hugs: TrackSlice,
}

fn snapshot<'a>(
    tree: &Tree,
    idx: u32,
    rows_buf: &'a mut [Track; MAX_TRACKS],
    cols_buf: &'a mut [Track; MAX_TRACKS],
) -> Snapshot<'a> {
    let def = tree.grid_def(idx);
    let n_rows = def.rows.len as usize;
    let n_cols = def.cols.len as usize;
    rows_buf[..n_rows].copy_from_slice(tree.grid_tracks(def.rows));
    cols_buf[..n_cols].copy_from_slice(tree.grid_tracks(def.cols));
    Snapshot {
        rows: &rows_buf[..n_rows],
        cols: &cols_buf[..n_cols],
        row_gap: def.row_gap,
        col_gap: def.col_gap,
        row_hugs: def.row_hugs,
        col_hugs: def.col_hugs,
    }
}

/// WPF-style grid measure. Resolves Fixed tracks, walks children once feeding
/// each `Σ spanned-track sizes` (or `∞` if any spanned track is unresolved —
/// the WPF infinity trick → child reports intrinsic), then resolves Hug
/// tracks from span-1 children's desired sizes. Star tracks contribute 0 to
/// the grid's content size — final star sizes only resolve in arrange. See
/// `docs/grid.md`.
///
/// All scratch is stack-allocated, capped at `MAX_TRACKS` per axis. Nested
/// grids each get their own copy via the call stack — no shared buffer. The
/// per-track hug sizes are written through to `Tree::hug_pool` so
/// `arrange` can read them without re-walking children.
pub(super) fn measure(tree: &mut Tree, node: NodeId, idx: u32) -> Size {
    let mut rows_buf = [Track::new(Sizing::Hug); MAX_TRACKS];
    let mut cols_buf = [Track::new(Sizing::Hug); MAX_TRACKS];
    let snap = snapshot(tree, idx, &mut rows_buf, &mut cols_buf);
    let n_rows = snap.rows.len();
    let n_cols = snap.cols.len();

    if snap.rows.is_empty() || snap.cols.is_empty() {
        // Still measure children so their `desired` is set, then yield zero
        // — arrange will give them zero rects too.
        let mut kids = tree.child_cursor(node);
        while let Some(c) = kids.next(tree) {
            super::measure(tree, c, Size::ZERO);
        }
        return Size::ZERO;
    }

    let mut col_sizes = [0.0f32; MAX_TRACKS];
    let mut col_resolved = [false; MAX_TRACKS];
    let mut row_sizes = [0.0f32; MAX_TRACKS];
    let mut row_resolved = [false; MAX_TRACKS];
    for (i, t) in snap.cols.iter().enumerate() {
        if let Sizing::Fixed(v) = t.size {
            col_sizes[i] = v.clamp(t.min, t.max);
            col_resolved[i] = true;
        }
    }
    for (i, t) in snap.rows.iter().enumerate() {
        if let Sizing::Fixed(v) = t.size {
            row_sizes[i] = v.clamp(t.min, t.max);
            row_resolved[i] = true;
        }
    }

    let mut hug_w = [0.0f32; MAX_TRACKS];
    let mut hug_h = [0.0f32; MAX_TRACKS];

    let mut kids = tree.child_cursor(node);
    while let Some(c) = kids.next(tree) {
        let collapsed = tree.node(c).element.visibility == Visibility::Collapsed;
        let cell = clamp_cell(tree.node(c).element.grid, n_rows, n_cols);

        let avail_w = sum_spanned_known(
            &col_sizes[..n_cols],
            &col_resolved[..n_cols],
            cell.col,
            cell.col_span,
        );
        let avail_h = sum_spanned_known(
            &row_sizes[..n_rows],
            &row_resolved[..n_rows],
            cell.row,
            cell.row_span,
        );
        let d = super::measure(tree, c, Size::new(avail_w, avail_h));
        if collapsed {
            continue;
        }
        // Span-1 only: drives Hug-track sizing. Span >1 deliberately sits out
        // (avoids the WPF Auto↔Star cyclic-iteration trap).
        if cell.col_span == 1 && matches!(snap.cols[cell.col as usize].size, Sizing::Hug) {
            let i = cell.col as usize;
            hug_w[i] = hug_w[i].max(d.w);
        }
        if cell.row_span == 1 && matches!(snap.rows[cell.row as usize].size, Sizing::Hug) {
            let i = cell.row as usize;
            hug_h[i] = hug_h[i].max(d.h);
        }
    }

    for (i, t) in snap.cols.iter().enumerate() {
        if matches!(t.size, Sizing::Hug) {
            col_sizes[i] = hug_w[i].clamp(t.min, t.max);
        }
    }
    for (i, t) in snap.rows.iter().enumerate() {
        if matches!(t.size, Sizing::Hug) {
            row_sizes[i] = hug_h[i].clamp(t.min, t.max);
        }
    }

    // Cache hug arrays so `arrange` can call `resolve_axis_tracks`
    // without re-walking children to recompute them.
    tree.grid_hugs_mut(snap.col_hugs)
        .copy_from_slice(&hug_w[..n_cols]);
    tree.grid_hugs_mut(snap.row_hugs)
        .copy_from_slice(&hug_h[..n_rows]);

    // Star tracks contribute 0 to grid's content — Hug parent collapses them
    // (matches WPF). If parent gives the grid Fill space, arrange expands.
    let total_w =
        col_sizes[..n_cols].iter().sum::<f32>() + snap.col_gap * n_cols.saturating_sub(1) as f32;
    let total_h =
        row_sizes[..n_rows].iter().sum::<f32>() + snap.row_gap * n_rows.saturating_sub(1) as f32;
    Size::new(total_w, total_h)
}

pub(super) fn arrange(tree: &mut Tree, node: NodeId, inner: Rect, idx: u32) {
    let mut rows_buf = [Track::new(Sizing::Hug); MAX_TRACKS];
    let mut cols_buf = [Track::new(Sizing::Hug); MAX_TRACKS];
    let snap = snapshot(tree, idx, &mut rows_buf, &mut cols_buf);
    let n_rows = snap.rows.len();
    let n_cols = snap.cols.len();

    if snap.rows.is_empty() || snap.cols.is_empty() {
        let mut kids = tree.child_cursor(node);
        while let Some(c) = kids.next(tree) {
            super::zero_subtree(tree, c, inner.min);
        }
        return;
    }

    // Hug arrays were cached by `measure`; copy them onto the stack so
    // `resolve_axis_tracks` can read alongside the &mut Tree we'll need below.
    let mut hug_w = [0.0f32; MAX_TRACKS];
    let mut hug_h = [0.0f32; MAX_TRACKS];
    hug_w[..n_cols].copy_from_slice(tree.grid_hugs(snap.col_hugs));
    hug_h[..n_rows].copy_from_slice(tree.grid_hugs(snap.row_hugs));

    let mut col_sizes = [0.0f32; MAX_TRACKS];
    let mut row_sizes = [0.0f32; MAX_TRACKS];
    resolve_axis_tracks(
        snap.cols,
        inner.size.w,
        snap.col_gap,
        &hug_w[..n_cols],
        &mut col_sizes[..n_cols],
    );
    resolve_axis_tracks(
        snap.rows,
        inner.size.h,
        snap.row_gap,
        &hug_h[..n_rows],
        &mut row_sizes[..n_rows],
    );
    let mut col_offsets = [0.0f32; MAX_TRACKS];
    let mut row_offsets = [0.0f32; MAX_TRACKS];
    track_offsets(
        &col_sizes[..n_cols],
        snap.col_gap,
        &mut col_offsets[..n_cols],
    );
    track_offsets(
        &row_sizes[..n_rows],
        snap.row_gap,
        &mut row_offsets[..n_rows],
    );

    let parent_layout = tree.node(node).element;
    let mut kids = tree.child_cursor(node);
    while let Some(c) = kids.next(tree) {
        if tree.node(c).element.visibility == Visibility::Collapsed {
            super::zero_subtree(tree, c, inner.min);
            continue;
        }
        let cell = clamp_cell(tree.node(c).element.grid, n_rows, n_cols);
        let s = tree.node(c).element;
        let d = tree.node(c).desired;

        let slot_x = col_offsets[cell.col as usize];
        let slot_y = row_offsets[cell.row as usize];
        let slot_w = span_size(&col_sizes[..n_cols], cell.col, cell.col_span, snap.col_gap);
        let slot_h = span_size(&row_sizes[..n_rows], cell.row, cell.row_span, snap.row_gap);

        // Grid: a child with no explicit alignment stretches to fill its cell
        // (WPF default). `place_axis` is told `auto_stretches = true` so Auto
        // collapses to Stretch even when the child isn't `Sizing::Fill`.
        let h_align = s.align.h.or(parent_layout.child_align.h).to_axis();
        let v_align = s.align.v.or(parent_layout.child_align.v).to_axis();
        let (w, x_off) = super::place_axis(h_align, s.size.w, d.w, slot_w, true);
        let (h, y_off) = super::place_axis(v_align, s.size.h, d.h, slot_h, true);

        let child_rect = Rect::new(
            inner.min.x + slot_x + x_off,
            inner.min.y + slot_y + y_off,
            w,
            h,
        );
        super::arrange(tree, c, child_rect);
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

/// Resolve track sizes for one axis. Fixed and Hug tracks are clamped to
/// `[min, max]` once. Star tracks split the leftover proportionally to weight,
/// using **constraint resolution by exclusion** (CSS Grid / Flutter flex):
/// any star whose proportional share violates `[min, max]` clamps and exits
/// the pool, the remaining stars rebalance, repeat until stable. Bounded —
/// each iteration removes at least one star, so O(N²) worst case.
fn resolve_axis_tracks(tracks: &[Track], total: f32, gap: f32, hug_sizes: &[f32], out: &mut [f32]) {
    let n = tracks.len();
    debug_assert_eq!(hug_sizes.len(), n);
    debug_assert_eq!(out.len(), n);
    out.fill(0.0);
    let mut consumed = gap * n.saturating_sub(1) as f32;
    // Stack-allocated indices of unresolved Fill tracks. Order is preserved
    // when we remove an entry (shift-remove) so iteration stays deterministic.
    let mut flexible: [usize; MAX_TRACKS] = [0; MAX_TRACKS];
    let mut flex_len = 0usize;
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
                flexible[flex_len] = i;
                flex_len += 1;
                flexible_weight += w.max(0.0);
            }
        }
    }

    let mut remaining = (total - consumed).max(0.0);

    'outer: while flex_len > 0 && flexible_weight > 0.0 {
        let mut k = 0;
        while k < flex_len {
            let i = flexible[k];
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
                // Shift-remove flexible[k] to keep order stable.
                flexible.copy_within(k + 1..flex_len, k);
                flex_len -= 1;
                continue 'outer;
            }
            k += 1;
        }
        // No track was clamped this iteration → assign candidates and exit.
        for &i in flexible.iter().take(flex_len) {
            let w = match tracks[i].size {
                Sizing::Fill(w) => w.max(0.0),
                _ => unreachable!(),
            };
            out[i] = remaining * w / flexible_weight;
        }
        break;
    }
    // Any leftover flexible items with zero weight collapse to 0 (already set
    // by `out.fill(0.0)`).
}

#[cfg(test)]
mod tests;
