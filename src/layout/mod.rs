use crate::element::{LayoutMode, UiElement};
use crate::primitives::{
    AxisAlign, GridCell, Justify, Rect, Size, Sizes, Sizing, Track, Visibility,
};
use crate::shape::Shape;
use crate::tree::{NodeId, Tree};
use glam::Vec2;

/// Run measure + arrange for `root` given the surface rect.
pub fn run(tree: &mut Tree, root: NodeId, surface: Rect) {
    measure(tree, root, Size::new(surface.width(), surface.height()));
    arrange(tree, root, surface);
}

/// Bottom-up. Returns the node's desired *slot* size (including its own margin)
/// and stores it on the node.
fn measure(tree: &mut Tree, node: NodeId, available: Size) -> Size {
    if tree.node(node).element.visibility == Visibility::Collapsed {
        tree.node_mut(node).desired = Size::ZERO;
        return Size::ZERO;
    }
    let style = tree.node(node).element;
    let mode = tree.node(node).element.mode;

    // Inner available = available minus margin minus padding.
    let inner_avail = Size::new(
        (available.w - style.margin.horiz() - style.padding.horiz()).max(0.0),
        (available.h - style.margin.vert() - style.padding.vert()).max(0.0),
    );

    let content = match mode {
        LayoutMode::Leaf => leaf_content_size(tree, node),
        LayoutMode::HStack => stack_measure(tree, node, inner_avail, Axis::X),
        LayoutMode::VStack => stack_measure(tree, node, inner_avail, Axis::Y),
        LayoutMode::ZStack => zstack_measure(tree, node),
        LayoutMode::Canvas => canvas_measure(tree, node),
        LayoutMode::Grid(idx) => grid_measure(tree, node, idx),
    };

    let hug_w = content.w + style.padding.horiz() + style.margin.horiz();
    let hug_h = content.h + style.padding.vert() + style.margin.vert();
    let desired = Size::new(
        resolve_axis_size(
            style.size.w,
            hug_w,
            available.w,
            style.margin.horiz(),
            style.min_size.w,
            style.max_size.w,
        ),
        resolve_axis_size(
            style.size.h,
            hug_h,
            available.h,
            style.margin.vert(),
            style.min_size.h,
            style.max_size.h,
        ),
    );

    tree.node_mut(node).desired = desired;
    desired
}

/// Top-down. `slot` is the rect the parent reserved (including this node's margin).
fn arrange(tree: &mut Tree, node: NodeId, slot: Rect) {
    if tree.node(node).element.visibility == Visibility::Collapsed {
        tree.node_mut(node).rect = Rect {
            min: slot.min,
            size: Size::ZERO,
        };
        return;
    }
    let style = tree.node(node).element;
    let mode = tree.node(node).element.mode;

    let rendered = slot.deflated_by(style.margin);
    tree.node_mut(node).rect = rendered;
    let inner = rendered.deflated_by(style.padding);

    match mode {
        LayoutMode::Leaf => {}
        LayoutMode::HStack => arrange_stack(tree, node, inner, Axis::X),
        LayoutMode::VStack => arrange_stack(tree, node, inner, Axis::Y),
        LayoutMode::ZStack => arrange_zstack(tree, node, inner),
        LayoutMode::Canvas => arrange_canvas(tree, node, inner),
        LayoutMode::Grid(idx) => arrange_grid(tree, node, inner, idx),
    }
}

/// Resolve a node's outer slot size on one axis, given its sizing policy,
/// hug-content size, parent-supplied available, own margin, and clamps.
fn resolve_axis_size(
    s: Sizing,
    hug_outer: f32,
    available: f32,
    margin: f32,
    min: f32,
    max: f32,
) -> f32 {
    let slot = match s {
        Sizing::Fixed(v) => v + margin,
        Sizing::Hug => hug_outer,
        Sizing::Fill(_) => {
            if available.is_finite() {
                available
            } else {
                hug_outer
            }
        }
    };
    let rendered = (slot - margin).max(0.0).clamp(min, max);
    rendered + margin
}

