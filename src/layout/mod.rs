use crate::element::{LayoutCore, LayoutMode};
use crate::primitives::{Align, AxisAlign, Rect, Size, Sizes, Sizing};
use crate::shape::{Shape, TextWrap};
use crate::text::TextMeasurer;
use crate::tree::{NodeId, Tree};
use glam::Vec2;
use grid::GridContext;
use std::collections::HashMap;

mod axis;
mod canvas;
mod grid;
mod intrinsic;
mod result;
mod stack;
mod zstack;

pub use axis::Axis;
pub use intrinsic::{IntrinsicQuery, LenReq};
pub use result::{LayoutResult, ShapedText};

/// Persistent layout engine. Holds intermediate per-frame scratch + the
/// `LayoutResult` the encoder reads after layout. All allocations retain
/// capacity across frames.
///
/// - `grid` — grid-driver scratch (per-depth track state, hug pool).
/// - `desired` — measure-pass output read by arrange. Pure measure→arrange
///   handoff; nothing outside layout reads it (yet — `Ui::desired(id)`
///   exposes it for future debug/devtools but no current consumer).
/// - `intrinsics` — intra-frame cache for `intrinsic(node, axis, req)`
///   queries (see `intrinsic.md`). Pure function of subtree;
///   safe to memoize within a frame. Cleared in `run`.
/// - `result` — post-layout output (rects, text shapes) read by the encoder
///   + hit-index.
#[derive(Default)]
pub struct LayoutEngine {
    pub(super) grid: GridContext,
    desired: Vec<Size>,
    intrinsics: HashMap<IntrinsicQuery, f32>,
    result: LayoutResult,
}

