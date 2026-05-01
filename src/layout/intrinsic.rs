//! Intrinsic-dimensions queries — the on-demand `LenReq` API spec'd in
//! `docs/intrinsics.md`.
//!
//! Drivers (Grid Auto track sizing, Stack Fill distribution) ask
//! `engine.intrinsic(node, axis, req)` to find a child's preferred size on
//! one axis under a given content-sizing mode. Answers are pure functions
//! of the subtree (no parent-state dependency), so the engine caches them
//! per frame.
//!
//! No production code consumes intrinsics yet (Step A in the plan): this
//! module exists so Steps B/C can wire Grid + Stack to it without
//! re-debating the API shape.

use super::{Axis, LayoutEngine, LayoutMode};
use crate::element::LayoutCore;
use crate::primitives::{Size, Sizing};
use crate::shape::Shape;
use crate::text::TextMeasurer;
use crate::tree::{NodeId, Tree};

/// Intrinsic content-size kind, per CSS Grid spec terminology.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum LenReq {
    /// Smallest size the node can occupy without breaking. Text: longest
    /// unbreakable run.
    MinContent,
    /// Size the node "wants" with unlimited room. Text: natural unbroken
    /// width.
    MaxContent,
}

/// One intrinsic query — what the cache keys on. `f32` answers are
/// indexed by these.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub struct IntrinsicQuery {
    pub node: NodeId,
    pub axis: Axis,
    pub req: LenReq,
}

/// Outer intrinsic on `axis`: content + padding + margin, respecting the
/// node's `Sizing` override and `min_size` / `max_size` clamps.
///
/// Pure function of the subtree at `node`. Engine caches the result; this
/// function is the cache miss path.
pub(super) fn compute(
    engine: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    axis: Axis,
    req: LenReq,
    text: &mut TextMeasurer,
) -> f32 {
    if tree.is_collapsed(node) {
        return 0.0;
    }

    let style = *tree.layout(node);
    let extras = tree.read_extras(node);

    let sizing = axis.main_sizing(style.size);
    let pad = axis_pad(axis, &style);
    let margin = axis_margin(axis, &style);
    let (min_clamp, max_clamp) = axis_clamps(axis, extras.min_size, extras.max_size);

    let outer = match sizing {
        // Fixed size dominates — no need to query content.
        Sizing::Fixed(v) => v + margin,
        // Hug + Fill both report content-driven intrinsic. Per `docs/
        // intrinsics.md` "Step B design commitments": Fill in intrinsic
        // context returns its content's intrinsic, ignoring weight.
        Sizing::Hug | Sizing::Fill(_) => {
            let content = content_intrinsic(engine, tree, node, axis, req, text, style.mode);
            content + pad + margin
        }
    };

    (outer - margin).max(0.0).clamp(min_clamp, max_clamp) + margin
}

fn content_intrinsic(
    engine: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    axis: Axis,
    req: LenReq,
    text: &mut TextMeasurer,
    mode: LayoutMode,
) -> f32 {
    match mode {
        LayoutMode::Leaf => leaf(tree, node, axis, req, text),
        LayoutMode::HStack => stack(engine, tree, node, Axis::X, axis, req, text),
        LayoutMode::VStack => stack(engine, tree, node, Axis::Y, axis, req, text),
        LayoutMode::ZStack => zstack(engine, tree, node, axis, req, text),
        LayoutMode::Canvas => canvas(engine, tree, node, axis, req, text),
        LayoutMode::Grid(idx) => grid(engine, tree, node, idx, axis, req, text),
    }
}

/// Leaf: walk shapes and aggregate. Only `Shape::Text` contributes
/// non-zero intrinsics today; other shapes are owner-relative paint.
fn leaf(tree: &Tree, node: NodeId, axis: Axis, req: LenReq, text: &mut TextMeasurer) -> f32 {
    let mut acc = 0.0_f32;
    for shape in tree.shapes_of(node) {
        if let Shape::Text {
            text: src,
            font_size_px,
            ..
        } = shape
        {
            let m = text.measure(src, *font_size_px, None);
            let v = match (axis, req) {
                (Axis::X, LenReq::MinContent) => m.intrinsic_min,
                (Axis::X, LenReq::MaxContent) => m.size.w,
                (Axis::Y, _) => m.size.h,
            };
            acc = acc.max(v);
        }
    }
    acc
}