fn leaf_content_size(tree: &Tree, node: NodeId) -> Size {
    // For a Leaf, content size = bounding box of any Text shapes' measured size,
    // or zero. Other shapes are owner-relative and don't drive size.
    let mut s = Size::ZERO;
    for sh in tree.shapes_of(node) {
        if let Shape::Text { measured, .. } = sh {
            s = s.max(*measured);
        }
    }
    s
}

#[derive(Copy, Clone, PartialEq)]
enum Axis {
    X,
    Y,
}

impl Axis {
    fn main(self, s: Size) -> f32 {
        match self {
            Axis::X => s.w,
            Axis::Y => s.h,
        }
    }
    fn cross(self, s: Size) -> f32 {
        match self {
            Axis::X => s.h,
            Axis::Y => s.w,
        }
    }
    fn main_v(self, v: Vec2) -> f32 {
        match self {
            Axis::X => v.x,
            Axis::Y => v.y,
        }
    }
    fn cross_v(self, v: Vec2) -> f32 {
        match self {
            Axis::X => v.y,
            Axis::Y => v.x,
        }
    }
    fn main_sizing(self, s: Sizes) -> Sizing {
        match self {
            Axis::X => s.w,
            Axis::Y => s.h,
        }
    }
    fn cross_sizing(self, s: Sizes) -> Sizing {
        match self {
            Axis::X => s.h,
            Axis::Y => s.w,
        }
    }
    /// Cross-axis alignment of a child, with parent's `child_align` as
    /// fallback when the child's own align is `Auto`. Mapped through
    /// `AxisAlign` so the math is type-symmetric across axes.
    fn cross_align(self, child: &UiElement, parent: &UiElement) -> AxisAlign {
        match self {
            // HStack: cross = vertical
            Axis::X => child.align.v.or(parent.child_align.v).to_axis(),
            // VStack: cross = horizontal
            Axis::Y => child.align.h.or(parent.child_align.h).to_axis(),
        }
    }
    /// Build a `Size` from main- and cross-axis lengths.
    fn compose_size(self, main: f32, cross: f32) -> Size {
        match self {
            Axis::X => Size::new(main, cross),
            Axis::Y => Size::new(cross, main),
        }
    }
    /// Build a `Rect` from main- and cross-axis positions and lengths.
    fn compose_rect(self, main_pos: f32, cross_pos: f32, main: f32, cross: f32) -> Rect {
        match self {
            Axis::X => Rect::new(main_pos, cross_pos, main, cross),
            Axis::Y => Rect::new(cross_pos, main_pos, cross, main),
        }
    }
}

fn stack_measure(tree: &mut Tree, node: NodeId, inner: Size, axis: Axis) -> Size {
    // Pass infinite size on the main axis (WPF trick): children report intrinsic.
    let child_avail = axis.compose_size(f32::INFINITY, axis.cross(inner));
    let gap = tree.node(node).element.gap;

    let mut total_main = 0.0f32;
    let mut max_cross = 0.0f32;
    let mut count = 0usize;
    let mut kids = tree.child_cursor(node);
    while let Some(c) = kids.next(tree) {
        // Collapsed children still get measured (so `desired` is set to ZERO),
        // but don't contribute to the parent's content size or gap count.
        let collapsed = tree.node(c).element.visibility == Visibility::Collapsed;
        let d = measure(tree, c, child_avail);
        if collapsed {
            continue;
        }
        total_main += axis.main(d);
        max_cross = max_cross.max(axis.cross(d));
        count += 1;
    }
    total_main += gap * count.saturating_sub(1) as f32;
    axis.compose_size(total_main, max_cross)
}