impl LayoutEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn result(&self) -> &LayoutResult {
        &self.result
    }

    pub fn rect(&self, id: NodeId) -> Rect {
        self.result.rect(id)
    }

    pub fn desired(&self, id: NodeId) -> Size {
        self.desired[id.index()]
    }

    fn set_desired(&mut self, id: NodeId, v: Size) {
        self.desired[id.index()] = v;
    }

    /// On-demand intrinsic-size query — outer (margin-inclusive) size on
    /// `axis` under content-sizing `req`. See `intrinsic.md`.
    ///
    /// Pure function of the subtree at `node`: doesn't depend on the
    /// parent's available width or the arranged rect. Memoized via the
    /// intra-frame cache so repeated queries during the same `run` cost
    /// one HashMap lookup.
    ///
    /// Step A scaffolding: this method exists, drivers can call it, but
    /// nothing in the production measure/arrange path consumes intrinsics
    /// yet. Steps B and C wire Grid + Stack to it.
    pub fn intrinsic(
        &mut self,
        tree: &Tree,
        node: NodeId,
        axis: Axis,
        req: LenReq,
        text: &mut TextMeasurer,
    ) -> f32 {
        let key = IntrinsicQuery { node, axis, req };
        if let Some(&v) = self.intrinsics.get(&key) {
            return v;
        }
        let v = intrinsic::compute(self, tree, node, axis, req, text);
        self.intrinsics.insert(key, v);
        v
    }

    /// Run measure + arrange for `root` given the surface rect. Reuses
    /// internal scratch — call this each frame for amortized zero-alloc
    /// layout (after warmup). Output lands in `self.result`.
    ///
    /// `text` carries the shaper (or the mono fallback inside it) and is
    /// borrowed for the duration of the call so wrapping leaves can reshape
    /// against the parent-committed width during measure.
    pub fn run(&mut self, tree: &Tree, root: NodeId, surface: Rect, text: &mut TextMeasurer) {
        assert_eq!(
            self.grid.depth_stack.depth(),
            0,
            "LayoutEngine::run entered with non-zero grid depth"
        );
        let n = tree.node_count();
        self.desired.clear();
        self.desired.resize(n, Size::ZERO);
        self.intrinsics.clear();
        self.result.resize_for(tree);
        self.grid.hugs.reset_for(tree);
        self.measure(
            tree,
            root,
            Size::new(surface.width(), surface.height()),
            text,
        );
        self.arrange(tree, root, surface);
        assert_eq!(
            self.grid.depth_stack.depth(),
            0,
            "LayoutEngine::run exited with non-zero grid depth"
        );
    }

    /// Bottom-up measure dispatcher. Children call back via this method to
    /// recurse. Stores `desired` for each visited node in `self.result`.
    pub(super) fn measure(
        &mut self,
        tree: &Tree,
        node: NodeId,
        available: Size,
        text: &mut TextMeasurer,
    ) -> Size {
        if tree.is_collapsed(node) {
            self.set_desired(node, Size::ZERO);
            return Size::ZERO;
        }
        let style = *tree.layout(node);
        let mode = style.mode;
        let extras = tree.read_extras(node);
        let (min_size, max_size) = (extras.min_size, extras.max_size);

        // For each axis: if this node has a declared `Fixed` size, that's the
        // outer width children see — `inner = fixed - padding`. Otherwise
        // (Hug / Fill) we propagate whatever the parent gave us. Without
        // this, a fixed-width parent above a wrapping child wouldn't
        // constrain the child's available width during measure, so wrapping
        // text would never reshape.
        let outer_w = match style.size.w {
            Sizing::Fixed(v) => v,
            _ => (available.w - style.margin.horiz()).max(0.0),
        };
        let outer_h = match style.size.h {
            Sizing::Fixed(v) => v,
            _ => (available.h - style.margin.vert()).max(0.0),
        };
        let inner_avail = Size::new(
            (outer_w - style.padding.horiz()).max(0.0),
            (outer_h - style.padding.vert()).max(0.0),
        );

        let content = match mode {
            LayoutMode::Leaf => self.leaf_content_size(tree, node, inner_avail.w, text),
            LayoutMode::HStack => {
                stack::measure(self, tree, node, inner_avail, stack::Axis::X, text)
            }
            LayoutMode::VStack => {
                stack::measure(self, tree, node, inner_avail, stack::Axis::Y, text)
            }
            LayoutMode::ZStack => zstack::measure(self, tree, node, inner_avail, text),
            LayoutMode::Canvas => canvas::measure(self, tree, node, inner_avail, text),
            LayoutMode::Grid(idx) => grid::measure(self, tree, node, idx, inner_avail, text),
        };

        let hug_w = content.w + style.padding.horiz() + style.margin.horiz();
        let hug_h = content.h + style.padding.vert() + style.margin.vert();
        let desired = Size::new(
            resolve_axis_size(
                style.size.w,
                hug_w,
                available.w,
                style.margin.horiz(),
                min_size.w,
                max_size.w,
            ),
            resolve_axis_size(
                style.size.h,
                hug_h,
                available.h,
                style.margin.vert(),
                min_size.h,
                max_size.h,
            ),
        );

        self.set_desired(node, desired);
        desired
    }

    /// Top-down arrange dispatcher. `slot` is the rect the parent reserved
    /// (margin-inclusive). Stores `rect` for each visited node in `self.result`.
    pub(super) fn arrange(&mut self, tree: &Tree, node: NodeId, slot: Rect) {
        if tree.is_collapsed(node) {
            zero_subtree(self, tree, node, slot.min);
            return;
        }
        let style = *tree.layout(node);
        let mode = style.mode;

        let rendered = slot.deflated_by(style.margin);
        self.result.set_rect(node, rendered);
        let inner = rendered.deflated_by(style.padding);

        match mode {
            LayoutMode::Leaf => {}
            LayoutMode::HStack => stack::arrange(self, tree, node, inner, stack::Axis::X),
            LayoutMode::VStack => stack::arrange(self, tree, node, inner, stack::Axis::Y),
            LayoutMode::ZStack => zstack::arrange(self, tree, node, inner),
            LayoutMode::Canvas => canvas::arrange(self, tree, node, inner),
            LayoutMode::Grid(idx) => grid::arrange(self, tree, node, inner, idx),
        }
    }
}

/// Resolve a node's outer slot size on one axis, given its sizing policy,
/// hug-content size (margin-inclusive), parent-supplied available, own margin,
/// and clamps. Each branch produces *rendered* size (margin-exclusive); we
/// clamp once and add margin once at the end.
///
/// Also reused by `intrinsic::compute` with `available = INFINITY`, which
/// collapses Fill to its content size — the parent-independent rule for
/// intrinsic queries (CSS Grid `1fr`-in-auto-context).
pub(super) fn resolve_axis_size(
    s: Sizing,
    hug_outer: f32,
    available: f32,
    margin: f32,
    min: f32,
    max: f32,
) -> f32 {
    let rendered = match s {
        Sizing::Fixed(v) => v,
        Sizing::Hug => hug_outer - margin,
        Sizing::Fill(_) => {
            // Fill in an unconstrained axis collapses to max-content
            // (matches CSS Grid: a `1fr` track with `width: auto` parent
            // resolves to its content size, not infinity).
            let outer = if available.is_finite() {
                available
            } else {
                hug_outer
            };
            outer - margin
        }
    };
    rendered.max(0.0).clamp(min, max) + margin
}

/// Set this node and every descendant to a zero-size rect anchored at
/// `anchor`. Walks the contiguous pre-order span `[node, subtree_end[node])`
/// directly — no recursion, no child cursors.
pub(super) fn zero_subtree(layout: &mut LayoutEngine, tree: &Tree, node: NodeId, anchor: Vec2) {
    let zero = Rect {
        min: anchor,
        size: Size::ZERO,
    };
    let start = node.index();
    let end = tree.subtree_ends()[start] as usize;
    for i in start..end {
        layout.result.set_rect(NodeId(i as u32), zero);
    }
}