/// Stack on `main_axis`. When the query asks for `main_axis`, sum
/// children's intrinsic on that axis plus gaps. Otherwise (cross axis),
/// max over children's intrinsic.
fn stack(
    engine: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    main_axis: Axis,
    query_axis: Axis,
    req: LenReq,
    text: &mut TextMeasurer,
) -> f32 {
    let gap = tree.read_extras(node).gap;
    if main_axis == query_axis {
        let mut total = 0.0_f32;
        let mut count = 0_usize;
        let mut kids = tree.child_cursor(node);
        while let Some(c) = kids.next(tree) {
            if tree.is_collapsed(c) {
                continue;
            }
            total += engine.intrinsic(tree, c, query_axis, req, text);
            count += 1;
        }
        total + gap * count.saturating_sub(1) as f32
    } else {
        let mut max = 0.0_f32;
        let mut kids = tree.child_cursor(node);
        while let Some(c) = kids.next(tree) {
            if tree.is_collapsed(c) {
                continue;
            }
            max = max.max(engine.intrinsic(tree, c, query_axis, req, text));
        }
        max
    }
}

/// ZStack: max over children on both axes. Children stack at the same
/// origin, so the parent hugs the largest child.
fn zstack(
    engine: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    axis: Axis,
    req: LenReq,
    text: &mut TextMeasurer,
) -> f32 {
    let mut max = 0.0_f32;
    let mut kids = tree.child_cursor(node);
    while let Some(c) = kids.next(tree) {
        if tree.is_collapsed(c) {
            continue;
        }
        max = max.max(engine.intrinsic(tree, c, axis, req, text));
    }
    max
}

/// Canvas: max over `(child.position + child.intrinsic)` on each axis,
/// matching how `canvas::measure` computes content size.
fn canvas(
    engine: &mut LayoutEngine,
    tree: &Tree,
    node: NodeId,
    axis: Axis,
    req: LenReq,
    text: &mut TextMeasurer,
) -> f32 {
    let mut max = 0.0_f32;
    let mut kids = tree.child_cursor(node);
    while let Some(c) = kids.next(tree) {
        if tree.is_collapsed(c) {
            continue;
        }
        let pos = tree.read_extras(c).position;
        let off = match axis {
            Axis::X => pos.x,
            Axis::Y => pos.y,
        };
        max = max.max(off + engine.intrinsic(tree, c, axis, req, text));
    }
    max
}

/// Grid: per-track intrinsic ranges aggregated from span-1 cells, summed
/// across tracks plus gaps. Step B will refactor `grid::measure` to
/// consume these ranges; for Step A we just compute and return the sum so
/// callers querying a Grid get a sensible answer.
///
/// Mirrors `Track`'s `Sizing` interpretation:
/// - `Fixed(v)` track: contributes `v` (clamped to `Track.min`/`max`).
/// - `Hug` track: contributes `max over span-1 cells of the intrinsic`,
///   clamped to track clamps.
/// - `Fill(_)` track: contributes `Track.min` (no intrinsic content
///   anchor — Fill claims leftover at distribution time, not here).
fn grid(
    engine: &mut LayoutEngine,
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

    // Per-track contribution: start from Sizing's "natural" value, then
    // pull span-1 children's intrinsics for Hug tracks.
    let mut track_size = vec![0.0_f32; n_tracks];
    for (i, t) in tracks.iter().enumerate() {
        track_size[i] = match t.size {
            Sizing::Fixed(v) => v.clamp(t.min, t.max),
            _ => t.min, // Hug starts at Track.min, grown by children below.
                        // Fill stays at Track.min (no content anchor).
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
            continue; // Span > 1 excluded — see docs/intrinsics.md §B.3.
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
            continue; // Only Hug tracks consume children's intrinsics.
        }
        let child_v = engine.intrinsic(tree, c, axis, req, text);
        track_size[track_idx] = track_size[track_idx].max(child_v.clamp(t.min, t.max));
    }

    let total: f32 = track_size.iter().sum();
    total + gap * n_tracks.saturating_sub(1) as f32
}

fn axis_pad(axis: Axis, style: &LayoutCore) -> f32 {
    match axis {
        Axis::X => style.padding.horiz(),
        Axis::Y => style.padding.vert(),
    }
}

fn axis_margin(axis: Axis, style: &LayoutCore) -> f32 {
    match axis {
        Axis::X => style.margin.horiz(),
        Axis::Y => style.margin.vert(),
    }
}

fn axis_clamps(axis: Axis, min: Size, max: Size) -> (f32, f32) {
    match axis {
        Axis::X => (min.w, max.w),
        Axis::Y => (min.h, max.h),
    }
}