fn arrange_stack(tree: &mut Tree, node: NodeId, inner: Rect, axis: Axis) {
    let parent_layout = tree.node(node).element;
    let gap = parent_layout.gap;
    let justify = parent_layout.justify;

    // Sum desired along main axis for non-Fill children; collect Fill weights.
    // Fill siblings split the remaining space proportionally (WPF Star semantics)
    // independent of their intrinsic content size.
    let mut sum_main_desired = 0.0f32;
    let mut total_weight = 0.0f32;
    let mut count = 0usize;
    let mut kids = tree.child_cursor(node);
    while let Some(c) = kids.next(tree) {
        let n = tree.node(c);
        if n.element.visibility == Visibility::Collapsed {
            continue;
        }
        if let Sizing::Fill(weight) = axis.main_sizing(n.element.size) {
            total_weight += weight.max(0.0);
        } else {
            sum_main_desired += axis.main(n.desired);
        }
        count += 1;
    }
    let total_gap = gap * count.saturating_sub(1) as f32;

    let main_total = axis.main(inner.size);
    let cross = axis.cross(inner.size);
    let leftover = (main_total - sum_main_desired - total_gap).max(0.0);

    // `justify` distributes any unused main-axis space. With Fill children
    // present, leftover is consumed by Fill weights → justify is a no-op
    // (degrade to Start / original gap).
    let (start_offset, effective_gap) = if total_weight > 0.0 {
        (0.0, gap)
    } else {
        match justify {
            Justify::Start => (0.0, gap),
            Justify::Center => (leftover * 0.5, gap),
            Justify::End => (leftover, gap),
            Justify::SpaceBetween if count > 1 => (0.0, gap + leftover / (count - 1) as f32),
            Justify::SpaceAround if count > 0 => {
                let extra = leftover / count as f32;
                (extra * 0.5, gap + extra)
            }
            // Fewer than 2 / 1 children → fallback to Start.
            Justify::SpaceBetween | Justify::SpaceAround => (0.0, gap),
        }
    };

    let cross_min = axis.cross_v(inner.min);
    let mut cursor = axis.main_v(inner.min) + start_offset;
    let mut first = true;

    let mut kids = tree.child_cursor(node);
    while let Some(c) = kids.next(tree) {
        if tree.node(c).element.visibility == Visibility::Collapsed {
            // Give Collapsed children a zero rect at the cursor so they exist
            // in the tree but consume no space, no gap, no fill weight.
            arrange(
                tree,
                c,
                Rect {
                    min: axis.compose_rect(cursor, cross_min, 0.0, 0.0).min,
                    size: Size::ZERO,
                },
            );
            continue;
        }
        if !first {
            cursor += effective_gap;
        }
        first = false;
        let d = tree.node(c).desired;
        let s = tree.node(c).element;

        let main_sizing = axis.main_sizing(s.size);
        let main_size = match main_sizing {
            Sizing::Fill(weight) if total_weight > 0.0 => {
                leftover * (weight.max(0.0) / total_weight)
            }
            _ => axis.main(d),
        };

        let cross_align = axis.cross_align(&s, &parent_layout);
        let cross_sizing = axis.cross_sizing(s.size);
        let cross_desired = axis.cross(d);
        let (cross_size, cross_offset) =
            place_axis(cross_align, cross_sizing, cross_desired, cross);

        let child_rect = axis.compose_rect(cursor, cross_min + cross_offset, main_size, cross_size);
        arrange(tree, c, child_rect);
        cursor += main_size;
    }
}

/// ZStack: children all at the same position (top-left of inner rect).
/// Pass `INFINITY` on both axes during measure so `Fill` children fall back to
/// intrinsic — otherwise the `Hug` panel would size to its own `Fill` children
/// (recursive). Content size = `max(child desired)` per axis, so the panel
/// hugs the largest child.
fn zstack_measure(tree: &mut Tree, node: NodeId) -> Size {
    let child_avail = Size::INF;
    let mut max_w = 0.0f32;
    let mut max_h = 0.0f32;
    let mut kids = tree.child_cursor(node);
    while let Some(c) = kids.next(tree) {
        let d = measure(tree, c, child_avail);
        max_w = max_w.max(d.w);
        max_h = max_h.max(d.h);
    }
    Size::new(max_w, max_h)
}

/// Canvas: children placed at their declared `Layout.position` (parent-inner
/// coords, defaulting to `(0, 0)`). Pass `INFINITY` on both axes during measure
/// so `Fill` children fall back to intrinsic — "fill the rest" is meaningless
/// when children can overlap. Content size = `max(child_pos + child_desired)`
/// per axis, so a `Hug` Canvas grows to the union of placed rects.
fn canvas_measure(tree: &mut Tree, node: NodeId) -> Size {
    let child_avail = Size::INF;
    let mut max_w = 0.0f32;
    let mut max_h = 0.0f32;
    let mut kids = tree.child_cursor(node);
    while let Some(c) = kids.next(tree) {
        let pos = tree.node(c).element.position;
        let d = measure(tree, c, child_avail);
        max_w = max_w.max(pos.x + d.w);
        max_h = max_h.max(pos.y + d.h);
    }
    Size::new(max_w, max_h)
}