/// Per-axis available size to pass to children of a panel that sizes per its
/// own `Sizing` on each axis: pass `inner_avail` on Fill/Fixed axes (children
/// see the committed slot), `INFINITY` on Hug axes (avoids recursive sizing).
/// Used by ZStack and Canvas. Stack uses a different rule (always INF on main).
pub(super) fn child_avail_per_axis_hug(size: Sizes, inner_avail: Size) -> Size {
    Size::new(
        if matches!(size.w, Sizing::Hug) {
            f32::INFINITY
        } else {
            inner_avail.w
        },
        if matches!(size.h, Sizing::Hug) {
            f32::INFINITY
        } else {
            inner_avail.h
        },
    )
}

/// How `place_axis` interprets `AxisAlign::Auto`.
#[derive(Copy, Clone, PartialEq, Eq)]
pub(super) enum AutoBias {
    /// Stack/ZStack: Auto stretches only when the child is `Sizing::Fill`.
    StretchIfFill,
    /// Grid: Auto stretches unconditionally (WPF cell default).
    AlwaysStretch,
}

impl LayoutEngine {
    /// Walk a Leaf's recorded shapes and return the content size that drives
    /// its hugging. For `Shape::Text` runs, this is also where shaping
    /// happens: the shaped buffer + measured size land on
    /// `LayoutResult.text_shapes` so the encoder can pick them up later.
    /// `available_w` flows down from the parent and gates wrapping.
    fn leaf_content_size(
        &mut self,
        tree: &Tree,
        node: NodeId,
        available_w: f32,
        text: &mut TextMeasurer,
    ) -> Size {
        let mut s = Size::ZERO;
        for shape in tree.shapes_of(node) {
            if let Shape::Text {
                text: src,
                font_size_px,
                wrap,
                ..
            } = shape
            {
                let m = self.shape_text(node, src, *font_size_px, *wrap, available_w, text);
                s = s.max(m);
            }
        }
        s
    }

    fn shape_text(
        &mut self,
        node: NodeId,
        src: &str,
        font_size_px: f32,
        wrap: TextWrap,
        available_w: f32,
        text: &mut TextMeasurer,
    ) -> Size {
        let unbounded = text.measure(src, font_size_px, None);
        let (measured, key) = if matches!(wrap, TextWrap::Wrap)
            && available_w.is_finite()
            && available_w < unbounded.size.w
        {
            // Floor at the widest unbreakable run so we don't break inside a
            // word — the run overflows the slot instead.
            let target = available_w.max(unbounded.intrinsic_min);
            tracing::trace!(node = node.index(), target, "wrap reshape");
            let m = text.measure(src, font_size_px, Some(target));
            (m.size, m.key)
        } else {
            (unbounded.size, unbounded.key)
        };

        self.result
            .set_text_shape(node, ShapedText { measured, key });
        measured
    }
}

/// Resolve a child's alignment on both axes: child's own value if not `Auto`,
/// else the parent's `child_align` for that axis. Single source of truth for
/// the alignment cascade — every layout (stack, grid, zstack) calls this so
/// they can't drift. Stack discards the unused axis; the cost is two enum
/// matches per child per frame.
pub(super) fn resolved_axis_align(
    child: &LayoutCore,
    parent_child_align: Align,
) -> (AxisAlign, AxisAlign) {
    let a = child.align;
    (
        a.halign().or(parent_child_align.halign()).to_axis(),
        a.valign().or(parent_child_align.valign()).to_axis(),
    )
}

/// Compute size + offset along one axis given the child's alignment, its
/// declared sizing, intrinsic desired size, and the inner span available.
/// Used for stack cross-axis, ZStack per-axis, and Grid per-cell placement.
/// `bias` selects the per-driver `AxisAlign::Auto` rule (see `AutoBias`).
pub(super) fn place_axis(
    align: AxisAlign,
    sizing: Sizing,
    desired: f32,
    inner: f32,
    bias: AutoBias,
) -> (f32, f32) {
    let stretch = matches!(align, AxisAlign::Stretch)
        || matches!(align, AxisAlign::Auto)
            && (matches!(bias, AutoBias::AlwaysStretch) || matches!(sizing, Sizing::Fill(_)));
    let size = if stretch { inner } else { desired };
    let offset = match align {
        AxisAlign::Center => ((inner - size) * 0.5).max(0.0),
        AxisAlign::End => (inner - size).max(0.0),
        _ => 0.0,
    };
    (size, offset)
}