/// Each child gets a slot at `inner.min + style.position`, sized per its
/// desired (intrinsic) size. `Fill` falls back to intrinsic — same reason as
/// `canvas_measure`.
fn arrange_canvas(tree: &mut Tree, node: NodeId, inner: Rect) {
    let mut kids = tree.child_cursor(node);
    while let Some(c) = kids.next(tree) {
        let d = tree.node(c).desired;
        let pos = tree.node(c).element.position;
        let child_rect = Rect {
            min: inner.min + pos,
            size: d,
        };
        arrange(tree, c, child_rect);
    }
}

/// Each child gets a slot inside `inner`, sized per its own `Sizing` and
/// positioned per its `align_x` / `align_y` (with the ZStack's
/// `child_align` as fallback when child's own axis is `Auto`).
/// Defaults pin to top-left unless the child has `Sizing::Fill` — then `Auto`
/// falls back to stretch on that axis.
fn arrange_zstack(tree: &mut Tree, node: NodeId, inner: Rect) {
    let parent_layout = tree.node(node).element;
    let mut kids = tree.child_cursor(node);
    while let Some(c) = kids.next(tree) {
        let d = tree.node(c).desired;
        let s = tree.node(c).element;

        let h_align = s.align.h.or(parent_layout.child_align.h).to_axis();
        let v_align = s.align.v.or(parent_layout.child_align.v).to_axis();
        let (w, x_off) = place_axis(h_align, s.size.w, d.w, inner.size.w);
        let (h, y_off) = place_axis(v_align, s.size.h, d.h, inner.size.h);

        let child_rect = Rect::new(inner.min.x + x_off, inner.min.y + y_off, w, h);
        arrange(tree, c, child_rect);
    }
}

/// WPF-style grid measure. Resolves Fixed tracks, walks children once feeding
/// each `Σ spanned-track sizes` (or `∞` if any spanned track is unresolved —
/// the WPF infinity trick → child reports intrinsic), then resolves Hug
/// tracks from span-1 children's desired sizes. Star tracks contribute 0 to
/// the grid's content size — final star sizes only resolve in arrange. See
/// `docs/grid.md`.
fn grid_measure(tree: &mut Tree, node: NodeId, idx: u32) -> Size {
    let def = tree.grid_def(idx).clone();
    if def.rows.is_empty() || def.cols.is_empty() {
        // Still measure children so their `desired` is set, then yield zero
        // — arrange will give them zero rects too.
        let mut kids = tree.child_cursor(node);
        while let Some(c) = kids.next(tree) {
            measure(tree, c, Size::ZERO);
        }
        return Size::ZERO;
    }
    let n_rows = def.rows.len();
    let n_cols = def.cols.len();

    let mut col_sizes = vec![0.0f32; n_cols];
    let mut col_resolved = vec![false; n_cols];
    let mut row_sizes = vec![0.0f32; n_rows];
    let mut row_resolved = vec![false; n_rows];
    for (i, t) in def.cols.iter().enumerate() {
        if let Sizing::Fixed(v) = t.size {
            col_sizes[i] = v.clamp(t.min, t.max);
            col_resolved[i] = true;
        }
    }
    for (i, t) in def.rows.iter().enumerate() {
        if let Sizing::Fixed(v) = t.size {
            row_sizes[i] = v.clamp(t.min, t.max);
            row_resolved[i] = true;
        }
    }

    let mut hug_w = vec![0.0f32; n_cols];
    let mut hug_h = vec![0.0f32; n_rows];

    let mut kids = tree.child_cursor(node);
    while let Some(c) = kids.next(tree) {
        let collapsed = tree.node(c).element.visibility == Visibility::Collapsed;
        let cell = clamp_grid_cell(tree.node(c).element.grid, n_rows, n_cols);

        let avail_w = sum_spanned_known(&col_sizes, &col_resolved, cell.col, cell.col_span);
        let avail_h = sum_spanned_known(&row_sizes, &row_resolved, cell.row, cell.row_span);
        let d = measure(tree, c, Size::new(avail_w, avail_h));
        if collapsed {
            continue;
        }
        // Span-1 only: drives Hug-track sizing. Span >1 deliberately sits out
        // (avoids the WPF Auto↔Star cyclic-iteration trap).
        if cell.col_span == 1 && matches!(def.cols[cell.col as usize].size, Sizing::Hug) {
            let i = cell.col as usize;
            hug_w[i] = hug_w[i].max(d.w);
        }
        if cell.row_span == 1 && matches!(def.rows[cell.row as usize].size, Sizing::Hug) {
            let i = cell.row as usize;
            hug_h[i] = hug_h[i].max(d.h);
        }
    }

    for (i, t) in def.cols.iter().enumerate() {
        if matches!(t.size, Sizing::Hug) {
            col_sizes[i] = hug_w[i].clamp(t.min, t.max);
        }
    }
    for (i, t) in def.rows.iter().enumerate() {
        if matches!(t.size, Sizing::Hug) {
            row_sizes[i] = hug_h[i].clamp(t.min, t.max);
        }
    }

    // Star tracks contribute 0 to grid's content — Hug parent collapses them
    // (matches WPF). If parent gives the grid Fill space, arrange expands.
    let total_w = col_sizes.iter().sum::<f32>() + def.col_gap * n_cols.saturating_sub(1) as f32;
    let total_h = row_sizes.iter().sum::<f32>() + def.row_gap * n_rows.saturating_sub(1) as f32;
    Size::new(total_w, total_h)
}

fn arrange_grid(tree: &mut Tree, node: NodeId, inner: Rect, idx: u32) {
    let def = tree.grid_def(idx).clone();
    if def.rows.is_empty() || def.cols.is_empty() {
        let mut kids = tree.child_cursor(node);
        while let Some(c) = kids.next(tree) {
            arrange(
                tree,
                c,
                Rect {
                    min: inner.min,
                    size: Size::ZERO,
                },
            );
        }
        return;
    }
    let n_rows = def.rows.len();
    let n_cols = def.cols.len();

    let mut hug_w = vec![0.0f32; n_cols];
    let mut hug_h = vec![0.0f32; n_rows];
    let mut kids = tree.child_cursor(node);
    while let Some(c) = kids.next(tree) {
        let n = tree.node(c);
        if n.element.visibility == Visibility::Collapsed {
            continue;
        }
        let cell = clamp_grid_cell(n.element.grid, n_rows, n_cols);
        if cell.col_span == 1 && matches!(def.cols[cell.col as usize].size, Sizing::Hug) {
            let i = cell.col as usize;
            hug_w[i] = hug_w[i].max(n.desired.w);
        }
        if cell.row_span == 1 && matches!(def.rows[cell.row as usize].size, Sizing::Hug) {
            let i = cell.row as usize;
            hug_h[i] = hug_h[i].max(n.desired.h);
        }
    }

    let col_sizes = resolve_axis_tracks(&def.cols, inner.size.w, def.col_gap, &hug_w);
    let row_sizes = resolve_axis_tracks(&def.rows, inner.size.h, def.row_gap, &hug_h);
    let col_starts = cum_starts(&col_sizes, def.col_gap);
    let row_starts = cum_starts(&row_sizes, def.row_gap);

    let parent_layout = tree.node(node).element;
    let mut kids = tree.child_cursor(node);
    while let Some(c) = kids.next(tree) {
        if tree.node(c).element.visibility == Visibility::Collapsed {
            arrange(
                tree,
                c,
                Rect {
                    min: inner.min,
                    size: Size::ZERO,
                },
            );
            continue;
        }
        let cell = clamp_grid_cell(tree.node(c).element.grid, n_rows, n_cols);
        let s = tree.node(c).element;
        let d = tree.node(c).desired;

        let slot_x = col_starts[cell.col as usize];
        let slot_y = row_starts[cell.row as usize];
        let slot_w = span_size(&col_sizes, cell.col, cell.col_span, def.col_gap);
        let slot_h = span_size(&row_sizes, cell.row, cell.row_span, def.row_gap);

        // WPF-default behavior: a Grid child with no explicit alignment
        // stretches to fill its cell. `place_axis` would otherwise leave a
        // Hug child at desired size in the slot's top-left corner.
        let h_align = s.align.h.or(parent_layout.child_align.h).to_axis();
        let v_align = s.align.v.or(parent_layout.child_align.v).to_axis();
        let h_align = if matches!(h_align, AxisAlign::Auto) {
            AxisAlign::Stretch
        } else {
            h_align
        };
        let v_align = if matches!(v_align, AxisAlign::Auto) {
            AxisAlign::Stretch
        } else {
            v_align
        };
        let (w, x_off) = place_axis(h_align, s.size.w, d.w, slot_w);
        let (h, y_off) = place_axis(v_align, s.size.h, d.h, slot_h);

        let child_rect = Rect::new(
            inner.min.x + slot_x + x_off,
            inner.min.y + slot_y + y_off,
            w,
            h,
        );
        arrange(tree, c, child_rect);
    }
}

fn clamp_grid_cell(c: GridCell, n_rows: usize, n_cols: usize) -> GridCell {
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

fn cum_starts(sizes: &[f32], gap: f32) -> Vec<f32> {
    let mut starts = Vec::with_capacity(sizes.len());
    let mut acc = 0.0f32;
    for (i, &s) in sizes.iter().enumerate() {
        starts.push(acc);
        acc += s;
        if i + 1 < sizes.len() {
            acc += gap;
        }
    }
    starts
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
fn resolve_axis_tracks(tracks: &[Track], total: f32, gap: f32, hug_sizes: &[f32]) -> Vec<f32> {
    let n = tracks.len();
    let mut sizes = vec![0.0f32; n];
    let mut consumed = gap * n.saturating_sub(1) as f32;
    let mut flexible: Vec<usize> = Vec::new();
    let mut flexible_weight = 0.0f32;

    for (i, t) in tracks.iter().enumerate() {
        match t.size {
            Sizing::Fixed(v) => {
                sizes[i] = v.clamp(t.min, t.max);
                consumed += sizes[i];
            }
            Sizing::Hug => {
                sizes[i] = hug_sizes[i].clamp(t.min, t.max);
                consumed += sizes[i];
            }
            Sizing::Fill(w) => {
                flexible.push(i);
                flexible_weight += w.max(0.0);
            }
        }
    }

    let mut remaining = (total - consumed).max(0.0);

    'outer: while !flexible.is_empty() && flexible_weight > 0.0 {
        for k in 0..flexible.len() {
            let i = flexible[k];
            let t = &tracks[i];
            let w = match t.size {
                Sizing::Fill(w) => w.max(0.0),
                _ => unreachable!(),
            };
            let candidate = remaining * w / flexible_weight;
            if candidate < t.min {
                sizes[i] = t.min;
                remaining = (remaining - t.min).max(0.0);
                flexible_weight -= w;
                flexible.remove(k);
                continue 'outer;
            }
            if candidate > t.max {
                sizes[i] = t.max;
                remaining = (remaining - t.max).max(0.0);
                flexible_weight -= w;
                flexible.remove(k);
                continue 'outer;
            }
        }
        // No track was clamped this iteration → assign candidates and exit.
        for &i in &flexible {
            let w = match tracks[i].size {
                Sizing::Fill(w) => w.max(0.0),
                _ => unreachable!(),
            };
            sizes[i] = remaining * w / flexible_weight;
        }
        break;
    }
    // Any leftover flexible items with zero weight collapse to 0 (already set).
    sizes
}

/// Compute size + offset along one axis given the child's alignment, its
/// declared sizing, intrinsic desired size, and the inner span available.
/// Used for both stack cross-axis placement and ZStack per-axis placement.
fn place_axis(align: AxisAlign, sizing: Sizing, desired: f32, inner: f32) -> (f32, f32) {
    let stretch = matches!(align, AxisAlign::Stretch)
        || (matches!(align, AxisAlign::Auto) && matches!(sizing, Sizing::Fill(_)));
    let size = if stretch { inner } else { desired };
    let offset = match align {
        AxisAlign::Center => ((inner - size) * 0.5).max(0.0),
        AxisAlign::End => (inner - size).max(0.0),
        _ => 0.0,
    };
    (size, offset)
}

#[cfg(test)]
mod tests;
